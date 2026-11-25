//! websocket_client_connection.rs のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - 状態遷移の整合性
//! - ハンドシェイク処理の堅牢性
//! - フレーム処理の堅牢性
//! - タイマー処理の整合性

use base64ct::{Base64, Encoding};
use shiguredo_websocket::{
    ClientConnectionOptions, CloseCode, ConnectionEvent, ConnectionOutput, ConnectionState,
    ErrorKind, Frame, Opcode, PerMessageDeflateConfig, RandomSource, Timestamp,
    WebSocketClientConnection,
};

/// nonce から Sec-WebSocket-Accept を計算する
fn compute_accept(nonce: &[u8; 16]) -> String {
    let key = Base64::encode_string(nonce.as_slice());
    let combined = format!("{}258EAFA5-E914-47DA-95CA-C5AB0DC85B11", key);
    let hash = aws_lc_rs::digest::digest(
        &aws_lc_rs::digest::SHA1_FOR_LEGACY_USE_ONLY,
        combined.as_bytes(),
    );
    Base64::encode_string(hash.as_ref())
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

    conn.connect().expect("接続処理に失敗してはならない");

    // 出力を消費（ハンドシェイクリクエスト）
    while conn.poll_output().is_some() {}

    // Accept を計算してレスポンスを送信
    let accept = compute_accept(&nonce);
    let response = create_valid_handshake_response(&accept, None, None);
    conn.feed_recv_buf(&response, now)
        .expect("受信データの投入に失敗してはならない");

    // イベントと出力を消費
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    (conn, now, nonce)
}

// ==== 接続開始テスト ====

/// connect() で状態が Connecting に変わる
#[test]
fn prop_connect_changes_state_to_connecting() -> noprop::TestResult {
    pbt::run(|ctx| {
        let host = format!(
            "{}.{}",
            pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz", 1, 20),
            pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz", 2, 5),
        );
        let path = format!(
            "/{}",
            pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz0123456789/", 0, 30),
        );
        let options = ClientConnectionOptions::new(&host, &path);
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        conn.connect().expect("接続処理に失敗してはならない");
        assert_eq!(conn.state(), ConnectionState::Connecting);
        Ok(())
    })?;
    Ok(())
}

/// connect() でハンドシェイクリクエストが送信される
#[test]
fn prop_connect_sends_handshake_request() -> noprop::TestResult {
    pbt::run(|ctx| {
        let host = format!(
            "{}.{}",
            pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz", 1, 20),
            pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz", 2, 5),
        );
        let path = format!(
            "/{}",
            pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz0123456789/", 0, 30),
        );
        let options = ClientConnectionOptions::new(&host, &path);
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        conn.connect().expect("接続処理に失敗してはならない");
        let output = conn.poll_output().expect("出力が取得できるはず");
        match output {
            ConnectionOutput::SendData(data) => {
                let request = String::from_utf8_lossy(&data);
                let expected_path = format!("GET {} HTTP/1.1", path);
                let expected_host = format!("Host: {}", host);
                assert!(request.contains(expected_path.as_str()));
                assert!(request.contains(expected_host.as_str()));
                assert!(request.contains("Upgrade: websocket"));
                assert!(request.contains("Connection: Upgrade"));
                assert!(request.contains("Sec-WebSocket-Key:"));
                assert!(request.contains("Sec-WebSocket-Version: 13"));
            }
            _ => panic!("expected SendData"),
        }
        Ok(())
    })?;
    Ok(())
}

/// 二重接続はエラー
#[test]
fn prop_double_connect_fails() -> noprop::TestResult {
    pbt::run(|ctx| {
        let host = format!(
            "{}.{}",
            pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz", 1, 20),
            pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz", 2, 5),
        );
        let path = format!(
            "/{}",
            pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz0123456789/", 0, 30),
        );
        let options = ClientConnectionOptions::new(&host, &path);
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        conn.connect().expect("接続処理に失敗してはならない");
        let result = conn.connect();
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// ==== ハンドシェイク完了テスト ====

/// 有効なハンドシェイクで Connected 状態になる
#[test]
fn prop_valid_handshake_connects() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let random = FixedRandom::with_nonce(nonce);
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);
        conn.connect().expect("接続処理に失敗してはならない");
        while conn.poll_output().is_some() {}
        let accept = compute_accept(&nonce);
        let response = create_valid_handshake_response(&accept, None, None);
        conn.feed_recv_buf(&response, now)
            .expect("受信データの投入に失敗してはならない");
        while conn.poll_event().is_some() {}
        while conn.poll_output().is_some() {}
        assert_eq!(conn.state(), ConnectionState::Connected);
        Ok(())
    })?;
    Ok(())
}

/// プロトコル付きのハンドシェイクが正しく処理される
#[test]
fn prop_handshake_with_protocol() -> noprop::TestResult {
    pbt::run(|ctx| {
        let protocol = pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz", 1, 20);
        let options = ClientConnectionOptions::new("example.com", "/ws").protocol(&protocol);
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        let now = Timestamp::from_millis(0);

        conn.connect().expect("接続処理に失敗してはならない");

        // リクエストから nonce を取得
        let output = conn.poll_output().expect("出力が取得できるはず");
        let request_data = match output {
            ConnectionOutput::SendData(data) => data,
            _ => panic!("expected SendData"),
        };
        let request_str = String::from_utf8_lossy(&request_data);

        // プロトコルがリクエストに含まれることを確認
        let expected_protocol_header = format!("Sec-WebSocket-Protocol: {}", protocol);
        assert!(request_str.contains(&expected_protocol_header));

        let key_line = request_str
            .lines()
            .find(|l| l.starts_with("Sec-WebSocket-Key:"))
            .expect("Sec-WebSocket-Key ヘッダー行が存在するはず");
        let key = key_line
            .split(": ")
            .nth(1)
            .expect("要素が存在するはず")
            .trim();
        let nonce_vec = Base64::decode_vec(key).expect("base64 デコードに失敗してはならない");
        let mut nonce = [0u8; 16];
        nonce.copy_from_slice(&nonce_vec);

        let accept = compute_accept(&nonce);
        let response = create_valid_handshake_response(&accept, Some(&protocol), None);
        conn.feed_recv_buf(&response, now)
            .expect("受信データの投入に失敗してはならない");

        assert_eq!(conn.state(), ConnectionState::Connected);
        assert_eq!(conn.protocol(), Some(protocol.as_str()));
        Ok(())
    })?;
    Ok(())
}

/// 無効な Accept はエラー
#[test]
fn prop_invalid_accept_fails() -> noprop::TestResult {
    pbt::run(|ctx| {
        let mut invalid_accept = pbt::sample_alphabet(
            ctx,
            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+/=",
            20,
            40,
        );
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);
        conn.connect().expect("接続処理に失敗してはならない");
        while conn.poll_output().is_some() {}
        let valid_accept = compute_accept(&nonce);
        // 一致したら拒否しない。文字クラス [a-zA-Z0-9+/=] に含まれる
        // a / b で先頭 1 文字を入れ替え、必ず異なるようにする。
        if invalid_accept == valid_accept {
            let new_first = if invalid_accept.starts_with('a') {
                'b'
            } else {
                'a'
            };
            invalid_accept = format!("{new_first}{}", &invalid_accept[1..]);
        }
        let response = create_valid_handshake_response(&invalid_accept, None, None);
        let result = conn.feed_recv_buf(&response, now);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// サポートされていないプロトコルのハンドシェイク (RFC 6455 準拠)
#[test]
fn prop_handshake_unsupported_protocol() -> noprop::TestResult {
    pbt::run(|ctx| {
        let client_protocol = pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz", 3, 10);
        let mut server_protocol = pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz", 3, 10);
        // 異なるプロトコルの場合のみテスト
        // 一致したら拒否しない。サーバー側の先頭文字を別の a-z にする。
        // a なら b に、それ以外は a にする。
        if client_protocol == server_protocol {
            let new_first = if server_protocol.starts_with('a') {
                'b'
            } else {
                'a'
            };
            server_protocol = format!("{new_first}{}", &server_protocol[1..]);
        }

        let options = ClientConnectionOptions::new("example.com", "/ws").protocol(&client_protocol);
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);

        conn.connect().expect("接続処理に失敗してはならない");
        while conn.poll_output().is_some() {}

        // サーバーがクライアントの要求と異なるプロトコルを返す
        let accept = compute_accept(&nonce);
        let response = create_valid_handshake_response(&accept, Some(&server_protocol), None);
        let result = conn.feed_recv_buf(&response, now);

        // RFC 6455: クライアントが要求していないプロトコルは拒否
        assert!(
            result.is_err(),
            "Server returned unsolicited protocol should be rejected"
        );
        Ok(())
    })?;
    Ok(())
}

// ==== データ送受信テスト ====

/// テキストメッセージの送信
#[test]
fn prop_send_text_message() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_text(ctx, 0, 99);
        let (mut conn, _, _) = setup_connected_client();

        let result = conn.send_text(&text);
        assert!(result.is_ok());

        let output = conn.poll_output();
        assert!(matches!(output, Some(ConnectionOutput::SendData(_))));
        Ok(())
    })?;
    Ok(())
}

/// バイナリメッセージの送信
#[test]
fn prop_send_binary_message() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 1000);
        let (mut conn, _, _) = setup_connected_client();

        let result = conn.send_binary(&data);
        assert!(result.is_ok());

        let output = conn.poll_output();
        assert!(matches!(output, Some(ConnectionOutput::SendData(_))));
        Ok(())
    })?;
    Ok(())
}

/// テキストフレームの受信（サーバーからはマスクなし）
#[test]
fn prop_receive_text_frame() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_printable_ascii(ctx, 0, 100);
        let (mut conn, now, _) = setup_connected_client();

        // サーバーからのフレーム（マスクなし）
        let frame = Frame::text(&text).encode_unmasked();
        let result = conn.feed_recv_buf(&frame, now);
        assert!(result.is_ok());

        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(received) = event {
                assert_eq!(received, text);
                found = true;
                break;
            }
        }
        assert!(found, "TextMessage event not found");
        Ok(())
    })?;
    Ok(())
}

/// バイナリフレームの受信
#[test]
fn prop_receive_binary_frame() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 1000);
        let (mut conn, now, _) = setup_connected_client();

        let frame = Frame::binary(data.clone()).encode_unmasked();
        let result = conn.feed_recv_buf(&frame, now);
        assert!(result.is_ok());

        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::BinaryMessage(received) = event {
                assert_eq!(received, data);
                found = true;
                break;
            }
        }
        assert!(found, "BinaryMessage event not found");
        Ok(())
    })?;
    Ok(())
}

// ==== Ping/Pong のテスト ====

/// Ping を受信すると Pong を自動返信する
#[test]
fn prop_ping_auto_pong() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 125);
        let (mut conn, now, _) = setup_connected_client();

        let ping = Frame::ping(data.clone())
            .expect("Ping フレームの生成に失敗してはならない")
            .encode_unmasked();
        conn.feed_recv_buf(&ping, now)
            .expect("受信データの投入に失敗してはならない");

        // Ping イベントが発生
        let mut ping_found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Ping(received) = event {
                assert_eq!(received, data);
                ping_found = true;
                break;
            }
        }
        assert!(ping_found, "Ping event not found");

        // Pong が送信される
        let mut pong_found = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(_) = output {
                pong_found = true;
                break;
            }
        }
        assert!(pong_found, "Pong response not sent");
        Ok(())
    })?;
    Ok(())
}

/// Pong を受信すると awaiting_pong がリセットされる
#[test]
fn prop_pong_clears_awaiting() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 125);
        let (mut conn, now, _) = setup_connected_client();

        // まず Ping を送信
        conn.send_ping(&[])
            .expect("Ping の送信に失敗してはならない");
        while conn.poll_output().is_some() {}

        // Pong を受信
        let pong = Frame::pong(data.clone())
            .expect("Pong フレームの生成に失敗してはならない")
            .encode_unmasked();
        conn.feed_recv_buf(&pong, now)
            .expect("受信データの投入に失敗してはならない");

        // Pong イベントが発生
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Pong(received) = event {
                assert_eq!(received, data);
                found = true;
                break;
            }
        }
        assert!(found, "Pong event not found");
        Ok(())
    })?;
    Ok(())
}

// ==== Close のテスト ====

/// close() に渡す CloseCode の候補。
const CLOSE_CODES: [CloseCode; 3] = [
    CloseCode::NORMAL,
    CloseCode::GOING_AWAY,
    CloseCode::PROTOCOL_ERROR,
];

/// Close フレームを受信すると Close イベントが発生する
#[test]
fn prop_close_frame_received() -> noprop::TestResult {
    pbt::run(|ctx| {
        let code = pbt::sample_sendable_close_code(ctx);
        let reason = pbt::sample_printable_ascii(ctx, 0, 50);
        let (mut conn, now, _) = setup_connected_client();

        let close = Frame::close(Some(code), &reason)
            .expect("Close フレームの生成に失敗してはならない")
            .encode_unmasked();
        conn.feed_recv_buf(&close, now)
            .expect("受信データの投入に失敗してはならない");

        // Close イベントが発生
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Close {
                code: Some(c),
                reason: r,
            } = event
            {
                assert_eq!(c.as_u16(), code);
                assert_eq!(r, reason);
                found = true;
                break;
            }
        }
        assert!(found, "Close event not found");

        // 状態が Closed に変わる
        assert_eq!(conn.state(), ConnectionState::Closed);
        Ok(())
    })?;
    Ok(())
}

/// close() を呼び出すと Close フレームが送信される
#[test]
fn prop_close_sends_frame() -> noprop::TestResult {
    pbt::run(|ctx| {
        let code = noprop::sample_choice(ctx, &CLOSE_CODES);
        let reason = pbt::sample_printable_ascii(ctx, 0, 50);
        let (mut conn, _, _) = setup_connected_client();

        conn.close(code, &reason)
            .expect("クローズ処理に失敗してはならない");

        assert_eq!(conn.state(), ConnectionState::Closing);

        let mut found = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(_) = output {
                found = true;
                break;
            }
        }
        assert!(found, "Close frame not sent");
        Ok(())
    })?;
    Ok(())
}

/// Connecting 状態で close() を呼び出すとエラーになる
///
/// RFC 6455 Section 7.1.2: Close フレームは established connection 上でのみ送信可能
#[test]
fn prop_close_rejected_in_connecting_state() -> noprop::TestResult {
    pbt::run(|ctx| {
        let code = noprop::sample_choice(ctx, &CLOSE_CODES);
        let reason = pbt::sample_printable_ascii(ctx, 0, 50);
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let random = FixedRandom::new();
        let mut conn = WebSocketClientConnection::new(options, random);

        conn.connect().expect("接続処理に失敗してはならない");
        // ハンドシェイクレスポンスを送らず Connecting 状態のまま
        assert_eq!(conn.state(), ConnectionState::Connecting);

        // close() はエラーになるはず
        let result = conn.close(code, &reason);
        assert!(result.is_err(), "close() should fail in Connecting state");

        // フレームが出力されていないこと
        while conn.poll_output().is_some() {}
        assert_eq!(conn.state(), ConnectionState::Connecting);
        Ok(())
    })?;
    Ok(())
}

// ==== 状態遷移のテスト ====

/// Disconnected 状態では送信できない
#[test]
fn prop_cannot_send_while_disconnected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_text(ctx, 0, 99);
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        let result = conn.send_text(&text);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Connecting 状態では送信できない
#[test]
fn prop_cannot_send_while_connecting() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_text(ctx, 0, 99);
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        conn.connect().expect("接続処理に失敗してはならない");
        assert_eq!(conn.state(), ConnectionState::Connecting);
        let result = conn.send_text(&text);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Disconnected 状態ではデータを受信できない
#[test]
fn prop_feed_to_disconnected_fails() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 1, 100);
        let options = ClientConnectionOptions::new("example.com", "/ws");
        let mut conn = WebSocketClientConnection::new(options, FixedRandom::new());
        let now = Timestamp::from_millis(0);
        let result = conn.feed_recv_buf(&data, now);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Closed 状態ではデータを受信できない
#[test]
fn prop_feed_to_closed_fails() -> noprop::TestResult {
    pbt::run(|ctx| {
        let extra = pbt::sample_byte_vec(ctx, 1, 50);
        let (mut conn, now, _) = setup_connected_client();
        let close = Frame::close(Some(1000), "")
            .expect("Close フレームの生成に失敗してはならない")
            .encode_unmasked();
        conn.feed_recv_buf(&close, now)
            .expect("受信データの投入に失敗してはならない");
        assert_eq!(conn.state(), ConnectionState::Closed);
        let result = conn.feed_recv_buf(&extra, now);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// ==== RSV ビットのテスト ====

/// RSV2 ビットが立ったフレームは拒否される
#[test]
fn prop_rsv2_bit_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 0, 50);
        let (mut conn, now, _) = setup_connected_client();
        // RSV2 bit: 0xA1 = FIN + RSV2 + text opcode, サーバーからはマスクなし
        let mut frame = vec![0xA1, payload.len() as u8];
        frame.extend_from_slice(&payload);
        let result = conn.feed_recv_buf(&frame, now);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// RSV3 ビットが立ったフレームは拒否される
#[test]
fn prop_rsv3_bit_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 0, 50);
        let (mut conn, now, _) = setup_connected_client();
        // RSV3 bit: 0x91 = FIN + RSV3 + text opcode
        let mut frame = vec![0x91, payload.len() as u8];
        frame.extend_from_slice(&payload);
        let result = conn.feed_recv_buf(&frame, now);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// permessage-deflate なしで RSV1 ビットは拒否される
#[test]
fn prop_rsv1_without_deflate_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 0, 50);
        let (mut conn, now, _) = setup_connected_client();
        // RSV1 bit: 0xC1 = FIN + RSV1 + text opcode
        let mut frame = vec![0xC1, payload.len() as u8];
        frame.extend_from_slice(&payload);
        let result = conn.feed_recv_buf(&frame, now);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// ==== フラグメント処理のテスト ====

/// フラグメントされたテキストメッセージが正しく再構築される
#[test]
fn prop_fragmented_text_message() -> noprop::TestResult {
    pbt::run(|ctx| {
        let parts = pbt::sample_vec(ctx, 2, 5, |ctx| pbt::sample_printable_ascii(ctx, 1, 50));
        let (mut conn, now, _) = setup_connected_client();

        let full_text: String = parts.iter().cloned().collect();

        // 最初のフラグメント（opcode=Text, fin=false）
        let first_frame = {
            let mut frame = Frame::text(&parts[0]);
            frame.fin = false;
            frame.encode_unmasked()
        };
        conn.feed_recv_buf(&first_frame, now)
            .expect("受信データの投入に失敗してはならない");

        // 中間のフラグメント
        for part in parts.iter().take(parts.len() - 1).skip(1) {
            let mut frame = Frame::new(Opcode::Continuation, part.as_bytes().to_vec());
            frame.fin = false;
            let encoded = frame.encode_unmasked();
            conn.feed_recv_buf(&encoded, now)
                .expect("受信データの投入に失敗してはならない");
        }

        // 最後のフラグメント
        let last_idx = parts.len() - 1;
        let mut frame = Frame::new(Opcode::Continuation, parts[last_idx].as_bytes().to_vec());
        frame.fin = true;
        let encoded = frame.encode_unmasked();
        conn.feed_recv_buf(&encoded, now)
            .expect("受信データの投入に失敗してはならない");

        // 完全なメッセージが受信される
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(received) = event {
                assert_eq!(received, full_text);
                found = true;
                break;
            }
        }
        assert!(found, "Complete message not received");
        Ok(())
    })?;
    Ok(())
}

/// 開始フレームなしで Continuation は失敗
#[test]
fn prop_continuation_without_start_fails() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 100);
        let (mut conn, now, _) = setup_connected_client();
        let cont = Frame::new(Opcode::Continuation, data).encode_unmasked();
        let result = conn.feed_recv_buf(&cont, now);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// RFC 6455 Section 5.4: フラグメント中に新しいデータフレームは禁止
#[test]
fn prop_new_message_during_fragment_fails() -> noprop::TestResult {
    pbt::run(|ctx| {
        let first_part = pbt::sample_printable_ascii(ctx, 1, 50);
        let new_message = pbt::sample_printable_ascii(ctx, 1, 50);
        let (mut conn, now, _) = setup_connected_client();
        let mut frame1 = Frame::text(&first_part);
        frame1.fin = false;
        conn.feed_recv_buf(&frame1.encode_unmasked(), now)
            .expect("エンコードに失敗してはならない");
        let frame2 = Frame::text(&new_message).encode_unmasked();
        let result = conn.feed_recv_buf(&frame2, now);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// ==== 部分的なデータ受信のテスト ====

/// データを小さなチャンクで分割して送っても正しく処理される
#[test]
fn prop_chunked_frame_reception() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_printable_ascii(ctx, 10, 100);
        let chunk_size = pbt::sample_len(ctx, 1, 10);
        let (mut conn, now, _) = setup_connected_client();

        let frame = Frame::text(&text).encode_unmasked();

        // チャンクに分割して送信
        for chunk in frame.chunks(chunk_size) {
            conn.feed_recv_buf(chunk, now)
                .expect("受信データの投入に失敗してはならない");
        }

        // メッセージが正しく受信される
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(received) = event {
                assert_eq!(received, text);
                found = true;
                break;
            }
        }
        assert!(found, "Message not received after chunked delivery");
        Ok(())
    })?;
    Ok(())
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

    conn.connect().expect("接続処理に失敗してはならない");
    while conn.poll_output().is_some() {}

    let accept = compute_accept(&nonce);
    let response = create_valid_handshake_response(&accept, None, Some("permessage-deflate"));
    conn.feed_recv_buf(&response, now)
        .expect("受信データの投入に失敗してはならない");

    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    (conn, now, nonce)
}

/// 圧縮テキスト送信（RSV1 フラグとペイロード検証）
#[test]
fn prop_deflate_send_compressed_text() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_printable_ascii(ctx, 65, 200);
        let (mut conn, _, _) = setup_connected_client_with_deflate();

        conn.send_text(&text)
            .expect("テキストメッセージの送信に失敗してはならない");

        let output = conn.poll_output().expect("出力が取得できるはず");
        match output {
            ConnectionOutput::SendData(data) => {
                // RSV1 が設定されていること（圧縮フレーム）
                assert_eq!(
                    data[0] & 0x40,
                    0x40,
                    "RSV1 should be set for compressed messages"
                );
                assert_eq!(data[0] & 0x0F, 0x01, "opcode should be text");
            }
            _ => panic!("expected SendData"),
        }
        Ok(())
    })?;
    Ok(())
}

/// 圧縮テキスト受信（解凍検証）
#[test]
fn prop_deflate_receive_compressed_text() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_printable_ascii(ctx, 1, 200);
        let (mut conn, now, _) = setup_connected_client_with_deflate();

        // サーバー側コーデックでテキストを圧縮
        let mut server_deflate =
            shiguredo_websocket::PerMessageDeflate::new_server(PerMessageDeflateConfig::new());
        let compressed = server_deflate
            .compress(text.as_bytes())
            .expect("圧縮に失敗してはならない");

        // 圧縮フレームを構築: FIN=1, RSV1=1, opcode=text(1)
        let mut frame = vec![0xC1]; // FIN + RSV1 + text
        if compressed.len() >= 126 {
            frame.push(126);
            frame.extend_from_slice(&(compressed.len() as u16).to_be_bytes());
        } else {
            frame.push(compressed.len() as u8);
        }
        frame.extend_from_slice(&compressed);

        conn.feed_recv_buf(&frame, now)
            .expect("受信データの投入に失敗してはならない");

        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(received) = event {
                assert_eq!(received, text);
                found = true;
                break;
            }
        }
        assert!(found, "TextMessage event not found");
        Ok(())
    })?;
    Ok(())
}

/// 小メッセージの非圧縮（しきい値以下）
#[test]
fn prop_deflate_small_message_not_compressed() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_printable_ascii(ctx, 1, 10);
        let (mut conn, _, _) = setup_connected_client_with_deflate();

        conn.send_text(&text)
            .expect("テキストメッセージの送信に失敗してはならない");

        let output = conn.poll_output().expect("出力が取得できるはず");
        match output {
            ConnectionOutput::SendData(data) => {
                // RSV1 が設定されていないこと（非圧縮）
                assert_eq!(
                    data[0] & 0x40,
                    0x00,
                    "RSV1 should not be set for small messages"
                );
            }
            _ => panic!("expected SendData"),
        }
        Ok(())
    })?;
    Ok(())
}

/// Zip Bomb 保護（最大解凍サイズ超過）
#[test]
fn prop_deflate_zip_bomb_protection() -> noprop::TestResult {
    pbt::run(|ctx| {
        let repeat_count = pbt::sample_len(ctx, 3, 10);
        let max_decompressed = 100usize;
        let options = ClientConnectionOptions::new("example.com", "/ws")
            .deflate(PerMessageDeflateConfig::new())
            .max_decompressed_size(max_decompressed);
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);

        conn.connect().expect("接続処理に失敗してはならない");
        while conn.poll_output().is_some() {}

        let accept = compute_accept(&nonce);
        let response = create_valid_handshake_response(&accept, None, Some("permessage-deflate"));
        conn.feed_recv_buf(&response, now)
            .expect("受信データの投入に失敗してはならない");
        while conn.poll_event().is_some() {}
        while conn.poll_output().is_some() {}

        // max_decompressed を超えるデータを圧縮
        let mut server_deflate =
            shiguredo_websocket::PerMessageDeflate::new_server(PerMessageDeflateConfig::new());
        let large_data = "A".repeat(max_decompressed * repeat_count);
        let compressed = server_deflate
            .compress(large_data.as_bytes())
            .expect("圧縮に失敗してはならない");

        let mut frame = vec![0xC1];
        if compressed.len() >= 126 {
            frame.push(126);
            frame.extend_from_slice(&(compressed.len() as u16).to_be_bytes());
        } else {
            frame.push(compressed.len() as u8);
        }
        frame.extend_from_slice(&compressed);

        let result = conn.feed_recv_buf(&frame, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("exceeds maximum limit"));
        Ok(())
    })?;
    Ok(())
}

// ==== 無効な UTF-8 のテスト ====

/// UTF-8 として不正になり得る先頭バイト。
const INVALID_UTF8_FIRST_BYTES: [u8; 5] = [0x80, 0x81, 0xBF, 0xFE, 0xFF];

/// 無効な UTF-8 を含むテキストフレームはエラーを返す
#[test]
fn prop_invalid_utf8_text_frame() -> noprop::TestResult {
    pbt::run(|ctx| {
        let first_byte = noprop::sample_choice(ctx, &INVALID_UTF8_FIRST_BYTES);
        let extra_bytes = pbt::sample_byte_vec(ctx, 0, 10);
        let (mut conn, now, _) = setup_connected_client();
        let mut invalid_utf8 = vec![first_byte];
        invalid_utf8.extend(extra_bytes);
        let frame = Frame::new(Opcode::Text, invalid_utf8).encode_unmasked();
        let result = conn.feed_recv_buf(&frame, now);
        assert!(result.is_err(), "Invalid UTF-8 should return error");
        Ok(())
    })?;
    Ok(())
}

// ==== RFC 7692 window_bits 検証のテスト ====

/// RFC 7692 Section 7.1.2.1: server_max_window_bits がクライアントの offer を超える場合は拒否
#[test]
fn prop_reject_server_max_window_bits_exceeding_offer() -> noprop::TestResult {
    pbt::run(|ctx| {
        // server の window bits は client より必ず大きい。
        let client_bits = noprop::sample_u8_in(ctx, 8..=14);
        let server_bits = noprop::sample_u8_in(ctx, (client_bits + 1)..=15);

        let options = ClientConnectionOptions::new("example.com", "/ws")
            .deflate(PerMessageDeflateConfig::new().server_max_window_bits(client_bits));
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);

        conn.connect().expect("接続処理に失敗してはならない");
        while conn.poll_output().is_some() {}

        let accept = compute_accept(&nonce);
        let ext = format!("permessage-deflate; server_max_window_bits={}", server_bits);
        let response = create_valid_handshake_response(&accept, None, Some(&ext));

        let result = conn.feed_recv_buf(&response, now);
        assert!(
            result.is_err(),
            "Should reject server_max_window_bits {} > client offer {}",
            server_bits,
            client_bits
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("server_max_window_bits") && err_msg.contains("exceeds"));
        Ok(())
    })?;
    Ok(())
}

/// RFC 7692 Section 7.1.2.1: server_max_window_bits がクライアントの offer 以下なら受理
#[test]
fn prop_accept_server_max_window_bits_within_offer() -> noprop::TestResult {
    pbt::run(|ctx| {
        // server の window bits は client 以下。
        let client_bits = noprop::sample_u8_in(ctx, 8..=15);
        let server_bits = noprop::sample_u8_in(ctx, 8..=client_bits);

        let options = ClientConnectionOptions::new("example.com", "/ws")
            .deflate(PerMessageDeflateConfig::new().server_max_window_bits(client_bits));
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);

        conn.connect().expect("接続処理に失敗してはならない");
        while conn.poll_output().is_some() {}

        let accept = compute_accept(&nonce);
        let ext = format!("permessage-deflate; server_max_window_bits={}", server_bits);
        let response = create_valid_handshake_response(&accept, None, Some(&ext));

        let result = conn.feed_recv_buf(&response, now);
        assert!(
            result.is_ok(),
            "Should accept server_max_window_bits {} <= client offer {}",
            server_bits,
            client_bits
        );
        Ok(())
    })?;
    Ok(())
}

/// RFC 7692 Section 7.1.2.2: client_max_window_bits がクライアントの offer を超える場合は拒否
#[test]
fn prop_reject_client_max_window_bits_exceeding_offer() -> noprop::TestResult {
    pbt::run(|ctx| {
        // server の window bits は client より必ず大きい。
        let client_bits = noprop::sample_u8_in(ctx, 8..=14);
        let server_bits = noprop::sample_u8_in(ctx, (client_bits + 1)..=15);

        let options = ClientConnectionOptions::new("example.com", "/ws")
            .deflate(PerMessageDeflateConfig::new().client_max_window_bits(client_bits));
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);

        conn.connect().expect("接続処理に失敗してはならない");
        while conn.poll_output().is_some() {}

        let accept = compute_accept(&nonce);
        let ext = format!("permessage-deflate; client_max_window_bits={}", server_bits);
        let response = create_valid_handshake_response(&accept, None, Some(&ext));

        let result = conn.feed_recv_buf(&response, now);
        assert!(
            result.is_err(),
            "Should reject client_max_window_bits {} > client offer {}",
            server_bits,
            client_bits
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("client_max_window_bits") && err_msg.contains("exceeds"));
        Ok(())
    })?;
    Ok(())
}

/// client_max_window_bits < 15 は拒否される (window_bits=15 固定の制約)
#[test]
fn prop_reject_client_max_window_bits_less_than_15() -> noprop::TestResult {
    pbt::run(|ctx| {
        let server_bits = noprop::sample_u8_in(ctx, 8..=14);

        let options = ClientConnectionOptions::new("example.com", "/ws")
            .deflate(PerMessageDeflateConfig::new().client_max_window_bits(15));
        let random = FixedRandom::new();
        let nonce = random.nonce;
        let mut conn = WebSocketClientConnection::new(options, random);
        let now = Timestamp::from_millis(0);

        conn.connect().expect("接続処理に失敗してはならない");
        while conn.poll_output().is_some() {}

        let accept = compute_accept(&nonce);
        let ext = format!("permessage-deflate; client_max_window_bits={}", server_bits);
        let response = create_valid_handshake_response(&accept, None, Some(&ext));

        let result = conn.feed_recv_buf(&response, now);
        assert!(
            result.is_err(),
            "Should reject client_max_window_bits={} (< 15)",
            server_bits
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("is not supported"));
        Ok(())
    })?;
    Ok(())
}

// ==== RFC 6455 Section 7.1.7: failed フラグのテスト ====

/// process_frames() がエラーを返した後の feed_recv_buf() 再呼び出しで即座に Err が返る
///
/// RFC 6455 Section 7.1.7: Fail the WebSocket Connection 後は
/// データ処理を継続してはならない (MUST NOT)
#[test]
fn prop_failed_flag_prevents_reprocessing() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 0, 50);
        let (mut conn, now, _) = setup_connected_client();

        // RSV2 ビットが立ったフレームでエラーを発生させる（サーバーからはマスクなし）
        let mut bad_frame = vec![0xA1, payload.len() as u8];
        bad_frame.extend_from_slice(&payload);

        // 最初の呼び出しはエラーになる
        let first = conn.feed_recv_buf(&bad_frame, now);
        assert!(first.is_err());

        // 2 回目以降も即座にエラーになる (failed フラグ)
        let second = conn.feed_recv_buf(&[], now);
        assert!(second.is_err());
        Ok(())
    })?;
    Ok(())
}

// ==== max_message_size 上限に対する大量フラグメント攻撃シミュレーション ====

/// `max_message_size` を指定したクライアント接続を Connected 状態まで進めるヘルパ
fn setup_connected_client_with_max_message_size(
    max_message_size: usize,
) -> (WebSocketClientConnection<FixedRandom>, Timestamp) {
    let options =
        ClientConnectionOptions::new("example.com", "/ws").max_message_size(max_message_size);
    let random = FixedRandom::new();
    let nonce = random.nonce;
    let mut conn = WebSocketClientConnection::new(options, random);
    let now = Timestamp::from_millis(0);

    conn.connect().expect("接続処理に失敗してはならない");
    while conn.poll_output().is_some() {}

    let accept = compute_accept(&nonce);
    let response = create_valid_handshake_response(&accept, None, None);
    conn.feed_recv_buf(&response, now)
        .expect("ハンドシェイク応答の投入に失敗してはならない");
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    (conn, now)
}

/// マスクなし Text フレーム (FIN=fin) を構築する
fn build_unmasked_text_fragment(payload: &[u8], fin: bool) -> Vec<u8> {
    let mut frame = vec![
        if fin { 0x81 } else { 0x01 }, // FIN + Text opcode
        payload.len() as u8,           // MASK=0 + length
    ];
    frame.extend_from_slice(payload);
    frame
}

/// マスクなし Continuation フレーム (FIN=fin) を構築する
fn build_unmasked_continuation_fragment(payload: &[u8], fin: bool) -> Vec<u8> {
    let mut frame = vec![
        if fin { 0x80 } else { 0x00 }, // FIN + Continuation opcode (0)
        payload.len() as u8,           // MASK=0 + length
    ];
    frame.extend_from_slice(payload);
    frame
}

/// `max_message_size = 100` のクライアント接続に対し、10 バイトずつの Text + Continuation
/// フラグメントを連続で送ると、累積サイズが上限を超えた時点で
/// `Err(ErrorKind::ProtocolViolation)` となり、MESSAGE_TOO_BIG (1009) で接続が閉じられる
#[test]
fn prop_fragment_flood_exceeds_max_message_size() -> noprop::TestResult {
    pbt::run(|ctx| {
        let num_fragments = pbt::sample_len_inclusive(ctx, 15, 30);
        const MAX_MSG: usize = 100;
        const FRAG_PAYLOAD: &[u8] = b"0123456789";

        let (mut conn, now) = setup_connected_client_with_max_message_size(MAX_MSG);

        // 最初の Text フラグメント (FIN=0)
        let first = build_unmasked_text_fragment(FRAG_PAYLOAD, false);
        conn.feed_recv_buf(&first, now)
            .expect("最初のフラグメントは受理されるべき");

        // Continuation フラグメントを送り、上限超過まで feed
        let mut overflowed = false;
        for _ in 0..num_fragments {
            let cont = build_unmasked_continuation_fragment(FRAG_PAYLOAD, false);
            let result = conn.feed_recv_buf(&cont, now);
            if let Err(e) = result {
                assert_eq!(e.kind, ErrorKind::ProtocolViolation);
                overflowed = true;
                break;
            }
        }
        assert!(
            overflowed,
            "num_fragments={} で max_message_size={} を超えても拒否されなかった",
            num_fragments, MAX_MSG
        );

        // 接続が close 状態 (close_internal が呼ばれている) になっており、
        // MESSAGE_TOO_BIG (1009) の Close フレームが送出されている
        let mut found_close = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(data) = output {
                let mut decoder = shiguredo_websocket::FrameDecoder::new();
                decoder.feed(&data);
                if let Ok(Some(decoded)) = decoder.decode_with_info()
                    && decoded.frame.opcode == Opcode::Close
                    && decoded.frame.payload.len() >= 2
                {
                    let code =
                        u16::from_be_bytes([decoded.frame.payload[0], decoded.frame.payload[1]]);
                    assert_eq!(code, CloseCode::MESSAGE_TOO_BIG.as_u16());
                    found_close = true;
                    break;
                }
            }
        }
        assert!(
            found_close,
            "MESSAGE_TOO_BIG (1009) Close フレームが送出されていない"
        );
        Ok(())
    })?;
    Ok(())
}
