//! websocket_client_connection.rs のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - 状態遷移の整合性
//! - ハンドシェイク処理の堅牢性
//! - フレーム処理の堅牢性
//! - タイマー処理の整合性

use base64ct::{Base64, Encoding};
use proptest::prelude::*;
use sha1::{Digest, Sha1};
use shiguredo_websocket::{
    ClientConnectionOptions, CloseCode, ConnectionEvent, ConnectionOutput, ConnectionState, Frame,
    Opcode, PerMessageDeflateConfig, RandomSource, Timestamp, WebSocketClientConnection,
};

/// nonce から Sec-WebSocket-Accept を計算する
fn compute_accept(nonce: &[u8; 16]) -> String {
    let key = Base64::encode_string(nonce.as_slice());
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let hash = hasher.finalize();
    Base64::encode_string(hash.as_slice())
}

/// 有効なハンドシェイクレスポンスを生成
fn create_valid_handshake_response(
    accept: &str,
    protocol: Option<&str>,
    extensions: Option<&str>,
) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n",
        accept
    );
    if let Some(p) = protocol {
        response.push_str(&format!("Sec-WebSocket-Protocol: {}\r\n", p));
    }
    if let Some(e) = extensions {
        response.push_str(&format!("Sec-WebSocket-Extensions: {}\r\n", e));
    }
    response.push_str("\r\n");
    response.into_bytes()
}

/// テスト用の固定乱数ソース
struct FixedRandom {
    masking_key: [u8; 4],
    nonce: [u8; 16],
}

impl FixedRandom {
    fn new() -> Self {
        Self {
            masking_key: [0xAB, 0xCD, 0xEF, 0x12],
            nonce: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        }
    }

    fn with_nonce(nonce: [u8; 16]) -> Self {
        Self {
            masking_key: [0xAB, 0xCD, 0xEF, 0x12],
            nonce,
        }
    }
}

impl RandomSource for FixedRandom {
    fn masking_key(&mut self) -> [u8; 4] {
        self.masking_key
    }

    fn nonce(&mut self) -> [u8; 16] {
        self.nonce
    }
}

/// 接続を作成して Connected 状態まで進める
fn setup_connected_client() -> (WebSocketClientConnection<FixedRandom>, Timestamp, [u8; 16]) {
    let options = ClientConnectionOptions::new("example.com", "/ws");
    let random = FixedRandom::new();
    let nonce = random.nonce;
    let mut conn = WebSocketClientConnection::new(options, random);
    let now = Timestamp::from_millis(0);

    conn.connect().unwrap();

    // 出力を消費（ハンドシェイクリクエスト）
    while conn.poll_output().is_some() {}

    // Accept を計算してレスポンスを送信
    let accept = compute_accept(&nonce);
    let response = create_valid_handshake_response(&accept, None, None);
    conn.feed_recv_buf(&response, now).unwrap();

    // イベントと出力を消費
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    (conn, now, nonce)
}

// ==== ClientConnectionOptions のテスト ====

proptest! {
    /// ClientConnectionOptions::new でホストとパスが設定される
    #[test]
    fn prop_client_options_new(
        host in "[a-z]{1,20}\\.[a-z]{2,5}",
        path in "/[a-z0-9/]{0,30}",
    ) {
        let options = ClientConnectionOptions::new(&host, &path);
        prop_assert_eq!(options.host, host);
        prop_assert_eq!(options.path, path);
    }

    /// ClientConnectionOptions::origin が正しく設定される
    #[test]
    fn prop_client_options_origin(
        origin in "https://[a-z]{1,20}\\.[a-z]{2,5}",
    ) {
        let options = ClientConnectionOptions::new("example.com", "/")
            .origin(&origin);
        prop_assert_eq!(options.origin, Some(origin));
    }

    /// ClientConnectionOptions::protocol が複数回呼び出しても正しく蓄積される
    #[test]
    fn prop_client_options_multiple_protocols(
        protocols in prop::collection::vec("[a-z]{1,20}", 0..10)
    ) {
        let mut options = ClientConnectionOptions::new("example.com", "/");
        for p in &protocols {
            options = options.protocol(p);
        }
        prop_assert_eq!(options.protocols.len(), protocols.len());
        for (i, p) in protocols.iter().enumerate() {
            prop_assert_eq!(&options.protocols[i], p);
        }
    }

    /// ClientConnectionOptions::header が複数回呼び出しても正しく蓄積される
    #[test]
    fn prop_client_options_multiple_headers(
        headers in prop::collection::vec(("[a-zA-Z-]{1,20}", "[a-zA-Z0-9 ]{0,50}"), 0..10)
    ) {
        let mut options = ClientConnectionOptions::new("example.com", "/");
        for (name, value) in &headers {
            options = options.header(name, value);
        }
        prop_assert_eq!(options.additional_headers.len(), headers.len());
    }

    /// ping_interval は任意の値を設定可能
    #[test]
    fn prop_client_options_ping_interval(interval in 0u64..=u64::MAX) {
        let options = ClientConnectionOptions::new("example.com", "/")
            .ping_interval(interval);
        prop_assert_eq!(options.ping_interval_millis, interval);
    }
}

// ==== 接続開始テスト ====

proptest! {
    /// connect() で状態が Connecting に変わる
    #[test]
    fn prop_connect_changes_state_to_connecting(
        host in "[a-z]{1,20}\\.[a-z]{2,5}",
        path in "/[a-z0-9/]{0,30}",
    ) {
        let options = ClientConnectionOptions::new(&host, &path);
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        conn.connect().unwrap();
        prop_assert_eq!(conn.state(), ConnectionState::Connecting);
    }

    /// connect() でハンドシェイクリクエストが送信される
    #[test]
    fn prop_connect_sends_handshake_request(
        host in "[a-z]{1,20}\\.[a-z]{2,5}",
        path in "/[a-z0-9/]{0,30}",
    ) {
        let options = ClientConnectionOptions::new(&host, &path);
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        conn.connect().unwrap();
        let output = conn.poll_output().unwrap();
        match output {
            ConnectionOutput::SendData(data) => {
                let request = String::from_utf8_lossy(&data);
                let expected_path = format!("GET {} HTTP/1.1", path);
                let expected_host = format!("Host: {}", host);
                prop_assert!(request.contains(expected_path.as_str()));
                prop_assert!(request.contains(expected_host.as_str()));
                prop_assert!(request.contains("Upgrade: websocket"));
                prop_assert!(request.contains("Connection: Upgrade"));
                prop_assert!(request.contains("Sec-WebSocket-Key:"));
                prop_assert!(request.contains("Sec-WebSocket-Version: 13"));
            }
            _ => panic!("expected SendData"),
        }
    }

    /// 二重接続はエラー
    #[test]
    fn prop_double_connect_fails(
        host in "[a-z]{1,20}\\.[a-z]{2,5}",
        path in "/[a-z0-9/]{0,30}",
    ) {
        let options = ClientConnectionOptions::new(&host, &path);
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        conn.connect().unwrap();
        let result = conn.connect();
        prop_assert!(result.is_err());
    }
}

// ==== ハンドシェイク完了テスト ====

proptest! {
    /// 有効なハンドシェイクで Connected 状態になる
    #[test]
    fn prop_valid_handshake_connects(nonce in any::<[u8; 16]>()) {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let random = FixedRandom::with_nonce(nonce);
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);
        conn.connect().unwrap();
        while conn.poll_output().is_some() {}
        let accept = compute_accept(&nonce);
        let response = create_valid_handshake_response(&accept, None, None);
        conn.feed_recv_buf(&response, now).unwrap();
        while conn.poll_event().is_some() {}
        while conn.poll_output().is_some() {}
        prop_assert_eq!(conn.state(), ConnectionState::Connected);
    }
}

proptest! {
    /// プロトコル付きのハンドシェイクが正しく処理される
    #[test]
    fn prop_handshake_with_protocol(
        protocol in "[a-z]{1,20}",
    ) {
        let options = ClientConnectionOptions::new("example.com", "/ws")
            .protocol(&protocol);
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        let now = Timestamp::from_millis(0);

        conn.connect().unwrap();

        // リクエストから nonce を取得
        let output = conn.poll_output().unwrap();
        let request_data = match output {
            ConnectionOutput::SendData(data) => data,
            _ => panic!("expected SendData"),
        };
        let request_str = String::from_utf8_lossy(&request_data);

        // プロトコルがリクエストに含まれることを確認
        let expected_protocol_header = format!("Sec-WebSocket-Protocol: {}", protocol);
        prop_assert!(request_str.contains(&expected_protocol_header));

        let key_line = request_str.lines()
            .find(|l| l.starts_with("Sec-WebSocket-Key:"))
            .unwrap();
        let key = key_line.split(": ").nth(1).unwrap().trim();
        let nonce_vec = Base64::decode_vec(key).unwrap();
        let mut nonce = [0u8; 16];
        nonce.copy_from_slice(&nonce_vec);

        let accept = compute_accept(&nonce);
        let response = create_valid_handshake_response(&accept, Some(&protocol), None);
        conn.feed_recv_buf(&response, now).unwrap();

        prop_assert_eq!(conn.state(), ConnectionState::Connected);
        prop_assert_eq!(conn.protocol(), Some(protocol.as_str()));
    }

    /// 無効な Accept はエラー
    #[test]
    fn prop_invalid_accept_fails(invalid_accept in "[a-zA-Z0-9+/=]{20,40}") {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);
        conn.connect().unwrap();
        while conn.poll_output().is_some() {}
        let valid_accept = compute_accept(&nonce);
        prop_assume!(invalid_accept != valid_accept);
        let response = create_valid_handshake_response(&invalid_accept, None, None);
        let result = conn.feed_recv_buf(&response, now);
        prop_assert!(result.is_err());
    }

    /// サポートされていないプロトコルのハンドシェイク (RFC 6455 準拠)
    #[test]
    fn prop_handshake_unsupported_protocol(
        client_protocol in "[a-z]{3,10}",
        server_protocol in "[a-z]{3,10}"
    ) {
        // 異なるプロトコルの場合のみテスト
        prop_assume!(client_protocol != server_protocol);

        let options = ClientConnectionOptions::new("example.com", "/ws")
            .protocol(&client_protocol);
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);

        conn.connect().unwrap();
        while conn.poll_output().is_some() {}

        // サーバーがクライアントの要求と異なるプロトコルを返す
        let accept = compute_accept(&nonce);
        let response = create_valid_handshake_response(&accept, Some(&server_protocol), None);
        let result = conn.feed_recv_buf(&response, now);

        // RFC 6455: クライアントが要求していないプロトコルは拒否
        prop_assert!(result.is_err(), "Server returned unsolicited protocol should be rejected");
    }
}

// ==== データ送受信テスト ====

proptest! {
    /// テキストメッセージの送信
    #[test]
    fn prop_send_text_message(
        text in ".*",
    ) {
        let (mut conn, _, _) = setup_connected_client();

        let result = conn.send_text(&text);
        prop_assert!(result.is_ok());

        let output = conn.poll_output();
        prop_assert!(matches!(output, Some(ConnectionOutput::SendData(_))));
    }

    /// バイナリメッセージの送信
    #[test]
    fn prop_send_binary_message(
        data in prop::collection::vec(any::<u8>(), 0..1000),
    ) {
        let (mut conn, _, _) = setup_connected_client();

        let result = conn.send_binary(&data);
        prop_assert!(result.is_ok());

        let output = conn.poll_output();
        prop_assert!(matches!(output, Some(ConnectionOutput::SendData(_))));
    }

    /// テキストフレームの受信（サーバーからはマスクなし）
    #[test]
    fn prop_receive_text_frame(
        text in "[\\x20-\\x7E]{0,100}",
    ) {
        let (mut conn, now, _) = setup_connected_client();

        // サーバーからのフレーム（マスクなし）
        let frame = Frame::text(&text).encode_unmasked();
        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_ok());

        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(received) = event {
                prop_assert_eq!(received, text);
                found = true;
                break;
            }
        }
        prop_assert!(found, "TextMessage event not found");
    }

    /// バイナリフレームの受信
    #[test]
    fn prop_receive_binary_frame(
        data in prop::collection::vec(any::<u8>(), 0..1000),
    ) {
        let (mut conn, now, _) = setup_connected_client();

        let frame = Frame::binary(data.clone()).encode_unmasked();
        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_ok());

        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::BinaryMessage(received) = event {
                prop_assert_eq!(received, data);
                found = true;
                break;
            }
        }
        prop_assert!(found, "BinaryMessage event not found");
    }
}

// ==== Ping/Pong のテスト ====

proptest! {
    /// Ping を受信すると Pong を自動返信する
    #[test]
    fn prop_ping_auto_pong(
        data in prop::collection::vec(any::<u8>(), 0..125),
    ) {
        let (mut conn, now, _) = setup_connected_client();

        let ping = Frame::ping(data.clone()).unwrap().encode_unmasked();
        conn.feed_recv_buf(&ping, now).unwrap();

        // Ping イベントが発生
        let mut ping_found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Ping(received) = event {
                prop_assert_eq!(received, data);
                ping_found = true;
                break;
            }
        }
        prop_assert!(ping_found, "Ping event not found");

        // Pong が送信される
        let mut pong_found = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(_) = output {
                pong_found = true;
                break;
            }
        }
        prop_assert!(pong_found, "Pong response not sent");
    }

    /// Pong を受信すると awaiting_pong がリセットされる
    #[test]
    fn prop_pong_clears_awaiting(
        data in prop::collection::vec(any::<u8>(), 0..125),
    ) {
        let (mut conn, now, _) = setup_connected_client();

        // まず Ping を送信
        conn.send_ping(&[]).unwrap();
        while conn.poll_output().is_some() {}

        // Pong を受信
        let pong = Frame::pong(data.clone()).unwrap().encode_unmasked();
        conn.feed_recv_buf(&pong, now).unwrap();

        // Pong イベントが発生
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Pong(received) = event {
                prop_assert_eq!(received, data);
                found = true;
                break;
            }
        }
        prop_assert!(found, "Pong event not found");
    }

    /// send_pong で送信した Pong フレームが FIN=1, RSV=0, Opcode=Pong, masked の形式で出力される
    /// RFC 6455 §5.5.3: unsolicited Pong フレームの送信を許可する
    #[test]
    fn prop_send_pong_emits_pong_frame(
        data in prop::collection::vec(any::<u8>(), 0..=125),
    ) {
        let (mut conn, _now, _) = setup_connected_client();

        // 接続セットアップ時の出力を捨てる
        while conn.poll_output().is_some() {}

        conn.send_pong(&data).unwrap();

        let mut sent = Vec::new();
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(buf) = output {
                sent.extend_from_slice(&buf);
            }
        }
        prop_assert!(sent.len() >= 6, "Pong frame is at least 6 bytes (header + 4 byte mask)");

        // FIN=1, RSV=0, Opcode=Pong
        prop_assert_eq!(sent[0], 0x80 | Opcode::Pong.as_u8());
        // クライアント側マスクビット=1、ペイロード長 (0..=125 の範囲なので 7bit に収まる)
        prop_assert_eq!(sent[1] & 0x80, 0x80);
        prop_assert_eq!(sent[1] & 0x7F, data.len() as u8);
    }
}

// ==== Close のテスト ====

/// 有効な Close コードを生成する strategy
fn valid_close_code() -> impl Strategy<Value = u16> {
    prop_oneof![
        1000u16..=1003, // 正常終了系
        1007u16..=1011, // エラー系
        3000u16..4000,  // ライブラリ用
        4000u16..5000,  // アプリケーション用
    ]
}

proptest! {
    /// Close フレームを受信すると Close イベントが発生する
    #[test]
    fn prop_close_frame_received(
        code in valid_close_code(),
        reason in "[\\x20-\\x7E]{0,50}".prop_map(|s| s.to_string()),
    ) {
        let (mut conn, now, _) = setup_connected_client();

        let close = Frame::close(Some(code), &reason).unwrap().encode_unmasked();
        conn.feed_recv_buf(&close, now).unwrap();

        // Close イベントが発生
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Close { code: Some(c), reason: r } = event {
                prop_assert_eq!(c.as_u16(), code);
                prop_assert_eq!(r, reason);
                found = true;
                break;
            }
        }
        prop_assert!(found, "Close event not found");

        // 状態が Closed に変わる
        prop_assert_eq!(conn.state(), ConnectionState::Closed);
    }

    /// close() を呼び出すと Close フレームが送信される
    #[test]
    fn prop_close_sends_frame(
        code in prop::sample::select(vec![
            CloseCode::NORMAL,
            CloseCode::GOING_AWAY,
            CloseCode::PROTOCOL_ERROR,
        ]),
        reason in "[\\x20-\\x7E]{0,50}".prop_map(|s| s.to_string()),
    ) {
        let (mut conn, _, _) = setup_connected_client();

        conn.close(code, &reason).unwrap();

        prop_assert_eq!(conn.state(), ConnectionState::Closing);

        let mut found = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(_) = output {
                found = true;
                break;
            }
        }
        prop_assert!(found, "Close frame not sent");
    }

    /// Connecting 状態で close() を呼び出すとエラーになる
    ///
    /// RFC 6455 Section 7.1.2: Close フレームは established connection 上でのみ送信可能
    #[test]
    fn prop_close_rejected_in_connecting_state(
        code in prop::sample::select(vec![
            CloseCode::NORMAL,
            CloseCode::GOING_AWAY,
            CloseCode::PROTOCOL_ERROR,
        ]),
        reason in "[\\x20-\\x7E]{0,50}".prop_map(|s| s.to_string()),
    ) {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let random = FixedRandom::new();
        let mut conn = WebSocketClientConnection::new(options, random);

        conn.connect().unwrap();
        // ハンドシェイクレスポンスを送らず Connecting 状態のまま
        prop_assert_eq!(conn.state(), ConnectionState::Connecting);

        // close() はエラーになるはず
        let result = conn.close(code, &reason);
        prop_assert!(result.is_err(), "close() should fail in Connecting state");

        // フレームが出力されていないこと
        while conn.poll_output().is_some() {}
        prop_assert_eq!(conn.state(), ConnectionState::Connecting);
    }
}

// ==== 状態遷移のテスト ====

proptest! {
    /// Disconnected 状態では送信できない
    #[test]
    fn prop_cannot_send_while_disconnected(text in ".*") {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        let result = conn.send_text(&text);
        prop_assert!(result.is_err());
    }

    /// Connecting 状態では送信できない
    #[test]
    fn prop_cannot_send_while_connecting(text in ".*") {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        conn.connect().unwrap();
        prop_assert_eq!(conn.state(), ConnectionState::Connecting);
        let result = conn.send_text(&text);
        prop_assert!(result.is_err());
    }
}

proptest! {
    /// Disconnected 状態ではデータを受信できない
    #[test]
    fn prop_feed_to_disconnected_fails(data in prop::collection::vec(any::<u8>(), 1..100)) {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        let now = Timestamp::from_millis(0);
        let result = conn.feed_recv_buf(&data, now);
        prop_assert!(result.is_err());
    }

    /// Closed 状態ではデータを受信できない
    #[test]
    fn prop_feed_to_closed_fails(extra in prop::collection::vec(any::<u8>(), 1..50)) {
        let (mut conn, now, _) = setup_connected_client();
        let close = Frame::close(Some(1000), "").unwrap().encode_unmasked();
        conn.feed_recv_buf(&close, now).unwrap();
        prop_assert_eq!(conn.state(), ConnectionState::Closed);
        let result = conn.feed_recv_buf(&extra, now);
        prop_assert!(result.is_err());
    }
}

// ==== RSV ビットのテスト ====

proptest! {
    /// RSV2 ビットが立ったフレームは拒否される
    #[test]
    fn prop_rsv2_bit_rejected(payload in prop::collection::vec(any::<u8>(), 0..50)) {
        let (mut conn, now, _) = setup_connected_client();
        // RSV2 bit: 0xA1 = FIN + RSV2 + text opcode, サーバーからはマスクなし
        let mut frame = vec![0xA1, payload.len() as u8];
        frame.extend_from_slice(&payload);
        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_err());
    }

    /// RSV3 ビットが立ったフレームは拒否される
    #[test]
    fn prop_rsv3_bit_rejected(payload in prop::collection::vec(any::<u8>(), 0..50)) {
        let (mut conn, now, _) = setup_connected_client();
        // RSV3 bit: 0x91 = FIN + RSV3 + text opcode
        let mut frame = vec![0x91, payload.len() as u8];
        frame.extend_from_slice(&payload);
        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_err());
    }

    /// permessage-deflate なしで RSV1 ビットは拒否される
    #[test]
    fn prop_rsv1_without_deflate_rejected(payload in prop::collection::vec(any::<u8>(), 0..50)) {
        let (mut conn, now, _) = setup_connected_client();
        // RSV1 bit: 0xC1 = FIN + RSV1 + text opcode
        let mut frame = vec![0xC1, payload.len() as u8];
        frame.extend_from_slice(&payload);
        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_err());
    }
}

// ==== フラグメント処理のテスト ====

proptest! {
    /// フラグメントされたテキストメッセージが正しく再構築される
    #[test]
    fn prop_fragmented_text_message(
        parts in prop::collection::vec("[\\x20-\\x7E]{1,50}", 2..5),
    ) {
        let (mut conn, now, _) = setup_connected_client();

        let full_text: String = parts.iter().cloned().collect();

        // 最初のフラグメント（opcode=Text, fin=false）
        let first_frame = {
            let mut frame = Frame::text(&parts[0]);
            frame.fin = false;
            frame.encode_unmasked()
        };
        conn.feed_recv_buf(&first_frame, now).unwrap();

        // 中間のフラグメント
        for part in parts.iter().take(parts.len() - 1).skip(1) {
            let mut frame = Frame::new(Opcode::Continuation, part.as_bytes().to_vec());
            frame.fin = false;
            let encoded = frame.encode_unmasked();
            conn.feed_recv_buf(&encoded, now).unwrap();
        }

        // 最後のフラグメント
        let last_idx = parts.len() - 1;
        let mut frame = Frame::new(Opcode::Continuation, parts[last_idx].as_bytes().to_vec());
        frame.fin = true;
        let encoded = frame.encode_unmasked();
        conn.feed_recv_buf(&encoded, now).unwrap();

        // 完全なメッセージが受信される
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(received) = event {
                prop_assert_eq!(received, full_text);
                found = true;
                break;
            }
        }
        prop_assert!(found, "Complete message not received");
    }

    /// 開始フレームなしで Continuation は失敗
    #[test]
    fn prop_continuation_without_start_fails(data in prop::collection::vec(any::<u8>(), 0..100)) {
        let (mut conn, now, _) = setup_connected_client();
        let cont = Frame::new(Opcode::Continuation, data).encode_unmasked();
        let result = conn.feed_recv_buf(&cont, now);
        prop_assert!(result.is_err());
    }

    /// RFC 6455 Section 5.4: フラグメント中に新しいデータフレームは禁止
    #[test]
    fn prop_new_message_during_fragment_fails(
        first_part in "[\\x20-\\x7E]{1,50}",
        new_message in "[\\x20-\\x7E]{1,50}",
    ) {
        let (mut conn, now, _) = setup_connected_client();
        let mut frame1 = Frame::text(&first_part);
        frame1.fin = false;
        conn.feed_recv_buf(&frame1.encode_unmasked(), now).unwrap();
        let frame2 = Frame::text(&new_message).encode_unmasked();
        let result = conn.feed_recv_buf(&frame2, now);
        prop_assert!(result.is_err());
    }
}

// ==== 部分的なデータ受信のテスト ====

proptest! {
    /// データを小さなチャンクで分割して送っても正しく処理される
    #[test]
    fn prop_chunked_frame_reception(
        text in "[\\x20-\\x7E]{10,100}",
        chunk_size in 1usize..10,
    ) {
        let (mut conn, now, _) = setup_connected_client();

        let frame = Frame::text(&text).encode_unmasked();

        // チャンクに分割して送信
        for chunk in frame.chunks(chunk_size) {
            conn.feed_recv_buf(chunk, now).unwrap();
        }

        // メッセージが正しく受信される
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(received) = event {
                prop_assert_eq!(received, text);
                found = true;
                break;
            }
        }
        prop_assert!(found, "Message not received after chunked delivery");
    }
}

// ==== deflate 統合テスト ====

/// deflate 対応接続を作成して Connected 状態まで進める
fn setup_connected_client_with_deflate()
-> (WebSocketClientConnection<FixedRandom>, Timestamp, [u8; 16]) {
    let options =
        ClientConnectionOptions::new("example.com", "/ws").deflate(PerMessageDeflateConfig::new());
    let random = FixedRandom::new();
    let nonce = random.nonce;
    let mut conn = WebSocketClientConnection::new(options, random);
    let now = Timestamp::from_millis(0);

    conn.connect().unwrap();
    while conn.poll_output().is_some() {}

    let accept = compute_accept(&nonce);
    let response = create_valid_handshake_response(&accept, None, Some("permessage-deflate"));
    conn.feed_recv_buf(&response, now).unwrap();

    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    (conn, now, nonce)
}

proptest! {
    /// 圧縮テキスト送信（RSV1 フラグとペイロード検証）
    #[test]
    fn prop_deflate_send_compressed_text(
        text in "[\\x20-\\x7E]{65,200}",
    ) {
        let (mut conn, _, _) = setup_connected_client_with_deflate();

        conn.send_text(&text).unwrap();

        let output = conn.poll_output().unwrap();
        match output {
            ConnectionOutput::SendData(data) => {
                // RSV1 が設定されていること（圧縮フレーム）
                prop_assert_eq!(data[0] & 0x40, 0x40, "RSV1 should be set for compressed messages");
                prop_assert_eq!(data[0] & 0x0F, 0x01, "opcode should be text");
            }
            _ => panic!("expected SendData"),
        }
    }

    /// 圧縮テキスト受信（解凍検証）
    #[test]
    fn prop_deflate_receive_compressed_text(
        text in "[\\x20-\\x7E]{1,200}",
    ) {
        let (mut conn, now, _) = setup_connected_client_with_deflate();

        // サーバー側コーデックでテキストを圧縮
        let mut server_deflate =
            shiguredo_websocket::PerMessageDeflate::new_server(PerMessageDeflateConfig::new());
        let compressed = server_deflate.compress(text.as_bytes()).unwrap();

        // 圧縮フレームを構築: FIN=1, RSV1=1, opcode=text(1)
        let mut frame = vec![0xC1]; // FIN + RSV1 + text
        if compressed.len() >= 126 {
            frame.push(126);
            frame.extend_from_slice(&(compressed.len() as u16).to_be_bytes());
        } else {
            frame.push(compressed.len() as u8);
        }
        frame.extend_from_slice(&compressed);

        conn.feed_recv_buf(&frame, now).unwrap();

        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(received) = event {
                prop_assert_eq!(received, text);
                found = true;
                break;
            }
        }
        prop_assert!(found, "TextMessage event not found");
    }

    /// 小メッセージの非圧縮（しきい値以下）
    #[test]
    fn prop_deflate_small_message_not_compressed(
        text in "[\\x20-\\x7E]{1,10}",
    ) {
        let (mut conn, _, _) = setup_connected_client_with_deflate();

        conn.send_text(&text).unwrap();

        let output = conn.poll_output().unwrap();
        match output {
            ConnectionOutput::SendData(data) => {
                // RSV1 が設定されていないこと（非圧縮）
                prop_assert_eq!(data[0] & 0x40, 0x00, "RSV1 should not be set for small messages");
            }
            _ => panic!("expected SendData"),
        }
    }

    /// Zip Bomb 保護（最大解凍サイズ超過）
    #[test]
    fn prop_deflate_zip_bomb_protection(
        repeat_count in 3usize..10,
    ) {
        let max_decompressed = 100usize;
        let options = ClientConnectionOptions::new("example.com", "/ws")
            .deflate(PerMessageDeflateConfig::new())
            .max_decompressed_size(max_decompressed);
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);

        conn.connect().unwrap();
        while conn.poll_output().is_some() {}

        let accept = compute_accept(&nonce);
        let response = create_valid_handshake_response(&accept, None, Some("permessage-deflate"));
        conn.feed_recv_buf(&response, now).unwrap();
        while conn.poll_event().is_some() {}
        while conn.poll_output().is_some() {}

        // max_decompressed を超えるデータを圧縮
        let mut server_deflate =
            shiguredo_websocket::PerMessageDeflate::new_server(PerMessageDeflateConfig::new());
        let large_data = "A".repeat(max_decompressed * repeat_count);
        let compressed = server_deflate.compress(large_data.as_bytes()).unwrap();

        let mut frame = vec![0xC1];
        if compressed.len() >= 126 {
            frame.push(126);
            frame.extend_from_slice(&(compressed.len() as u16).to_be_bytes());
        } else {
            frame.push(compressed.len() as u8);
        }
        frame.extend_from_slice(&compressed);

        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        prop_assert!(err_msg.contains("exceeds maximum limit"));
    }
}

// ==== 不正な入力に対する耐性テスト ====

proptest! {
    /// ランダムなバイト列は適切にエラーハンドリングされる
    #[test]
    fn prop_random_bytes_handling(
        random_data in prop::collection::vec(any::<u8>(), 0..1000),
    ) {
        let (mut conn, now, _) = setup_connected_client();

        // ランダムなデータを送信してもパニックしない
        let _ = conn.feed_recv_buf(&random_data, now);

        // 状態は一貫している
        let _ = conn.state();
    }

    /// ハンドシェイク中にランダムなレスポンスを送っても安全
    #[test]
    fn prop_random_bytes_during_handshake(
        random_data in prop::collection::vec(any::<u8>(), 1..500),
    ) {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        let now = Timestamp::from_millis(0);

        conn.connect().unwrap();
        while conn.poll_output().is_some() {}

        // ランダムなレスポンスを送信
        let _ = conn.feed_recv_buf(&random_data, now);

        // パニックしない
        let _ = conn.state();
    }
}

// ==== 無効な UTF-8 のテスト ====

proptest! {
    /// 無効な UTF-8 を含むテキストフレームはエラーを返す
    #[test]
    fn prop_invalid_utf8_text_frame(
        first_byte in prop::sample::select(vec![0x80u8, 0x81, 0xBF, 0xFE, 0xFF]),
        extra_bytes in prop::collection::vec(any::<u8>(), 0..10),
    ) {
        let (mut conn, now, _) = setup_connected_client();
        let mut invalid_utf8 = vec![first_byte];
        invalid_utf8.extend(extra_bytes);
        let frame = Frame::new(Opcode::Text, invalid_utf8).encode_unmasked();
        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_err(), "Invalid UTF-8 should return error");
    }
}

// ==== RFC 7692 window_bits 検証のテスト ====

proptest! {
    /// RFC 7692 Section 7.1.2.1: server_max_window_bits がクライアントの offer を超える場合は拒否
    #[test]
    fn prop_reject_server_max_window_bits_exceeding_offer(
        (client_bits, server_bits) in (8u8..=14u8).prop_flat_map(|c| (Just(c), (c + 1)..=15u8)),
    ) {

        let options = ClientConnectionOptions::new("example.com", "/ws")
            .deflate(PerMessageDeflateConfig::new().server_max_window_bits(client_bits));
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);

        conn.connect().unwrap();
        while conn.poll_output().is_some() {}

        let accept = compute_accept(&nonce);
        let ext = format!("permessage-deflate; server_max_window_bits={}", server_bits);
        let response = create_valid_handshake_response(&accept, None, Some(&ext));

        let result = conn.feed_recv_buf(&response, now);
        prop_assert!(result.is_err(), "Should reject server_max_window_bits {} > client offer {}", server_bits, client_bits);
        let err_msg = format!("{}", result.unwrap_err());
        prop_assert!(err_msg.contains("server_max_window_bits") && err_msg.contains("exceeds"));
    }

    /// RFC 7692 Section 7.1.2.1: server_max_window_bits がクライアントの offer 以下なら受理
    #[test]
    fn prop_accept_server_max_window_bits_within_offer(
        (client_bits, server_bits) in (8u8..=15u8).prop_flat_map(|c| (Just(c), 8u8..=c)),
    ) {

        let options = ClientConnectionOptions::new("example.com", "/ws")
            .deflate(PerMessageDeflateConfig::new().server_max_window_bits(client_bits));
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);

        conn.connect().unwrap();
        while conn.poll_output().is_some() {}

        let accept = compute_accept(&nonce);
        let ext = format!("permessage-deflate; server_max_window_bits={}", server_bits);
        let response = create_valid_handshake_response(&accept, None, Some(&ext));

        let result = conn.feed_recv_buf(&response, now);
        prop_assert!(result.is_ok(), "Should accept server_max_window_bits {} <= client offer {}", server_bits, client_bits);
    }

    /// RFC 7692 Section 7.1.2.2: client_max_window_bits がクライアントの offer を超える場合は拒否
    #[test]
    fn prop_reject_client_max_window_bits_exceeding_offer(
        (client_bits, server_bits) in (8u8..=14u8).prop_flat_map(|c| (Just(c), (c + 1)..=15u8)),
    ) {

        let options = ClientConnectionOptions::new("example.com", "/ws")
            .deflate(PerMessageDeflateConfig::new().client_max_window_bits(client_bits));
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);

        conn.connect().unwrap();
        while conn.poll_output().is_some() {}

        let accept = compute_accept(&nonce);
        let ext = format!("permessage-deflate; client_max_window_bits={}", server_bits);
        let response = create_valid_handshake_response(&accept, None, Some(&ext));

        let result = conn.feed_recv_buf(&response, now);
        prop_assert!(result.is_err(), "Should reject client_max_window_bits {} > client offer {}", server_bits, client_bits);
        let err_msg = format!("{}", result.unwrap_err());
        prop_assert!(err_msg.contains("client_max_window_bits") && err_msg.contains("exceeds"));
    }

    /// client_max_window_bits < 15 は拒否される (window_bits=15 固定の制約)
    #[test]
    fn prop_reject_client_max_window_bits_less_than_15(
        server_bits in 8u8..=14u8,
    ) {

        let options = ClientConnectionOptions::new("example.com", "/ws")
            .deflate(PerMessageDeflateConfig::new().client_max_window_bits(15));
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);

        conn.connect().unwrap();
        while conn.poll_output().is_some() {}

        let accept = compute_accept(&nonce);
        let ext = format!("permessage-deflate; client_max_window_bits={}", server_bits);
        let response = create_valid_handshake_response(&accept, None, Some(&ext));

        let result = conn.feed_recv_buf(&response, now);
        prop_assert!(result.is_err(), "Should reject client_max_window_bits={} (< 15)", server_bits);
        let err_msg = format!("{}", result.unwrap_err());
        prop_assert!(err_msg.contains("is not supported"));
    }
}

// ==== RFC 6455 Section 7.1.7: failed フラグのテスト ====

proptest! {
    /// process_frames() がエラーを返した後の feed_recv_buf() 再呼び出しで即座に Err が返る
    ///
    /// RFC 6455 Section 7.1.7: Fail the WebSocket Connection 後は
    /// データ処理を継続してはならない (MUST NOT)
    #[test]
    fn prop_failed_flag_prevents_reprocessing(
        payload in prop::collection::vec(any::<u8>(), 0..50),
    ) {
        let (mut conn, now, _) = setup_connected_client();

        // RSV2 ビットが立ったフレームでエラーを発生させる（サーバーからはマスクなし）
        let mut bad_frame = vec![0xA1, payload.len() as u8];
        bad_frame.extend_from_slice(&payload);

        // 最初の呼び出しはエラーになる
        let first = conn.feed_recv_buf(&bad_frame, now);
        prop_assert!(first.is_err());

        // 2 回目以降も即座にエラーになる (failed フラグ)
        let second = conn.feed_recv_buf(&[], now);
        prop_assert!(second.is_err());
    }
}
