//! websocket_connection_shared.rs の単体テスト
//!
//! SharedConnectionState は pub(crate) のため、
//! WebSocketClientConnection / WebSocketServerConnection の公開 API 経由でテストする。
//! PBT では実現しにくいエラーパス・境界値の検証を行う。

use base64ct::{Base64, Encoding};
use shiguredo_websocket::{
    ClientConnectionOptions, CloseCode, ConnectionEvent, ConnectionOutput, ErrorKind, Frame,
    FrameDecoder, Opcode, PerMessageDeflate, PerMessageDeflateConfig, RandomSource,
    ServerConnectionOptions, TimerId, Timestamp, WebSocketClientConnection,
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

    conn.connect().expect("接続処理に失敗してはならない");
    while conn.poll_output().is_some() {}

    let accept = compute_accept(&nonce);
    let response = create_valid_handshake_response(&accept);
    conn.feed_recv_buf(&response, now)
        .expect("受信データの投入に失敗してはならない");

    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    (conn, now)
}

/// サーバー接続を Connected 状態まで進める
fn setup_connected_server() -> WebSocketServerConnection {
    let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
    let key: [u8; 16] = [0; 16];
    let request = create_valid_handshake_request(&key);
    conn.feed_recv_buf(&request)
        .expect("受信データの投入に失敗してはならない");
    conn.accept_handshake_auto()
        .expect("ハンドシェイクの受諾に失敗してはならない");

    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    conn
}

/// 拡張ヘッダー付きの有効なハンドシェイクリクエストを生成する
fn create_valid_handshake_request_with_extensions(key: &[u8; 16], extensions: &str) -> Vec<u8> {
    let encoded_key = Base64::encode_string(key.as_slice());
    format!(
        "GET /websocket HTTP/1.1\r\n\
         Host: example.com\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {}\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Extensions: {}\r\n\
         \r\n",
        encoded_key, extensions
    )
    .into_bytes()
}

/// メッセージサイズとフレームサイズの上限を設定したサーバー接続を Connected 状態まで進める
fn setup_connected_server_with_limits(
    max_message_size: usize,
    max_frame_size: usize,
) -> WebSocketServerConnection {
    let options = ServerConnectionOptions::new()
        .max_message_size(max_message_size)
        .max_frame_size(max_frame_size);
    let mut conn = WebSocketServerConnection::new(options);
    let key: [u8; 16] = [0; 16];
    let request = create_valid_handshake_request(&key);
    conn.feed_recv_buf(&request)
        .expect("受信データの投入に失敗してはならない");
    conn.accept_handshake_auto()
        .expect("ハンドシェイクの受諾に失敗してはならない");

    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    conn
}

/// permessage-deflate を合意したサーバー接続を Connected 状態まで進める
fn setup_connected_server_with_deflate(max_message_size: usize) -> WebSocketServerConnection {
    let options = ServerConnectionOptions::new()
        .max_message_size(max_message_size)
        .max_frame_size(1000)
        .deflate(PerMessageDeflateConfig::default());
    let mut conn = WebSocketServerConnection::new(options);
    let key: [u8; 16] = [0; 16];
    let request = create_valid_handshake_request_with_extensions(&key, "permessage-deflate");
    conn.feed_recv_buf(&request)
        .expect("受信データの投入に失敗してはならない");
    conn.accept_handshake_auto()
        .expect("ハンドシェイクの受諾に失敗してはならない");

    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    conn
}

/// RSV1 を立てたマスク済みの圧縮フレームを生成する
fn compressed_frame(opcode: Opcode, payload: Vec<u8>) -> Vec<u8> {
    Frame {
        fin: true,
        rsv1: true,
        rsv2: false,
        rsv3: false,
        opcode,
        payload,
    }
    .encode([0xAB, 0xCD, 0xEF, 0x12])
}

/// 拡張ヘッダー付きの 101 レスポンスを生成する
fn create_valid_handshake_response_with_extensions(accept: &str, extensions: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\
         Sec-WebSocket-Extensions: {}\r\n\
         \r\n",
        accept, extensions
    )
    .into_bytes()
}

/// permessage-deflate を合意したクライアント接続を Connected 状態まで進める
fn setup_connected_client_with_deflate(
    max_decompressed_size: usize,
) -> (WebSocketClientConnection<FixedRandom>, Timestamp) {
    let options = ClientConnectionOptions::new("example.com", "/ws")
        .deflate(PerMessageDeflateConfig::default())
        .max_decompressed_size(max_decompressed_size);
    let random = FixedRandom::new();
    let nonce = random.nonce;
    let mut conn = WebSocketClientConnection::new(options, random);
    let now = Timestamp::from_millis(0);

    conn.connect().expect("接続処理に失敗してはならない");
    while conn.poll_output().is_some() {}

    let accept = compute_accept(&nonce);
    let response = create_valid_handshake_response_with_extensions(&accept, "permessage-deflate");
    conn.feed_recv_buf(&response, now)
        .expect("受信データの投入に失敗してはならない");

    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    (conn, now)
}

/// permessage-deflate を合意し、解凍後サイズの上限を設定したサーバー接続
fn setup_connected_server_with_decompressed_limit(
    max_decompressed_size: usize,
) -> WebSocketServerConnection {
    let options = ServerConnectionOptions::new()
        .max_message_size(1000)
        .max_frame_size(1000)
        .max_decompressed_size(max_decompressed_size)
        .deflate(PerMessageDeflateConfig::default());
    let mut conn = WebSocketServerConnection::new(options);
    let key: [u8; 16] = [0; 16];
    let request = create_valid_handshake_request_with_extensions(&key, "permessage-deflate");
    conn.feed_recv_buf(&request)
        .expect("受信データの投入に失敗してはならない");
    conn.accept_handshake_auto()
        .expect("ハンドシェイクの受諾に失敗してはならない");

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
    conn.send_ping(&[])
        .expect("Ping の送信に失敗してはならない");
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
    conn.send_ping(&[])
        .expect("Ping の送信に失敗してはならない");
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

// ============================================================
// テスト 5: ClientConnectionOptions / ServerConnectionOptions の
// pong_timeout / close_timeout ビルダーが SetTimer の duration_millis に伝播する
// ============================================================

/// 任意の `duration_millis` を持つ `SetTimer` 出力が含まれているか確認する
fn find_set_timer_with_duration(
    outputs: &[ConnectionOutput],
    target_id: TimerId,
    expected_duration: u64,
) -> bool {
    outputs.iter().any(|o| {
        matches!(o, ConnectionOutput::SetTimer { id, duration_millis }
            if *id == target_id && *duration_millis == expected_duration)
    })
}

#[test]
fn client_で_pong_timeout_ビルダー値が_set_timer_に伝播する() {
    let options = ClientConnectionOptions::new("example.com", "/ws").pong_timeout(33_333);
    let random = FixedRandom::new();
    let nonce = random.nonce;
    let mut conn = WebSocketClientConnection::new(options, random);
    let now = Timestamp::from_millis(0);

    conn.connect().expect("接続処理に失敗してはならない");
    while conn.poll_output().is_some() {}

    let accept = compute_accept(&nonce);
    let response = create_valid_handshake_response(&accept);
    conn.feed_recv_buf(&response, now)
        .expect("ハンドシェイク応答の投入に失敗してはならない");
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    conn.send_ping(&[])
        .expect("Ping の送信に失敗してはならない");

    let mut outputs = Vec::new();
    while let Some(o) = conn.poll_output() {
        outputs.push(o);
    }
    assert!(
        find_set_timer_with_duration(&outputs, TimerId::PongTimeout, 33_333),
        "PongTimeout の SetTimer が pong_timeout(33333) を反映していない"
    );
}

#[test]
fn client_で_close_timeout_ビルダー値が_set_timer_に伝播する() {
    use shiguredo_websocket::CloseCode;

    let options = ClientConnectionOptions::new("example.com", "/ws").close_timeout(44_444);
    let random = FixedRandom::new();
    let nonce = random.nonce;
    let mut conn = WebSocketClientConnection::new(options, random);
    let now = Timestamp::from_millis(0);

    conn.connect().expect("接続処理に失敗してはならない");
    while conn.poll_output().is_some() {}

    let accept = compute_accept(&nonce);
    let response = create_valid_handshake_response(&accept);
    conn.feed_recv_buf(&response, now)
        .expect("ハンドシェイク応答の投入に失敗してはならない");
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    conn.close(CloseCode::NORMAL, "bye")
        .expect("クローズ処理に失敗してはならない");

    let mut outputs = Vec::new();
    while let Some(o) = conn.poll_output() {
        outputs.push(o);
    }
    assert!(
        find_set_timer_with_duration(&outputs, TimerId::CloseTimeout, 44_444),
        "CloseTimeout の SetTimer が close_timeout(44444) を反映していない"
    );
}

#[test]
fn server_で_pong_timeout_ビルダー値が_set_timer_に伝播する() {
    let options = ServerConnectionOptions::new().pong_timeout(22_222);
    let mut conn = WebSocketServerConnection::new(options);
    let key: [u8; 16] = [0; 16];
    let request = create_valid_handshake_request(&key);
    conn.feed_recv_buf(&request)
        .expect("ハンドシェイク応答の投入に失敗してはならない");
    conn.accept_handshake_auto()
        .expect("ハンドシェイクの受諾に失敗してはならない");
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    conn.send_ping(&[])
        .expect("Ping の送信に失敗してはならない");
    let mut outputs = Vec::new();
    while let Some(o) = conn.poll_output() {
        outputs.push(o);
    }
    assert!(
        find_set_timer_with_duration(&outputs, TimerId::PongTimeout, 22_222),
        "PongTimeout の SetTimer が pong_timeout(22222) を反映していない"
    );
}

#[test]
fn server_で_close_timeout_ビルダー値が_set_timer_に伝播する() {
    use shiguredo_websocket::CloseCode;

    let options = ServerConnectionOptions::new().close_timeout(11_111);
    let mut conn = WebSocketServerConnection::new(options);
    let key: [u8; 16] = [0; 16];
    let request = create_valid_handshake_request(&key);
    conn.feed_recv_buf(&request)
        .expect("ハンドシェイク応答の投入に失敗してはならない");
    conn.accept_handshake_auto()
        .expect("ハンドシェイクの受諾に失敗してはならない");
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    conn.close(CloseCode::NORMAL, "bye")
        .expect("クローズ処理に失敗してはならない");
    let mut outputs = Vec::new();
    while let Some(o) = conn.poll_output() {
        outputs.push(o);
    }
    assert!(
        find_set_timer_with_duration(&outputs, TimerId::CloseTimeout, 11_111),
        "CloseTimeout の SetTimer が close_timeout(11111) を反映していない"
    );
}

// ============================================================
// max_message_size を未分割メッセージにも適用する
// ============================================================

/// 未分割テキストが上限を超えると Close 1009 で拒否される
#[test]
fn server_で未分割テキストが_max_message_size_を超えると_close_1009_が送信される() {
    let mut conn = setup_connected_server_with_limits(10, 1000);
    let frame = Frame::new(Opcode::Text, b"0123456789a".to_vec()).encode([0xAB, 0xCD, 0xEF, 0x12]);

    let err = conn
        .feed_recv_buf(&frame)
        .expect_err("上限を超える未分割メッセージは拒否される");
    assert_eq!(err.kind, ErrorKind::ProtocolViolation);

    let mut close_found = false;
    while let Some(output) = conn.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            assert_eq!(
                extract_close_code_from_unmasked_output(&data),
                1009,
                "Close code は 1009 (Message Too Big) であるべき"
            );
            close_found = true;
            break;
        }
    }
    assert!(close_found, "送信キューに Close フレームが積まれていない");
}

/// 未分割バイナリが上限を超えると Close 1009 で拒否される
#[test]
fn server_で未分割バイナリが_max_message_size_を超えると_close_1009_が送信される() {
    let mut conn = setup_connected_server_with_limits(10, 1000);
    let frame = Frame::new(Opcode::Binary, vec![0u8; 11]).encode([0xAB, 0xCD, 0xEF, 0x12]);

    let err = conn
        .feed_recv_buf(&frame)
        .expect_err("上限を超える未分割メッセージは拒否される");
    assert_eq!(err.kind, ErrorKind::ProtocolViolation);

    let mut close_found = false;
    while let Some(output) = conn.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            assert_eq!(
                extract_close_code_from_unmasked_output(&data),
                1009,
                "Close code は 1009 (Message Too Big) であるべき"
            );
            close_found = true;
            break;
        }
    }
    assert!(close_found, "送信キューに Close フレームが積まれていない");
}

/// 未分割メッセージが上限ちょうどなら受理される
#[test]
fn server_で未分割メッセージが_max_message_size_ちょうどなら受理される() {
    let mut conn = setup_connected_server_with_limits(10, 1000);

    let text_frame =
        Frame::new(Opcode::Text, b"0123456789".to_vec()).encode([0xAB, 0xCD, 0xEF, 0x12]);
    conn.feed_recv_buf(&text_frame)
        .expect("上限ちょうどのテキストは受理される");
    match conn.poll_event() {
        Some(ConnectionEvent::TextMessage(text)) => assert_eq!(text, "0123456789"),
        other => panic!("TextMessage を期待したが {:?} だった", other),
    }

    let binary_frame = Frame::new(Opcode::Binary, vec![0u8; 10]).encode([0xAB, 0xCD, 0xEF, 0x12]);
    conn.feed_recv_buf(&binary_frame)
        .expect("上限ちょうどのバイナリは受理される");
    match conn.poll_event() {
        Some(ConnectionEvent::BinaryMessage(data)) => assert_eq!(data, vec![0u8; 10]),
        other => panic!("BinaryMessage を期待したが {:?} だった", other),
    }
}

/// 圧縮メッセージの圧縮後ペイロードが上限を超えると Close 1009 で拒否される
#[test]
fn server_で未分割の圧縮メッセージが_max_message_size_を超えると_close_1009_が送信される() {
    let mut conn = setup_connected_server_with_deflate(10);
    let mut compressor = PerMessageDeflate::new_client(PerMessageDeflateConfig::default());
    let compressed = compressor
        .compress(b"hi")
        .expect("圧縮に失敗してはならない");
    assert!(
        compressed.len() > 10 && compressed.len() <= 1000,
        "テスト前提: 圧縮後サイズが上限を超え、フレームサイズ以内である ({} bytes)",
        compressed.len()
    );
    let frame = compressed_frame(Opcode::Text, compressed);

    let err = conn
        .feed_recv_buf(&frame)
        .expect_err("圧縮後ペイロードが上限を超えるメッセージは拒否される");
    assert_eq!(err.kind, ErrorKind::ProtocolViolation);

    let mut close_found = false;
    while let Some(output) = conn.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            assert_eq!(
                extract_close_code_from_unmasked_output(&data),
                1009,
                "Close code は 1009 (Message Too Big) であるべき"
            );
            close_found = true;
            break;
        }
    }
    assert!(close_found, "送信キューに Close フレームが積まれていない");
}

/// 圧縮メッセージの圧縮後ペイロードが上限以下なら受理される
#[test]
fn server_で未分割の圧縮メッセージが_max_message_size_以下なら受理される() {
    let mut conn = setup_connected_server_with_deflate(20);
    let mut compressor = PerMessageDeflate::new_client(PerMessageDeflateConfig::default());
    let original = vec![b'a'; 1000];
    let compressed = compressor
        .compress(&original)
        .expect("圧縮に失敗してはならない");
    assert!(
        compressed.len() <= 20,
        "テスト前提: 圧縮後サイズが上限以下である ({} bytes)",
        compressed.len()
    );
    let frame = compressed_frame(Opcode::Text, compressed);

    conn.feed_recv_buf(&frame)
        .expect("圧縮後ペイロードが上限以下のメッセージは受理される");
    match conn.poll_event() {
        Some(ConnectionEvent::TextMessage(text)) => {
            assert_eq!(text, "a".repeat(1000), "解凍後のメッセージが一致する");
        }
        other => panic!("TextMessage を期待したが {:?} だった", other),
    }
}

// ============================================================
// 解凍エラー時の Close フレーム
// ============================================================

/// サーバーで不正な DEFLATE データを受けると Close 1007 が送信される
#[test]
fn server_で解凍エラーが発生すると_close_1007_が送信される() {
    let mut conn = setup_connected_server_with_deflate(1000);
    let frame = Frame {
        fin: true,
        rsv1: true,
        rsv2: false,
        rsv3: false,
        opcode: Opcode::Text,
        payload: vec![0xFF],
    }
    .encode([0xAB, 0xCD, 0xEF, 0x12]);

    let err = conn
        .feed_recv_buf(&frame)
        .expect_err("不正な DEFLATE データは拒否される");
    assert_eq!(err.kind, ErrorKind::InvalidData);
    assert_eq!(err.close_code, Some(1007));

    let mut close_found = false;
    while let Some(output) = conn.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            assert_eq!(
                extract_close_code_from_unmasked_output(&data),
                1007,
                "Close code は 1007 (Invalid Payload) であるべき"
            );
            close_found = true;
            break;
        }
    }
    assert!(close_found, "送信キューに Close フレームが積まれていない");
}

/// クライアントで不正な DEFLATE データを受けると Close 1007 が送信される
#[test]
fn client_で解凍エラーが発生すると_close_1007_が送信される() {
    let (mut conn, now) = setup_connected_client_with_deflate(16 * 1024 * 1024);
    let frame = Frame {
        fin: true,
        rsv1: true,
        rsv2: false,
        rsv3: false,
        opcode: Opcode::Text,
        payload: vec![0xFF],
    }
    .encode_unmasked();

    let err = conn
        .feed_recv_buf(&frame, now)
        .expect_err("不正な DEFLATE データは拒否される");
    assert_eq!(err.kind, ErrorKind::InvalidData);
    assert_eq!(err.close_code, Some(1007));

    let mut close_found = false;
    while let Some(output) = conn.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            assert_eq!(
                extract_close_code_from_masked_output(&data),
                1007,
                "Close code は 1007 (Invalid Payload) であるべき"
            );
            close_found = true;
            break;
        }
    }
    assert!(close_found, "送信キューに Close フレームが積まれていない");
}

/// サーバーで解凍後サイズが max_decompressed_size を超えると Close 1009 が送信される
#[test]
fn server_で解凍後サイズ超過が発生すると_close_1009_が送信される() {
    let mut conn = setup_connected_server_with_decompressed_limit(10);
    let mut compressor = PerMessageDeflate::new_client(PerMessageDeflateConfig::default());
    let compressed = compressor
        .compress(&[b'a'; 100])
        .expect("圧縮に失敗してはならない");
    assert!(
        compressed.len() < 1000,
        "テスト前提: 圧縮後サイズがフレーム / メッセージの上限より小さい ({} bytes)",
        compressed.len()
    );
    let frame = Frame {
        fin: true,
        rsv1: true,
        rsv2: false,
        rsv3: false,
        opcode: Opcode::Text,
        payload: compressed,
    }
    .encode([0xAB, 0xCD, 0xEF, 0x12]);

    let err = conn
        .feed_recv_buf(&frame)
        .expect_err("解凍後サイズが上限を超えるメッセージは拒否される");
    assert_eq!(err.kind, ErrorKind::InvalidData);
    assert_eq!(err.close_code, Some(1009));

    let mut close_found = false;
    while let Some(output) = conn.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            assert_eq!(
                extract_close_code_from_unmasked_output(&data),
                1009,
                "Close code は 1009 (Message Too Big) であるべき"
            );
            close_found = true;
            break;
        }
    }
    assert!(close_found, "送信キューに Close フレームが積まれていない");
}

/// permessage-deflate は切り詰めストリームを検出しない (既知の制限)
///
/// noflate は入力枯渇をエラーにせず、BFINAL=0 のストリームは `is_finished` で
/// 終端を判定できない。切り詰めストリームの完全性検出は行わない。
/// このテストは修正前の実装でも通る characterization test であり、
/// 切り詰めが検出されない現状の挙動を固定する。
#[test]
fn permessage_deflate_は切り詰めストリームを検出しない() {
    let mut compressor = PerMessageDeflate::new_client(PerMessageDeflateConfig::default());
    let full = compressor
        .compress(b"hello")
        .expect("圧縮に失敗してはならない");
    let truncated = &full[..full.len() - 1];
    let mut decompressor = PerMessageDeflate::new_server(PerMessageDeflateConfig::default());

    let result = decompressor.decompress(truncated, 1024);
    let decoded = result.expect("切り詰めストリームは現状エラーにならない (既知の制限)");
    assert_ne!(
        decoded,
        b"hello".to_vec(),
        "切り詰めストリームは元のデータとして復元されない"
    );
}

/// 128 KiB を超える完全な圧縮メッセージが切り詰められない
#[test]
fn permessage_deflate_は_128ki_b_超のメッセージを完全に解凍する() {
    let mut compressor = PerMessageDeflate::new_client(PerMessageDeflateConfig::default());
    let original = vec![b'a'; 300_000];
    let compressed = compressor
        .compress(&original)
        .expect("圧縮に失敗してはならない");
    let mut decompressor = PerMessageDeflate::new_server(PerMessageDeflateConfig::default());

    let decoded = decompressor
        .decompress(&compressed, 16 * 1024 * 1024)
        .expect("解凍に失敗してはならない");
    assert_eq!(
        decoded.len(),
        original.len(),
        "解凍結果が切り詰められていない"
    );
    assert_eq!(decoded, original, "解凍結果が元のデータと一致する");
}

/// RFC 7692 Section 7.2.3.4: BFINAL=1 のメッセージは現状未対応で 1007 になる
#[test]
fn permessage_deflate_は_bfinal_1_のメッセージを未対応として拒否する() {
    let payload = [0xF3, 0x48, 0xCD, 0xC9, 0xC9, 0x07, 0x00, 0x00];
    let mut decompressor = PerMessageDeflate::new_server(PerMessageDeflateConfig::default());

    let err = decompressor
        .decompress(&payload, 1024)
        .expect_err("BFINAL=1 のメッセージは未対応で拒否される");
    assert_eq!(err.kind, ErrorKind::InvalidData);
    assert_eq!(err.close_code, Some(1007));
}

/// BFINAL=1 のメッセージは接続レベルでも Close 1007 で拒否される
#[test]
fn server_で_bfinal_1_の圧縮メッセージは_close_1007_で拒否される() {
    let mut conn = setup_connected_server_with_deflate(1000);
    let payload = [0xF3, 0x48, 0xCD, 0xC9, 0xC9, 0x07, 0x00, 0x00];
    let frame = compressed_frame(Opcode::Text, payload.to_vec());

    let err = conn
        .feed_recv_buf(&frame)
        .expect_err("BFINAL=1 のメッセージは未対応で拒否される");
    assert_eq!(err.close_code, Some(1007));

    let mut close_found = false;
    while let Some(output) = conn.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            assert_eq!(
                extract_close_code_from_unmasked_output(&data),
                1007,
                "Close code は 1007 (Invalid Payload) であるべき"
            );
            close_found = true;
            break;
        }
    }
    assert!(close_found, "送信キューに Close フレームが積まれていない");

    while let Some(event) = conn.poll_event() {
        assert!(
            !matches!(event, ConnectionEvent::TextMessage(_)),
            "未対応のメッセージを emit しない"
        );
    }
}

/// close() は送信不可コードを InvalidInput で拒否する
#[test]
fn server_close_rejects_unsendable_code() {
    let mut conn = setup_connected_server();
    let err = conn
        .close(CloseCode::new(1016), "")
        .expect_err("送信不可コードは拒否される");
    assert_eq!(err.kind, ErrorKind::InvalidInput);
}
