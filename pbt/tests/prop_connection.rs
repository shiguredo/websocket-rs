//! WebSocketClientConnection のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - 状態遷移の検証
//! - メッセージ送受信
//! - タイマー管理
//! - エラーケース

use base64::Engine;
use proptest::prelude::*;
use sha1::{Digest, Sha1};
use shiguredo_websocket::{
    ClientConnectionOptions, CloseCode, ConnectionEvent, ConnectionOutput, ConnectionState,
    PerMessageDeflateConfig, TimerId, Timestamp, WebSocketClientConnection,
};

// =============================================================================
// ヘルパー関数
// =============================================================================

/// テスト用: nonce から Sec-WebSocket-Accept を計算
fn calculate_accept(nonce: &[u8; 16]) -> String {
    const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let key = base64::engine::general_purpose::STANDARD.encode(nonce);
    let combined = format!("{}{}", key, WEBSOCKET_GUID);
    let mut hasher = Sha1::new();
    hasher.update(combined.as_bytes());
    let hash = hasher.finalize();
    base64::engine::general_purpose::STANDARD.encode(hash)
}

/// クライアント接続を Connected 状態にして返す
fn setup_connected_client(
    options: ClientConnectionOptions,
) -> (WebSocketClientConnection, Timestamp) {
    let mut conn = WebSocketClientConnection::new(options);
    let now = Timestamp::from_millis(0);
    conn.connect(now).unwrap();

    // nonce を取得するために SendData を見る
    let output = conn.poll_output().unwrap();
    let nonce = if let ConnectionOutput::SendData(data) = output {
        let s = String::from_utf8_lossy(&data);
        // Sec-WebSocket-Key: を探す
        if let Some(pos) = s.find("Sec-WebSocket-Key: ") {
            let start = pos + 19;
            let end = s[start..].find("\r\n").unwrap() + start;
            let key = &s[start..end];
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(key)
                .unwrap();
            let mut nonce = [0u8; 16];
            nonce.copy_from_slice(&decoded);
            nonce
        } else {
            panic!("Sec-WebSocket-Key not found");
        }
    } else {
        panic!("expected SendData");
    };

    // ハンドシェイクレスポンスを送信
    let accept = calculate_accept(&nonce);
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\
         \r\n",
        accept
    );
    conn.feed_recv_buf(response.as_bytes(), now).unwrap();

    // キューをクリア
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    (conn, now)
}

// =============================================================================
// 接続オプションのテスト
// =============================================================================

proptest! {
    /// ホストとパスは常に設定される
    #[test]
    fn prop_connection_options_basic(
        host in "[a-z][a-z0-9-]{0,20}\\.[a-z]{2,6}",
        path in "/[a-zA-Z0-9/_-]{0,50}"
    ) {
        let options = ClientConnectionOptions::new(&host, &path);
        // オプションが正しく作成される（パニックしない）
        let _ = WebSocketClientConnection::new(options);
    }

    /// origin の設定
    #[test]
    fn prop_connection_options_origin(
        host in "[a-z]{3,10}\\.[a-z]{2,4}",
        origin in "https://[a-z]{3,10}\\.[a-z]{2,4}"
    ) {
        let options = ClientConnectionOptions::new(&host, "/")
            .origin(&origin);
        let _ = WebSocketClientConnection::new(options);
    }

    /// protocol の設定
    #[test]
    fn prop_connection_options_protocol(
        host in "[a-z]{3,10}\\.[a-z]{2,4}",
        protocol in "[a-z][a-z0-9-]{0,20}"
    ) {
        let options = ClientConnectionOptions::new(&host, "/")
            .protocol(&protocol);
        let _ = WebSocketClientConnection::new(options);
    }

    /// ping_interval の設定
    #[test]
    fn prop_connection_options_ping_interval(
        host in "[a-z]{3,10}\\.[a-z]{2,4}",
        interval in 1000u64..300000
    ) {
        let options = ClientConnectionOptions::new(&host, "/")
            .ping_interval(interval);
        let _ = WebSocketClientConnection::new(options);
    }
}

// =============================================================================
// 状態遷移のテスト
// =============================================================================

proptest! {
    /// 初期状態は Disconnected
    #[test]
    fn prop_initial_state_disconnected(
        host in "[a-z]{3,10}\\.[a-z]{2,4}"
    ) {
        let options = ClientConnectionOptions::new(&host, "/");
        let ws = WebSocketClientConnection::new(options);
        prop_assert_eq!(ws.state(), ConnectionState::Disconnected);
    }

    /// connect() 後は Connecting
    #[test]
    fn prop_state_after_connect(
        host in "[a-z]{3,10}\\.[a-z]{2,4}",
        millis in 0u64..u64::MAX
    ) {
        let options = ClientConnectionOptions::new(&host, "/");
        let mut ws = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(millis);

        ws.connect(now).unwrap();
        prop_assert_eq!(ws.state(), ConnectionState::Connecting);
    }

    /// connect() は SendData を出力
    #[test]
    fn prop_connect_outputs_send_data(
        host in "[a-z]{3,10}\\.[a-z]{2,4}"
    ) {
        let options = ClientConnectionOptions::new(&host, "/");
        let mut ws = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        ws.connect(now).unwrap();

        let output = ws.poll_output();
        prop_assert!(output.is_some());

        if let Some(ConnectionOutput::SendData(data)) = output {
            // HTTP リクエストが生成される
            let s = String::from_utf8_lossy(&data);
            prop_assert!(s.starts_with("GET "));
            prop_assert!(s.contains("HTTP/1.1"));
            prop_assert!(s.contains("Upgrade: websocket"));
        }
    }
}

// =============================================================================
// Timestamp のテスト
// =============================================================================

proptest! {
    /// Timestamp の生成
    #[test]
    fn prop_timestamp_from_millis(millis in any::<u64>()) {
        let ts = Timestamp::from_millis(millis);
        prop_assert_eq!(ts.as_millis(), millis);
    }

    /// Timestamp の比較
    #[test]
    fn prop_timestamp_ordering(a in any::<u64>(), b in any::<u64>()) {
        let ts_a = Timestamp::from_millis(a);
        let ts_b = Timestamp::from_millis(b);

        prop_assert_eq!(ts_a < ts_b, a < b);
        prop_assert_eq!(ts_a == ts_b, a == b);
        prop_assert_eq!(ts_a > ts_b, a > b);
    }

    /// Timestamp の加算 (add_millis)
    #[test]
    fn prop_timestamp_add(base in 0u64..u64::MAX/2, offset in 0u64..u64::MAX/2) {
        let ts = Timestamp::from_millis(base);
        let result = ts.add_millis(offset);
        prop_assert_eq!(result.as_millis(), base + offset);
    }

    /// Timestamp の減算 (saturating_sub)
    #[test]
    fn prop_timestamp_saturating_sub(a in any::<u64>(), b in any::<u64>()) {
        let ts_a = Timestamp::from_millis(a);
        let ts_b = Timestamp::from_millis(b);

        let result = ts_a.saturating_sub(ts_b);
        prop_assert_eq!(result, a.saturating_sub(b));
    }

    /// Timestamp の Add 演算子
    #[test]
    fn prop_timestamp_add_operator(base in 0u64..u64::MAX/2, offset in 0u64..u64::MAX/2) {
        let ts = Timestamp::from_millis(base);
        let result = ts + offset;
        prop_assert_eq!(result.as_millis(), base.saturating_add(offset));
    }

    /// Timestamp の Sub 演算子
    #[test]
    fn prop_timestamp_sub_operator(a in any::<u64>(), b in any::<u64>()) {
        let ts_a = Timestamp::from_millis(a);
        let ts_b = Timestamp::from_millis(b);

        let result = ts_a - ts_b;
        prop_assert_eq!(result, a.saturating_sub(b));
    }

    /// Timestamp::default は 0
    #[test]
    fn prop_timestamp_default(_dummy in 0u8..1) {
        let ts = Timestamp::default();
        prop_assert_eq!(ts.as_millis(), 0);
    }

    /// Timestamp はオーバーフローで saturate する
    #[test]
    fn prop_timestamp_saturating_add_overflow(base in u64::MAX-1000..u64::MAX, offset in 1u64..1000) {
        let ts = Timestamp::from_millis(base);
        let result = ts + offset;
        // saturating_add の動作確認
        prop_assert_eq!(result.as_millis(), base.saturating_add(offset));
    }
}

// =============================================================================
// メッセージ送信のテスト（Connecting 状態）
// =============================================================================

proptest! {
    /// Connecting 状態ではメッセージ送信できない
    #[test]
    fn prop_cannot_send_while_connecting(
        host in "[a-z]{3,10}\\.[a-z]{2,4}",
        message in "\\PC{1,100}"
    ) {
        let options = ClientConnectionOptions::new(&host, "/");
        let mut ws = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        ws.connect(now).unwrap();

        // Connecting 状態でメッセージ送信はエラー
        let result = ws.send_text(&message, now);
        prop_assert!(result.is_err());
    }

    /// Disconnected 状態ではメッセージ送信できない
    #[test]
    fn prop_cannot_send_while_disconnected(
        host in "[a-z]{3,10}\\.[a-z]{2,4}",
        message in "\\PC{1,100}"
    ) {
        let options = ClientConnectionOptions::new(&host, "/");
        let mut ws = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        // 接続せずにメッセージ送信
        let result = ws.send_text(&message, now);
        prop_assert!(result.is_err());
    }
}

// =============================================================================
// イベントキューのテスト
// =============================================================================

proptest! {
    /// connect() 後は StateChanged イベント
    #[test]
    fn prop_connect_emits_state_changed(
        host in "[a-z]{3,10}\\.[a-z]{2,4}"
    ) {
        let options = ClientConnectionOptions::new(&host, "/");
        let mut ws = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        ws.connect(now).unwrap();

        let event = ws.poll_event();
        prop_assert!(event.is_some());

        if let Some(ConnectionEvent::StateChanged(state)) = event {
            prop_assert_eq!(state, ConnectionState::Connecting);
        }
    }
}

// =============================================================================
// 出力キューのテスト
// =============================================================================

proptest! {
    /// 複数回の poll_output() は空になるまで値を返す
    #[test]
    fn prop_poll_output_drains_queue(
        host in "[a-z]{3,10}\\.[a-z]{2,4}"
    ) {
        let options = ClientConnectionOptions::new(&host, "/");
        let mut ws = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        ws.connect(now).unwrap();

        // 最初の poll_output は SendData
        let first = ws.poll_output();
        prop_assert!(first.is_some());

        // 2回目以降は何かが返るか None
        let mut count = 1;
        while ws.poll_output().is_some() {
            count += 1;
            if count > 100 {
                break; // 無限ループ防止
            }
        }

        // 最終的には None が返る
        prop_assert!(ws.poll_output().is_none());
    }

    /// イベントキューも同様にドレインされる
    #[test]
    fn prop_poll_event_drains_queue(
        host in "[a-z]{3,10}\\.[a-z]{2,4}"
    ) {
        let options = ClientConnectionOptions::new(&host, "/");
        let mut ws = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        ws.connect(now).unwrap();

        let mut count = 0;
        while ws.poll_event().is_some() {
            count += 1;
            if count > 100 {
                break;
            }
        }

        prop_assert!(ws.poll_event().is_none());
    }
}

// =============================================================================
// 二重接続の防止
// =============================================================================

proptest! {
    /// 既に接続中の場合は connect() がエラー
    #[test]
    fn prop_double_connect_fails(
        host in "[a-z]{3,10}\\.[a-z]{2,4}"
    ) {
        let options = ClientConnectionOptions::new(&host, "/");
        let mut ws = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        // 1回目は成功
        ws.connect(now).unwrap();

        // 2回目はエラー
        let result = ws.connect(now);
        prop_assert!(result.is_err());
    }
}

// =============================================================================
// オプションビルダーの追加テスト
// =============================================================================

proptest! {
    /// ClientConnectionOptions::new() は正しく初期化される
    #[test]
    fn prop_client_options_new(
        host in "[a-z]{3,10}\\.[a-z]{2,4}",
        path in "/[a-zA-Z0-9/_-]{0,30}"
    ) {
        let options = ClientConnectionOptions::new(&host, &path);
        prop_assert_eq!(options.host, host);
        prop_assert_eq!(options.path, path);
    }

    /// 複数のプロトコルを追加
    #[test]
    fn prop_client_options_multiple_protocols(
        protocols in prop::collection::vec("[a-z]{3,10}", 1..5)
    ) {
        let mut options = ClientConnectionOptions::new("example.com", "/");
        for p in &protocols {
            options = options.protocol(p);
        }
        prop_assert_eq!(options.protocols.len(), protocols.len());
    }

    /// 複数のヘッダーを追加
    #[test]
    fn prop_client_options_multiple_headers(
        headers in prop::collection::vec(("[A-Z][a-zA-Z-]{3,15}", "[a-zA-Z0-9 ]{1,30}"), 1..5)
    ) {
        let mut options = ClientConnectionOptions::new("example.com", "/");
        for (name, value) in &headers {
            options = options.header(name, value);
        }
        prop_assert_eq!(options.additional_headers.len(), headers.len());
    }

    /// deflate 設定
    #[test]
    fn prop_client_options_deflate(
        server_bits in 9u8..=15,
        client_bits in 9u8..=15
    ) {
        let config = PerMessageDeflateConfig::new()
            .server_max_window_bits(server_bits)
            .client_max_window_bits(client_bits);
        let options = ClientConnectionOptions::new("example.com", "/").deflate(config);
        prop_assert!(options.deflate_config.is_some());
    }
}

// =============================================================================
// ハンドシェイク完了テスト
// =============================================================================

proptest! {
    /// 有効なハンドシェイクレスポンスで Connected になる
    #[test]
    fn prop_valid_handshake_connects(
        host in "[a-z]{3,10}\\.[a-z]{2,4}"
    ) {
        let options = ClientConnectionOptions::new(&host, "/");
        let (conn, _) = setup_connected_client(options);

        prop_assert_eq!(conn.state(), ConnectionState::Connected);
    }

    /// プロトコル付きハンドシェイク
    #[test]
    fn prop_handshake_with_protocol(
        protocol in "[a-z]{3,15}"
    ) {
        let options = ClientConnectionOptions::new("example.com", "/").protocol(&protocol);
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);
        conn.connect(now).unwrap();

        // nonce を取得
        let output = conn.poll_output().unwrap();
        let nonce = if let ConnectionOutput::SendData(data) = output {
            let s = String::from_utf8_lossy(&data);
            if let Some(pos) = s.find("Sec-WebSocket-Key: ") {
                let start = pos + 19;
                let end = s[start..].find("\r\n").unwrap() + start;
                let key = &s[start..end];
                let decoded = base64::engine::general_purpose::STANDARD.decode(key).unwrap();
                let mut nonce = [0u8; 16];
                nonce.copy_from_slice(&decoded);
                nonce
            } else {
                return Ok(()); // skip
            }
        } else {
            return Ok(());
        };

        let accept = calculate_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             Sec-WebSocket-Protocol: {}\r\n\
             \r\n",
            accept, protocol
        );
        conn.feed_recv_buf(response.as_bytes(), now).unwrap();

        prop_assert_eq!(conn.protocol(), Some(protocol.as_str()));
    }
}

// =============================================================================
// メッセージ送受信テスト
// =============================================================================

proptest! {
    /// Connected 状態でテキストメッセージを送信できる
    #[test]
    fn prop_send_text_message(
        text in "[a-zA-Z0-9 ]{1,100}"
    ) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        let result = conn.send_text(&text, now);
        prop_assert!(result.is_ok());

        // SendData 出力を確認
        let output = conn.poll_output();
        prop_assert!(output.is_some());
        if let Some(ConnectionOutput::SendData(data)) = output {
            // FIN + Text opcode + MASK
            prop_assert_eq!(data[0], 0x81);
            prop_assert!(data[1] & 0x80 != 0); // MASK bit set
        }
    }

    /// Connected 状態でバイナリメッセージを送信できる
    #[test]
    fn prop_send_binary_message(
        data in prop::collection::vec(any::<u8>(), 1..100)
    ) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        let result = conn.send_binary(&data, now);
        prop_assert!(result.is_ok());

        // SendData 出力を確認
        let output = conn.poll_output();
        prop_assert!(output.is_some());
        if let Some(ConnectionOutput::SendData(frame_data)) = output {
            // FIN + Binary opcode + MASK
            prop_assert_eq!(frame_data[0], 0x82);
        }
    }

    /// サーバーからのテキストフレームを受信
    #[test]
    fn prop_receive_text_frame(
        text in "[a-zA-Z0-9]{1,50}"
    ) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        // サーバーからのテキストフレーム（マスクなし）
        let mut frame_data = vec![0x81]; // FIN + Text
        frame_data.push(text.len() as u8);
        frame_data.extend(text.as_bytes());

        conn.feed_recv_buf(&frame_data, now).unwrap();

        // TextMessage イベントを確認
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(msg) = event {
                prop_assert_eq!(msg, text);
                found = true;
                break;
            }
        }
        prop_assert!(found);
    }

    /// サーバーからのバイナリフレームを受信
    #[test]
    fn prop_receive_binary_frame(
        data in prop::collection::vec(any::<u8>(), 1..50)
    ) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        // サーバーからのバイナリフレーム（マスクなし）
        let mut frame_data = vec![0x82]; // FIN + Binary
        frame_data.push(data.len() as u8);
        frame_data.extend(&data);

        conn.feed_recv_buf(&frame_data, now).unwrap();

        // BinaryMessage イベントを確認
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::BinaryMessage(msg) = event {
                prop_assert_eq!(msg, data);
                found = true;
                break;
            }
        }
        prop_assert!(found);
    }
}

// =============================================================================
// Ping/Pong テスト
// =============================================================================

proptest! {
    /// サーバーからの Ping に自動で Pong を返す
    #[test]
    fn prop_ping_auto_pong(
        ping_data in prop::collection::vec(any::<u8>(), 0..50)
    ) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        // Ping フレーム（マスクなし）
        let mut frame = vec![0x89]; // FIN + Ping
        frame.push(ping_data.len() as u8);
        frame.extend(&ping_data);

        conn.feed_recv_buf(&frame, now).unwrap();

        // Ping イベントを確認
        let mut found_ping = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Ping(data) = event {
                prop_assert_eq!(data, ping_data);
                found_ping = true;
                break;
            }
        }
        prop_assert!(found_ping);

        // Pong 送信を確認
        let mut found_pong = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(data) = output {
                // Pong opcode = 0x8A
                if data[0] == 0x8A {
                    found_pong = true;
                    break;
                }
            }
        }
        prop_assert!(found_pong);
    }

    /// Pong を受信すると awaiting_pong がクリアされる
    #[test]
    fn prop_pong_clears_awaiting(
        pong_data in prop::collection::vec(any::<u8>(), 0..50)
    ) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        // まず Ping を送信
        conn.send_ping(&[], now).unwrap();

        // Pong フレーム（マスクなし）
        let mut frame = vec![0x8A]; // FIN + Pong
        frame.push(pong_data.len() as u8);
        frame.extend(&pong_data);

        conn.feed_recv_buf(&frame, now).unwrap();

        // Pong イベントを確認
        let mut found_pong = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Pong(_) = event {
                found_pong = true;
                break;
            }
        }
        prop_assert!(found_pong);

        // ClearTimer 出力を確認
        let mut found_clear = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::ClearTimer { id: TimerId::PongTimeout } = output {
                found_clear = true;
                break;
            }
        }
        prop_assert!(found_clear);
    }
}

// =============================================================================
// Close テスト
// =============================================================================

proptest! {
    /// close() を呼ぶと Closing 状態になる
    #[test]
    fn prop_close_sends_frame(
        code in 1000u16..4999,
        reason in "[a-zA-Z0-9 ]{0,50}"
    ) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        let close_code = CloseCode::new(code);
        conn.close(close_code, &reason, now).unwrap();

        prop_assert_eq!(conn.state(), ConnectionState::Closing);

        // Close フレーム送信を確認
        let mut found_close = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(data) = output {
                if data[0] == 0x88 { // FIN + Close
                    found_close = true;
                    break;
                }
            }
        }
        prop_assert!(found_close);
    }

    /// サーバーからの Close フレームを受信
    #[test]
    fn prop_close_frame_received(
        code in 1000u16..4999
    ) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        // Close フレーム（マスクなし）
        let frame = vec![
            0x88, // FIN + Close
            0x02, // length = 2
            (code >> 8) as u8,
            (code & 0xFF) as u8,
        ];

        conn.feed_recv_buf(&frame, now).unwrap();

        // Close イベントを確認
        let mut found_close = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Close { code: Some(c), .. } = event {
                prop_assert_eq!(c.as_u16(), code);
                found_close = true;
                break;
            }
        }
        prop_assert!(found_close);

        // Closed 状態になる
        prop_assert_eq!(conn.state(), ConnectionState::Closed);
    }
}

// =============================================================================
// タイマー処理テスト
// =============================================================================

proptest! {
    /// Ping タイマーイベント
    #[test]
    fn prop_ping_timer_event(
        ping_interval in 1000u64..60000
    ) {
        let options = ClientConnectionOptions::new("example.com", "/")
            .ping_interval(ping_interval);
        let (mut conn, now) = setup_connected_client(options);

        // Ping タイマーを処理
        let result = conn.handle_timer(TimerId::Ping, now);
        prop_assert!(result.is_ok());

        // Ping 送信を確認
        let mut found_ping = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(data) = output {
                if data[0] == 0x89 { // FIN + Ping
                    found_ping = true;
                    break;
                }
            }
        }
        prop_assert!(found_ping);
    }

    /// Pong タイムアウトで接続がクローズされる
    #[test]
    fn prop_pong_timeout_closes_connection(_dummy in 0u8..1) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        // Ping を送信して awaiting_pong を true にする
        conn.send_ping(&[], now).unwrap();

        // キューをクリア
        while conn.poll_output().is_some() {}

        // Pong タイムアウトを処理
        conn.handle_timer(TimerId::PongTimeout, now).unwrap();

        // Error イベントを確認
        let mut found_error = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Error(_) = event {
                found_error = true;
                break;
            }
        }
        prop_assert!(found_error);

        // Closing 状態になる
        prop_assert_eq!(conn.state(), ConnectionState::Closing);
    }
}

// =============================================================================
// RSV ビット検証テスト
// =============================================================================

proptest! {
    /// RSV2 ビットが設定されたフレームは拒否される
    #[test]
    fn prop_rsv2_bit_rejected(_dummy in 0u8..1) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        // RSV2 が設定されたフレーム（0xA2 = 0x80 | 0x20 | 0x02）
        let frame = vec![0xA2, 0x01, 0x00];

        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_err());
    }

    /// RSV3 ビットが設定されたフレームは拒否される
    #[test]
    fn prop_rsv3_bit_rejected(_dummy in 0u8..1) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        // RSV3 が設定されたフレーム（0x92 = 0x80 | 0x10 | 0x02）
        let frame = vec![0x92, 0x01, 0x00];

        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_err());
    }

    /// permessage-deflate なしで RSV1 が設定されたフレームは拒否される
    #[test]
    fn prop_rsv1_without_deflate_rejected(_dummy in 0u8..1) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        // RSV1 が設定されたフレーム（0xC2 = 0x80 | 0x40 | 0x02）
        let frame = vec![0xC2, 0x01, 0x00];

        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_err());
    }
}

// =============================================================================
// フラグメント処理テスト
// =============================================================================

proptest! {
    /// フラグメントされたテキストメッセージ
    #[test]
    fn prop_fragmented_text_message(
        part1 in "[a-zA-Z0-9]{1,30}",
        part2 in "[a-zA-Z0-9]{1,30}"
    ) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        // 最初のフレーム（fin=false, opcode=text）
        let mut frame1 = vec![0x01]; // Text, fin=false
        frame1.push(part1.len() as u8);
        frame1.extend(part1.as_bytes());

        // 継続フレーム（fin=true, opcode=continuation）
        let mut frame2 = vec![0x80]; // Continuation, fin=true
        frame2.push(part2.len() as u8);
        frame2.extend(part2.as_bytes());

        conn.feed_recv_buf(&frame1, now).unwrap();
        conn.feed_recv_buf(&frame2, now).unwrap();

        // 完全なメッセージを確認
        let expected = format!("{}{}", part1, part2);
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(msg) = event {
                prop_assert_eq!(msg, expected);
                found = true;
                break;
            }
        }
        prop_assert!(found);
    }
}

// =============================================================================
// エッジケーステスト
// =============================================================================

proptest! {
    /// データをチャンクで分割して送っても正しく処理される
    #[test]
    fn prop_chunked_frame_reception(
        text in "[\\x20-\\x7E]{10,100}",
        chunk_size in 1usize..20
    ) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        // テキストフレームを作成
        let mut frame_data = vec![0x81]; // FIN + Text
        frame_data.push(text.len() as u8);
        frame_data.extend(text.as_bytes());

        // チャンクに分けて送信
        for chunk in frame_data.chunks(chunk_size) {
            conn.feed_recv_buf(chunk, now).unwrap();
        }

        // TextMessage イベントを確認
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(msg) = event {
                prop_assert_eq!(msg, text);
                found = true;
                break;
            }
        }
        prop_assert!(found);
    }

    /// ランダムなバイト列は適切に処理される
    #[test]
    fn prop_random_bytes_handling(
        data in prop::collection::vec(any::<u8>(), 1..100)
    ) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        // ランダムデータをフィード（エラーになる可能性あり）
        let _ = conn.feed_recv_buf(&data, now);
        // パニックしなければ OK
    }

    /// ハンドシェイク中にランダムデータを送っても安全
    #[test]
    fn prop_random_bytes_during_handshake(
        data in prop::collection::vec(any::<u8>(), 1..100)
    ) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        conn.connect(now).unwrap();
        // キューをクリア
        while conn.poll_output().is_some() {}

        // ランダムデータをフィード（エラーになる可能性あり）
        let _ = conn.feed_recv_buf(&data, now);
        // パニックしなければ OK
    }

    /// 継続フレームが最初のフレームなしでエラー
    #[test]
    fn prop_continuation_without_initial_fails(_dummy in 0u8..1) {
        let options = ClientConnectionOptions::new("example.com", "/");
        let (mut conn, now) = setup_connected_client(options);

        // 継続フレームを直接送信
        let frame = vec![0x80, 0x04, b't', b'e', b's', b't'];

        let result = conn.feed_recv_buf(&frame, now);
        prop_assert!(result.is_err());
    }

    /// サポートされていないプロトコルのハンドシェイク
    #[test]
    fn prop_handshake_unsupported_protocol(
        client_protocol in "[a-z]{3,10}",
        server_protocol in "[a-z]{3,10}"
    ) {
        // 異なるプロトコルの場合
        if client_protocol != server_protocol {
            let options = ClientConnectionOptions::new("example.com", "/")
                .protocol(&client_protocol);
            let mut conn = WebSocketClientConnection::new(options);
            let now = Timestamp::from_millis(0);
            conn.connect(now).unwrap();

            // nonce を取得
            let output = conn.poll_output().unwrap();
            let nonce = if let ConnectionOutput::SendData(data) = output {
                let s = String::from_utf8_lossy(&data);
                if let Some(pos) = s.find("Sec-WebSocket-Key: ") {
                    let start = pos + 19;
                    let end = s[start..].find("\r\n").unwrap() + start;
                    let key = &s[start..end];
                    let decoded = base64::engine::general_purpose::STANDARD.decode(key).unwrap();
                    let mut nonce = [0u8; 16];
                    nonce.copy_from_slice(&decoded);
                    nonce
                } else {
                    return Ok(());
                }
            } else {
                return Ok(());
            };

            let accept = calculate_accept(&nonce);
            let response = format!(
                "HTTP/1.1 101 Switching Protocols\r\n\
                 Upgrade: websocket\r\n\
                 Connection: Upgrade\r\n\
                 Sec-WebSocket-Accept: {}\r\n\
                 Sec-WebSocket-Protocol: {}\r\n\
                 \r\n",
                accept, server_protocol
            );
            conn.feed_recv_buf(response.as_bytes(), now).unwrap();

            // 接続は成功するが、プロトコルはサーバーのものになる
            prop_assert_eq!(conn.protocol(), Some(server_protocol.as_str()));
        }
    }
}
