//! websocket_connection_shared.rs の単体テスト
//!
//! SharedConnectionState は pub(crate) のため、
//! WebSocketClientConnection / WebSocketServerConnection の公開 API 経由でテストする。
//! PBT では実現しにくいエラーパス・境界値の検証を行う。

use base64ct::{Base64, Encoding};
use shiguredo_websocket::{
    ClientConnectionOptions, ConnectionEvent, ConnectionOutput, Frame, FrameDecoder, Opcode,
    RandomSource, ServerConnectionOptions, TimerId, Timestamp, WebSocketClientConnection,
    WebSocketServerConnection,
};

// ============================================================
// テストヘルパー
// ============================================================

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
}

impl RandomSource for FixedRandom {
    fn masking_key(&mut self) -> [u8; 4] {
        self.masking_key
    }

    fn nonce(&mut self) -> [u8; 16] {
        self.nonce
    }
}

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
fn create_valid_handshake_response(accept: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\
         \r\n",
        accept
    )
    .into_bytes()
}

/// 有効なハンドシェイクリクエストを生成
fn create_valid_handshake_request(key: &[u8; 16]) -> Vec<u8> {
    let encoded_key = Base64::encode_string(key.as_slice());
    format!(
        "GET /websocket HTTP/1.1\r\n\
         Host: example.com\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {}\r\n\
         Sec-WebSocket-Version: 13\r\n\
         \r\n",
        encoded_key
    )
    .into_bytes()
}

/// クライアント接続を Connected 状態まで進める
fn setup_connected_client() -> (WebSocketClientConnection<FixedRandom>, Timestamp) {
    let options = ClientConnectionOptions::new("example.com", "/ws");
    let random = FixedRandom::new();
    let nonce = random.nonce;
    let mut conn = WebSocketClientConnection::new(options, random);
    let now = Timestamp::from_millis(0);

    conn.connect().unwrap();
    while conn.poll_output().is_some() {}

    let accept = compute_accept(&nonce);
    let response = create_valid_handshake_response(&accept);
    conn.feed_recv_buf(&response, now).unwrap();

    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    (conn, now)
}

/// サーバー接続を Connected 状態まで進める
fn setup_connected_server() -> WebSocketServerConnection {
    let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
    let key: [u8; 16] = [0; 16];
    let request = create_valid_handshake_request(&key);
    conn.feed_recv_buf(&request).unwrap();
    conn.accept_handshake_auto().unwrap();

    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    conn
}

/// 送信キューから Close フレームを取得し、close code を抽出する (サーバー: マスクなし)
fn extract_close_code_from_unmasked_output(data: &[u8]) -> u16 {
    let mut decoder = FrameDecoder::new();
    decoder.feed(data);
    let frame = decoder
        .decode()
        .expect("デコードに失敗した")
        .expect("フレームが不完全");
    assert_eq!(frame.opcode, Opcode::Close, "Close フレームではない");
    assert!(
        frame.payload.len() >= 2,
        "Close フレームのペイロードが短すぎる"
    );
    u16::from_be_bytes([frame.payload[0], frame.payload[1]])
}

/// 送信キューから Close フレームを取得し、close code を抽出する (クライアント: マスク付き)
fn extract_close_code_from_masked_output(data: &[u8]) -> u16 {
    let mut decoder = FrameDecoder::new();
    decoder.feed(data);
    let decoded = decoder
        .decode_with_info()
        .expect("デコードに失敗した")
        .expect("フレームが不完全");
    assert_eq!(
        decoded.frame.opcode,
        Opcode::Close,
        "Close フレームではない"
    );
    assert!(
        decoded.masked,
        "クライアントからのフレームはマスク付きのはず"
    );
    assert!(
        decoded.frame.payload.len() >= 2,
        "Close フレームのペイロードが短すぎる"
    );
    u16::from_be_bytes([decoded.frame.payload[0], decoded.frame.payload[1]])
}

// ============================================================
// テスト 1: server で不正 UTF-8 テキストフレームによる Close (1007)
// ============================================================

#[test]
fn server_で不正_utf8_テキストフレームを受信すると_close_1007_が送信される() {
    let mut conn = setup_connected_server();

    // 不正な UTF-8 を含むテキストフレーム (マスク付き、サーバーへの送信はマスク必須)
    let mask_key = [0xAB, 0xCD, 0xEF, 0x12];
    let invalid_utf8 = vec![0xFF, 0xFE, 0x80];
    let frame = Frame::new(Opcode::Text, invalid_utf8).encode(mask_key);

    let result = conn.feed_recv_buf(&frame);
    assert!(result.is_err(), "不正な UTF-8 テキストはエラーを返すべき");

    // イベントキューに ConnectionEvent::Error が積まれていることを確認する
    let mut error_found = false;
    while let Some(event) = conn.poll_event() {
        if let ConnectionEvent::Error(msg) = event {
            assert!(
                msg.contains("invalid UTF-8"),
                "エラーメッセージに 'invalid UTF-8' が含まれるべき: {}",
                msg
            );
            error_found = true;
            break;
        }
    }
    assert!(
        error_found,
        "ConnectionEvent::Error がイベントキューに積まれていない"
    );

    // 送信キューに Close フレーム (close code 1007) が積まれていることを確認する
    let mut close_found = false;
    while let Some(output) = conn.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            let code = extract_close_code_from_unmasked_output(&data);
            assert_eq!(
                code, 1007,
                "Close code は 1007 (Invalid Payload Data) であるべき"
            );
            close_found = true;
            break;
        }
    }
    assert!(close_found, "送信キューに Close フレームが積まれていない");
}

// ============================================================
// テスト 2: client/server の PongTimeout で Close (1008)
// ============================================================

#[test]
fn client_で_pong_timeout_が発生すると_close_1008_が送信される() {
    let (mut conn, _now) = setup_connected_client();

    // send_ping で awaiting_pong = true にする
    conn.send_ping(&[]).unwrap();
    while conn.poll_output().is_some() {}

    // PongTimeout タイマーを発火させる
    let result = conn.handle_timer(TimerId::PongTimeout);
    assert!(result.is_ok(), "handle_timer は Ok(()) を返すべき");

    // イベントキューに ConnectionEvent::Error("pong timeout") が積まれていることを確認する
    let mut error_found = false;
    while let Some(event) = conn.poll_event() {
        if let ConnectionEvent::Error(msg) = event {
            assert_eq!(
                msg, "pong timeout",
                "エラーメッセージは 'pong timeout' であるべき"
            );
            error_found = true;
            break;
        }
    }
    assert!(
        error_found,
        "ConnectionEvent::Error(\"pong timeout\") がイベントキューに積まれていない"
    );

    // 送信キューに Close フレーム (close code 1008) が積まれていることを確認する
    // クライアントからの送信はマスク付き
    let mut close_found = false;
    while let Some(output) = conn.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            let code = extract_close_code_from_masked_output(&data);
            assert_eq!(
                code, 1008,
                "Close code は 1008 (Policy Violation) であるべき"
            );
            close_found = true;
            break;
        }
    }
    assert!(close_found, "送信キューに Close フレームが積まれていない");
}

#[test]
fn server_で_pong_timeout_が発生すると_close_1008_が送信される() {
    let mut conn = setup_connected_server();

    // send_ping で awaiting_pong = true にする
    conn.send_ping(&[]).unwrap();
    while conn.poll_output().is_some() {}

    // PongTimeout タイマーを発火させる
    let result = conn.handle_timer(TimerId::PongTimeout);
    assert!(result.is_ok(), "handle_timer は Ok(()) を返すべき");

    // イベントキューに ConnectionEvent::Error("pong timeout") が積まれていることを確認する
    let mut error_found = false;
    while let Some(event) = conn.poll_event() {
        if let ConnectionEvent::Error(msg) = event {
            assert_eq!(
                msg, "pong timeout",
                "エラーメッセージは 'pong timeout' であるべき"
            );
            error_found = true;
            break;
        }
    }
    assert!(
        error_found,
        "ConnectionEvent::Error(\"pong timeout\") がイベントキューに積まれていない"
    );

    // 送信キューに Close フレーム (close code 1008) が積まれていることを確認する
    // サーバーからの送信はマスクなし
    let mut close_found = false;
    while let Some(output) = conn.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            let code = extract_close_code_from_unmasked_output(&data);
            assert_eq!(
                code, 1008,
                "Close code は 1008 (Policy Violation) であるべき"
            );
            close_found = true;
            break;
        }
    }
    assert!(close_found, "送信キューに Close フレームが積まれていない");
}

// ============================================================
// テスト 3: client で masked フレーム受信による Close (1002)
// ============================================================

#[test]
fn client_で_masked_フレームを受信すると_close_1002_が送信される() {
    let (mut conn, now) = setup_connected_client();

    // サーバーからマスク付きフレームを送信する (RFC 6455 Section 5.1 違反)
    let masked_frame = Frame::new(Opcode::Text, b"hello".to_vec()).encode([0xAB, 0xCD, 0xEF, 0x12]);
    let result = conn.feed_recv_buf(&masked_frame, now);
    assert!(
        result.is_err(),
        "マスク付きサーバーフレームはエラーを返すべき"
    );

    // 送信キューに Close フレーム (close code 1002) が積まれていることを確認する
    // クライアントからの送信はマスク付き
    let mut close_found = false;
    while let Some(output) = conn.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            let code = extract_close_code_from_masked_output(&data);
            assert_eq!(code, 1002, "Close code は 1002 (Protocol Error) であるべき");
            close_found = true;
            break;
        }
    }
    assert!(close_found, "送信キューに Close フレームが積まれていない");
}

// ============================================================
// テスト 4: server で unmasked フレーム受信による Close (1002)
// ============================================================

#[test]
fn server_で_unmasked_フレームを受信すると_close_1002_が送信される() {
    let mut conn = setup_connected_server();

    // クライアントからマスクなしフレームを送信する (RFC 6455 Section 5.1 違反)
    let unmasked_frame = Frame::new(Opcode::Text, b"hello".to_vec()).encode_unmasked();
    let result = conn.feed_recv_buf(&unmasked_frame);
    assert!(
        result.is_err(),
        "マスクなしクライアントフレームはエラーを返すべき"
    );

    // 送信キューに Close フレーム (close code 1002) が積まれていることを確認する
    // サーバーからの送信はマスクなし
    let mut close_found = false;
    while let Some(output) = conn.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            let code = extract_close_code_from_unmasked_output(&data);
            assert_eq!(code, 1002, "Close code は 1002 (Protocol Error) であるべき");
            close_found = true;
            break;
        }
    }
    assert!(close_found, "送信キューに Close フレームが積まれていない");
}
