//! websocket_client_connection.rs のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - 状態遷移の整合性
//! - ハンドシェイク処理の堅牢性
//! - フレーム処理の堅牢性
//! - タイマー処理の整合性

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use proptest::prelude::*;
use sha1::{Digest, Sha1};
use shiguredo_websocket::{
    ClientConnectionOptions, CloseCode, ConnectionEvent, ConnectionOutput, ConnectionState, Frame,
    Opcode, PerMessageDeflateConfig, TimerId, Timestamp, WebSocketClientConnection,
};

/// nonce から Sec-WebSocket-Accept を計算する
fn compute_accept(nonce: &[u8; 16]) -> String {
    let key = STANDARD.encode(nonce);
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let hash = hasher.finalize();
    STANDARD.encode(hash)
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

/// 接続を作成して Connected 状態まで進める
fn setup_connected_client() -> (WebSocketClientConnection, Timestamp, [u8; 16]) {
    let options = ClientConnectionOptions::new("example.com", "/ws");
    let mut conn = WebSocketClientConnection::new(options);
    let now = Timestamp::from_millis(0);

    conn.connect(now).unwrap();

    // nonce を取得するために、出力からハンドシェイクリクエストを解析
    let output = conn.poll_output().unwrap();
    let request_data = match output {
        ConnectionOutput::SendData(data) => data,
        _ => panic!("expected SendData"),
    };

    // リクエストから Sec-WebSocket-Key を抽出
    let request_str = String::from_utf8_lossy(&request_data);
    let key_line = request_str
        .lines()
        .find(|l| l.starts_with("Sec-WebSocket-Key:"))
        .unwrap();
    let key = key_line.split(": ").nth(1).unwrap().trim();

    // キーをデコードして nonce を取得
    let nonce_vec = STANDARD.decode(key).unwrap();
    let mut nonce = [0u8; 16];
    nonce.copy_from_slice(&nonce_vec);

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
    /// ClientConnectionOptions::default のデフォルト値
    #[test]
    fn prop_client_options_default(_dummy in Just(())) {
        let options = ClientConnectionOptions::default();
        prop_assert_eq!(options.path, "/");
        prop_assert_eq!(options.host, "localhost");
        prop_assert!(options.origin.is_none());
        prop_assert!(options.protocols.is_empty());
        prop_assert!(options.deflate_config.is_none());
        prop_assert!(options.additional_headers.is_empty());
        prop_assert_eq!(options.ping_interval_millis, 30_000);
        prop_assert_eq!(options.pong_timeout_millis, 10_000);
        prop_assert_eq!(options.close_timeout_millis, 5_000);
    }

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

// ==== 初期状態のテスト ====

proptest! {
    /// 新規接続の初期状態は Disconnected
    #[test]
    fn prop_initial_state_is_disconnected(_dummy in Just(())) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let conn = WebSocketClientConnection::new(options);
        prop_assert_eq!(conn.state(), ConnectionState::Disconnected);
    }

    /// 新規接続のプロトコルは None
    #[test]
    fn prop_initial_protocol_is_none(_dummy in Just(())) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let conn = WebSocketClientConnection::new(options);
        prop_assert!(conn.protocol().is_none());
    }

    /// 新規接続の拡張は空
    #[test]
    fn prop_initial_extensions_is_empty(_dummy in Just(())) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let conn = WebSocketClientConnection::new(options);
        prop_assert!(conn.extensions().is_empty());
    }
}

// ==== 接続開始テスト ====

proptest! {
    /// connect() で状態が Connecting に変わる
    #[test]
    fn prop_connect_changes_state_to_connecting(_dummy in Just(())) {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        conn.connect(now).unwrap();
        prop_assert_eq!(conn.state(), ConnectionState::Connecting);
    }

    /// connect() でハンドシェイクリクエストが送信される
    #[test]
    fn prop_connect_sends_handshake_request(_dummy in Just(())) {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        conn.connect(now).unwrap();

        let output = conn.poll_output().unwrap();
        match output {
            ConnectionOutput::SendData(data) => {
                let request = String::from_utf8_lossy(&data);
                prop_assert!(request.contains("GET /ws HTTP/1.1"));
                prop_assert!(request.contains("Host: example.com"));
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
    fn prop_double_connect_fails(_dummy in Just(())) {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        conn.connect(now).unwrap();
        let result = conn.connect(now);
        prop_assert!(result.is_err());
    }
}

// ==== ハンドシェイク完了テスト ====

proptest! {
    /// 有効なハンドシェイクで Connected 状態になる
    #[test]
    fn prop_valid_handshake_connects(_dummy in Just(())) {
        let (conn, _, _) = setup_connected_client();
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
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        conn.connect(now).unwrap();

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
        let nonce_vec = STANDARD.decode(key).unwrap();
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
    fn prop_invalid_accept_fails(_dummy in Just(())) {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        conn.connect(now).unwrap();
        while conn.poll_output().is_some() {}

        // 無効な Accept
        let response = create_valid_handshake_response("invalid_accept", None, None);
        let result = conn.feed_recv_buf(&response, now);

        prop_assert!(result.is_err());
    }
}

// ==== データ送受信テスト ====

proptest! {
    /// テキストメッセージの送信
    #[test]
    fn prop_send_text_message(
        text in ".*",
    ) {
        let (mut conn, now, _) = setup_connected_client();

        let result = conn.send_text(&text, now);
        prop_assert!(result.is_ok());

        let output = conn.poll_output();
        prop_assert!(matches!(output, Some(ConnectionOutput::SendData(_))));
    }

    /// バイナリメッセージの送信
    #[test]
    fn prop_send_binary_message(
        data in prop::collection::vec(any::<u8>(), 0..1000),
    ) {
        let (mut conn, now, _) = setup_connected_client();

        let result = conn.send_binary(&data, now);
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

        let ping = Frame::ping(data.clone()).encode_unmasked();
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
        conn.send_ping(&[], now).unwrap();
        while conn.poll_output().is_some() {}

        // Pong を受信
        let pong = Frame::pong(data.clone()).encode_unmasked();
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

        let close = Frame::close(Some(code), &reason).encode_unmasked();
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
        let (mut conn, now, _) = setup_connected_client();

        conn.close(code, &reason, now).unwrap();

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

    /// コードなしの Close フレーム
    #[test]
    fn prop_close_without_code(_dummy in Just(())) {
        let (mut conn, now, _) = setup_connected_client();

        // コードなしの Close フレーム
        let close = Frame::close(None, "").encode_unmasked();
        conn.feed_recv_buf(&close, now).unwrap();

        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Close { code: None, reason } = event {
                prop_assert!(reason.is_empty());
                found = true;
                break;
            }
        }
        prop_assert!(found, "Close event not found");
    }
}

// ==== タイマー処理のテスト ====

proptest! {
    /// Ping タイマーで Ping が送信される
    #[test]
    fn prop_ping_timer_event(_dummy in Just(())) {
        let (mut conn, now, _) = setup_connected_client();

        conn.handle_timer(TimerId::Ping, now).unwrap();

        // Ping が送信されるはず
        let mut found = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(_) = output {
                found = true;
                break;
            }
        }
        prop_assert!(found, "Ping not sent on timer");
    }

    /// Pong タイムアウトで接続が閉じられる
    #[test]
    fn prop_pong_timeout_closes_connection(_dummy in Just(())) {
        let (mut conn, now, _) = setup_connected_client();

        // Ping を送信
        conn.send_ping(&[], now).unwrap();
        while conn.poll_output().is_some() {}
        while conn.poll_event().is_some() {}

        // Pong タイムアウトをトリガー
        conn.handle_timer(TimerId::PongTimeout, now).unwrap();

        // エラーイベントが発生
        let mut error_found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Error(msg) = event {
                prop_assert!(msg.contains("pong timeout"));
                error_found = true;
                break;
            }
        }
        prop_assert!(error_found, "Error event not found");

        // 状態が Closing に変わる
        prop_assert_eq!(conn.state(), ConnectionState::Closing);
    }

    /// Close タイムアウトで強制切断
    #[test]
    fn prop_close_timeout_forces_disconnect(_dummy in Just(())) {
        let (mut conn, now, _) = setup_connected_client();

        // Close を送信
        conn.close(CloseCode::NORMAL, "", now).unwrap();
        while conn.poll_output().is_some() {}
        while conn.poll_event().is_some() {}

        prop_assert_eq!(conn.state(), ConnectionState::Closing);

        // Close タイムアウトをトリガー
        conn.handle_timer(TimerId::CloseTimeout, now).unwrap();

        prop_assert_eq!(conn.state(), ConnectionState::Closed);
    }
}

// ==== 状態遷移のテスト ====

proptest! {
    /// Disconnected 状態では送信できない
    #[test]
    fn prop_cannot_send_while_disconnected(_dummy in Just(())) {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        let result = conn.send_text("hello", now);
        prop_assert!(result.is_err());
    }

    /// Connecting 状態では送信できない
    #[test]
    fn prop_cannot_send_while_connecting(_dummy in Just(())) {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        conn.connect(now).unwrap();
        prop_assert_eq!(conn.state(), ConnectionState::Connecting);

        let result = conn.send_text("hello", now);
        prop_assert!(result.is_err());
    }
}

proptest! {
    /// Disconnected 状態ではデータを受信できない
    #[test]
    fn prop_feed_to_disconnected_fails(_dummy in Just(())) {
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        let result = conn.feed_recv_buf(b"data", now);
        prop_assert!(result.is_err());
    }

    /// Closed 状態ではデータを受信できない
    #[test]
    fn prop_feed_to_closed_fails(_dummy in Just(())) {
        let (mut conn, now, _) = setup_connected_client();

        // Close フレームを送受信
        let close = Frame::close(Some(1000), "").encode_unmasked();
        conn.feed_recv_buf(&close, now).unwrap();

        prop_assert_eq!(conn.state(), ConnectionState::Closed);

        // Closed 状態でデータを送ろうとするとエラー
        let text_frame = Frame::text("hello").encode_unmasked();
        let result = conn.feed_recv_buf(&text_frame, now);
        prop_assert!(result.is_err());
    }
}

// ==== RSV ビットのテスト ====

proptest! {
    /// RSV2 ビットが立ったフレームは拒否される
    #[test]
    fn prop_rsv2_bit_rejected(_dummy in Just(())) {
        let (mut conn, now, _) = setup_connected_client();

        // RSV2 ビットが立ったフレーム
        let frame = vec![0xA1, 0x05, b'H', b'e', b'l', b'l', b'o'];
        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_err());
    }

    /// RSV3 ビットが立ったフレームは拒否される
    #[test]
    fn prop_rsv3_bit_rejected(_dummy in Just(())) {
        let (mut conn, now, _) = setup_connected_client();

        // RSV3 ビットが立ったフレーム
        let frame = vec![0x91, 0x05, b'H', b'e', b'l', b'l', b'o'];
        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_err());
    }

    /// permessage-deflate なしで RSV1 ビットは拒否される
    #[test]
    fn prop_rsv1_without_deflate_rejected(_dummy in Just(())) {
        let (mut conn, now, _) = setup_connected_client();

        // RSV1 ビットが立ったフレーム（permessage-deflate なし）
        let frame = vec![0xC1, 0x05, b'H', b'e', b'l', b'l', b'o'];
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
        for i in 1..parts.len()-1 {
            let mut frame = Frame::new(Opcode::Continuation, parts[i].as_bytes().to_vec());
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
    fn prop_continuation_without_start_fails(_dummy in Just(())) {
        let (mut conn, now, _) = setup_connected_client();

        // 開始フレームなしで Continuation を送る
        let cont = Frame::new(Opcode::Continuation, b"data".to_vec()).encode_unmasked();
        let result = conn.feed_recv_buf(&cont, now);
        prop_assert!(result.is_err());
    }

    /// RFC 6455 Section 5.4: フラグメント中に新しいデータフレームは禁止
    #[test]
    fn prop_new_message_during_fragment_fails(_dummy in Just(())) {
        let (mut conn, now, _) = setup_connected_client();

        // フラグメント開始（fin=false）
        let mut frame1 = Frame::text("part1");
        frame1.fin = false;
        conn.feed_recv_buf(&frame1.encode_unmasked(), now).unwrap();

        // 完了前に新しいメッセージ開始
        let frame2 = Frame::text("new message").encode_unmasked();
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

// ==== deflate 設定のテスト ====

proptest! {
    /// deflate オプションの設定
    #[test]
    fn prop_deflate_option_setting(_dummy in Just(())) {
        let config = PerMessageDeflateConfig::default();
        let options = ClientConnectionOptions::new("example.com", "/").deflate(config.clone());

        prop_assert!(options.deflate_config.is_some());
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
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        conn.connect(now).unwrap();
        while conn.poll_output().is_some() {}

        // ランダムなレスポンスを送信
        let _ = conn.feed_recv_buf(&random_data, now);

        // パニックしない
        let _ = conn.state();
    }
}

// ==== イベント・出力キューのテスト ====

proptest! {
    /// イベントキューはすべて消費できる
    #[test]
    fn prop_event_queue_drains(_dummy in Just(())) {
        let (mut conn, _, _) = setup_connected_client();

        // イベントをすべて消費
        while conn.poll_event().is_some() {}

        // もうイベントがない
        prop_assert!(conn.poll_event().is_none());
    }

    /// 出力キューはすべて消費できる
    #[test]
    fn prop_output_queue_drains(_dummy in Just(())) {
        let (mut conn, _, _) = setup_connected_client();

        // 出力をすべて消費
        while conn.poll_output().is_some() {}

        // もう出力がない
        prop_assert!(conn.poll_output().is_none());
    }
}

// ==== 無効な UTF-8 のテスト ====

proptest! {
    /// 無効な UTF-8 を含むテキストフレームはエラーを返す
    #[test]
    fn prop_invalid_utf8_text_frame(_dummy in Just(())) {
        let (mut conn, now, _) = setup_connected_client();

        // 無効な UTF-8 を含むテキストフレーム
        let invalid_utf8 = vec![0xFF, 0xFE, 0x00, 0x01];
        let frame = Frame::new(Opcode::Text, invalid_utf8).encode_unmasked();

        // RFC 6455 準拠: 無効な UTF-8 は即座にエラーを返す
        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_err(), "Invalid UTF-8 should return error");
        let err = result.unwrap_err();
        let err_msg = format!("{}", err);
        prop_assert!(err_msg.contains("UTF-8") || err_msg.contains("utf-8"));
    }
}
