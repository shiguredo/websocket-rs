//! websocket_server_connection.rs のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - 状態遷移の整合性
//! - フラグメント処理の堅牢性
//! - 不正な入力に対する耐性
//! - タイマー処理の整合性

use base64ct::{Base64, Encoding};
use shiguredo_websocket::{
    CloseCode, ConnectionEvent, ConnectionOutput, ConnectionState, ErrorKind, Frame,
    PerMessageDeflateConfig, ServerConnectionOptions, WebSocketServerConnection,
};

/// 小文字の ASCII アルファベット。
const LOWER_ASCII: &[u8] = b"abcdefghijklmnopqrstuvwxyz";

/// 大文字の ASCII アルファベット。
const UPPER_ASCII: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// 英字。ヘッダー名の先頭に使う。
const ASCII_LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// 英字、数字、ハイフン。ヘッダー名の 2 文字目以降に使う。
const HEADER_NAME_REST: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-";

/// 英数字。
const ASCII_ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// close() に渡す Close コードの候補。
const CLOSE_CODE_CHOICES: [CloseCode; 3] = [
    CloseCode::NORMAL,
    CloseCode::GOING_AWAY,
    CloseCode::PROTOCOL_ERROR,
];

/// 不正な UTF-8 の先頭になり得るバイト。
const INVALID_UTF8_LEAD_BYTES: [u8; 5] = [0x80, 0x81, 0xBF, 0xFE, 0xFF];

/// 予約済みのハンドシェイクヘッダー名。
const RESERVED_HEADER_NAMES: [&str; 5] = [
    "Upgrade",
    "Connection",
    "Sec-WebSocket-Accept",
    "Sec-WebSocket-Protocol",
    "Sec-WebSocket-Extensions",
];

/// 13 以外のバージョン番号。
const UNSUPPORTED_VERSIONS: [u8; 7] = [0, 1, 7, 8, 12, 14, 255];

/// RSV 違反の検査に使う opcode の一覧。
const FRAME_OPCODES: [u8; 5] = [0x01, 0x02, 0x08, 0x09, 0x0A];

/// 制御 opcode (Close, Ping, Pong) の候補。
const CONTROL_OPCODES: [u8; 3] = [0x08, 0x09, 0x0A];

/// 有効なハンドシェイクリクエストを生成
fn create_valid_handshake_request(
    key: &[u8; 16],
    protocol: Option<&str>,
    extensions: Option<&str>,
) -> Vec<u8> {
    let encoded_key = Base64::encode_string(key.as_slice());
    let mut request = format!(
        "GET /websocket HTTP/1.1\r\n\
         Host: example.com\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {}\r\n\
         Sec-WebSocket-Version: 13\r\n",
        encoded_key
    );
    if let Some(p) = protocol {
        request.push_str(&format!("Sec-WebSocket-Protocol: {}\r\n", p));
    }
    if let Some(e) = extensions {
        request.push_str(&format!("Sec-WebSocket-Extensions: {}\r\n", e));
    }
    request.push_str("\r\n");
    request.into_bytes()
}

/// マスク済みフレームを作成
fn create_masked_text_frame(text: &str, mask_key: [u8; 4]) -> Vec<u8> {
    Frame::text(text).encode(mask_key)
}

fn create_masked_binary_frame(data: &[u8], mask_key: [u8; 4]) -> Vec<u8> {
    Frame::binary(data.to_vec()).encode(mask_key)
}

fn create_masked_ping_frame(data: &[u8], mask_key: [u8; 4]) -> Vec<u8> {
    Frame::ping(data.to_vec())
        .expect("Ping フレームの生成に失敗してはならない")
        .encode(mask_key)
}

fn create_masked_pong_frame(data: &[u8], mask_key: [u8; 4]) -> Vec<u8> {
    Frame::pong(data.to_vec())
        .expect("Pong フレームの生成に失敗してはならない")
        .encode(mask_key)
}

fn create_masked_close_frame(code: Option<u16>, reason: &str, mask_key: [u8; 4]) -> Vec<u8> {
    Frame::close(code, reason)
        .expect("Close フレームの生成に失敗してはならない")
        .encode(mask_key)
}

// ==== ハンドシェイク処理のテスト ====

/// 有効なハンドシェイクリクエストは正しく処理される
#[test]
fn prop_valid_handshake_accepted() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");

        // ハンドシェイクリクエストが利用可能になるはず
        assert!(conn.handshake_request().is_some());
        assert_eq!(conn.state(), ConnectionState::Connecting);

        // accept_handshake_auto で受諾
        conn.accept_handshake_auto()
            .expect("ハンドシェイクの受諾に失敗してはならない");
        assert_eq!(conn.state(), ConnectionState::Connected);
        Ok(())
    })?;
    Ok(())
}

/// プロトコル付きのハンドシェイクリクエストが正しく処理される
#[test]
fn prop_handshake_with_protocol() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let protocol = pbt::sample_alphabet(ctx, LOWER_ASCII, 1, 20);
        let mut conn =
            WebSocketServerConnection::new(ServerConnectionOptions::new().protocol(&protocol));
        let request = create_valid_handshake_request(&key, Some(&protocol), None);

        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");
        conn.accept_handshake_auto()
            .expect("ハンドシェイクの受諾に失敗してはならない");

        assert_eq!(conn.state(), ConnectionState::Connected);
        assert_eq!(conn.protocol(), Some(protocol.as_str()));
        Ok(())
    })?;
    Ok(())
}

/// 対応していないプロトコルは選択されない
#[test]
fn prop_handshake_unsupported_protocol() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let client_protocol = pbt::sample_alphabet(ctx, LOWER_ASCII, 1, 20);
        let server_protocol = pbt::sample_alphabet(ctx, UPPER_ASCII, 1, 20);
        let mut conn = WebSocketServerConnection::new(
            ServerConnectionOptions::new().protocol(&server_protocol),
        );
        let request = create_valid_handshake_request(&key, Some(&client_protocol), None);

        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");
        conn.accept_handshake_auto()
            .expect("ハンドシェイクの受諾に失敗してはならない");

        // プロトコルが一致しないので None
        assert!(conn.protocol().is_none());
        Ok(())
    })?;
    Ok(())
}

/// ハンドシェイクを拒否すると Closed 状態になる
#[test]
fn prop_handshake_reject() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");
        conn.reject_handshake(403, "Forbidden", &[])
            .expect("ハンドシェイクの拒否に失敗してはならない");

        assert_eq!(conn.state(), ConnectionState::Closed);
        Ok(())
    })?;
    Ok(())
}

/// reject_handshake() で渡した任意のヘッダーが出力に含まれる
///
/// RFC 6455 Section 4.4: バージョン不一致時は Sec-WebSocket-Version MUST
#[test]
fn prop_handshake_reject_with_headers() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let header_name = pbt::sample_prefixed(ctx, ASCII_LETTERS, HEADER_NAME_REST, 2, 15);
        let header_value = pbt::sample_alphabet(ctx, ASCII_ALNUM, 1, 20);
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");
        conn.reject_handshake(
            426,
            "Upgrade Required",
            &[(header_name.as_str(), header_value.as_str())],
        )
        .expect("文字列が取得できるはず");

        assert_eq!(conn.state(), ConnectionState::Closed);

        // 出力に渡したヘッダーが含まれることを確認
        let expected = format!("{}: {}", header_name, header_value);
        let mut found = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(data) = output {
                let s = String::from_utf8_lossy(&data);
                if s.contains(&expected) {
                    found = true;
                }
            }
        }
        assert!(
            found,
            "header '{}' must be present in reject response",
            expected
        );
        Ok(())
    })?;
    Ok(())
}

// ==== 接続状態でのデータ送受信テスト ====

fn setup_connected_server() -> WebSocketServerConnection {
    let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
    let key: [u8; 16] = [0; 16];
    let request = create_valid_handshake_request(&key, None, None);
    conn.feed_recv_buf(&request)
        .expect("受信データの投入に失敗してはならない");
    conn.accept_handshake_auto()
        .expect("ハンドシェイクの受諾に失敗してはならない");

    // イベントと出力を消費
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    conn
}

/// テキストメッセージの送信
#[test]
fn prop_send_text_message() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_text(ctx, 0, 99);
        let mut conn = setup_connected_server();

        let result = conn.send_text(&text);
        assert!(result.is_ok());

        // 出力があるはず
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
        let mut conn = setup_connected_server();

        let result = conn.send_binary(&data);
        assert!(result.is_ok());

        let output = conn.poll_output();
        assert!(matches!(output, Some(ConnectionOutput::SendData(_))));
        Ok(())
    })?;
    Ok(())
}

/// テキストフレームの受信
#[test]
fn prop_receive_text_frame() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_printable_ascii(ctx, 0, 100);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        let frame = create_masked_text_frame(&text, mask_key);
        let result = conn.feed_recv_buf(&frame);
        assert!(result.is_ok());

        // TextMessage イベントが発生するはず
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
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        let frame = create_masked_binary_frame(&data, mask_key);
        let result = conn.feed_recv_buf(&frame);
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

// ==== 未マスクフレームの拒否テスト ====

/// 未マスクのフレームは拒否される
#[test]
fn prop_unmasked_frame_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 1, 50);
        let mut conn = setup_connected_server();

        // 未マスクのバイナリフレームを構築
        let mut frame = vec![0x82]; // FIN=1 + Binary opcode
        let len = payload.len() as u8;
        frame.push(len); // MASK=0 + length (no mask bit)
        frame.extend_from_slice(&payload);

        let result = conn.feed_recv_buf(&frame);
        assert!(result.is_err());
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
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        let ping = create_masked_ping_frame(&data, mask_key);
        conn.feed_recv_buf(&ping)
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
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        // まず Ping を送信
        conn.send_ping(&[])
            .expect("Ping の送信に失敗してはならない");
        while conn.poll_output().is_some() {}

        // Pong を受信
        let pong = create_masked_pong_frame(&data, mask_key);
        conn.feed_recv_buf(&pong)
            .expect("受信データの投入に失敗してはならない");

        // Pong イベントが発生
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Pong(_) = event {
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

/// Close フレームを受信すると Close イベントが発生する
#[test]
fn prop_close_frame_received() -> noprop::TestResult {
    pbt::run(|ctx| {
        let code = pbt::sample_sendable_close_code(ctx);
        let reason = pbt::sample_printable_ascii(ctx, 0, 50);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        let close = create_masked_close_frame(Some(code), &reason, mask_key);
        conn.feed_recv_buf(&close)
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
        let code = noprop::sample_choice(ctx, &CLOSE_CODE_CHOICES);
        let reason = pbt::sample_printable_ascii(ctx, 0, 50);
        let mut conn = setup_connected_server();

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

/// コードなしの Close フレーム
#[test]
fn prop_close_without_code() -> noprop::TestResult {
    pbt::run(|ctx| {
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        let close = create_masked_close_frame(None, "", mask_key);
        conn.feed_recv_buf(&close)
            .expect("受信データの投入に失敗してはならない");

        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Close { code: None, reason } = event {
                assert!(reason.is_empty());
                found = true;
                break;
            }
        }
        assert!(found, "Close event not found");
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
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());

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
        let key = noprop::sample_bytes::<16>(ctx);
        let text = pbt::sample_text(ctx, 0, 99);
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");
        assert_eq!(conn.state(), ConnectionState::Connecting);

        let result = conn.send_text(&text);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Closed 状態ではデータを受信できない
#[test]
fn prop_feed_to_closed_connection_fails() -> noprop::TestResult {
    pbt::run(|ctx| {
        let extra = pbt::sample_byte_vec(ctx, 1, 50);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        // Close フレームを送受信
        let close = create_masked_close_frame(Some(1000), "", mask_key);
        conn.feed_recv_buf(&close)
            .expect("受信データの投入に失敗してはならない");

        assert_eq!(conn.state(), ConnectionState::Closed);

        // Closed 状態でデータを送ろうとするとエラー
        let result = conn.feed_recv_buf(&extra);
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
        let payload = pbt::sample_byte_vec(ctx, 1, 50);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        // RSV2 ビットが立ったフレーム（0x81 | 0x20 = 0xA1）
        let len = payload.len() as u8;
        let mut frame = vec![0xA1, 0x80 | len];
        frame.extend_from_slice(&mask_key);
        let masked: Vec<u8> = payload
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ mask_key[i % 4])
            .collect();
        frame.extend_from_slice(&masked);

        let result = conn.feed_recv_buf(&frame);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// RSV3 ビットが立ったフレームは拒否される
#[test]
fn prop_rsv3_bit_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 1, 50);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        // RSV3 ビットが立ったフレーム（0x81 | 0x10 = 0x91）
        let len = payload.len() as u8;
        let mut frame = vec![0x91, 0x80 | len];
        frame.extend_from_slice(&mask_key);
        let masked: Vec<u8> = payload
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ mask_key[i % 4])
            .collect();
        frame.extend_from_slice(&masked);

        let result = conn.feed_recv_buf(&frame);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// permessage-deflate なしで RSV1 ビットは拒否される
#[test]
fn prop_rsv1_without_deflate_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 1, 50);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        // RSV1 ビットが立ったフレーム（0x81 | 0x40 = 0xC1）
        // permessage-deflate がネゴシエートされていないのでエラー
        let len = payload.len() as u8;
        let mut frame = vec![0xC1, 0x80 | len];
        frame.extend_from_slice(&mask_key);
        let masked: Vec<u8> = payload
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ mask_key[i % 4])
            .collect();
        frame.extend_from_slice(&masked);

        let result = conn.feed_recv_buf(&frame);
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
        let mask_keys = pbt::sample_vec(ctx, 2, 5, |ctx| noprop::sample_bytes::<4>(ctx));
        let mut conn = setup_connected_server();

        let full_text: String = parts.iter().cloned().collect();

        // 最初のフラグメント（opcode=Text, fin=false）
        let first_frame = {
            let mut frame = Frame::text(&parts[0]);
            frame.fin = false;
            frame.encode(mask_keys[0])
        };
        conn.feed_recv_buf(&first_frame)
            .expect("受信データの投入に失敗してはならない");

        // 中間のフラグメント（opcode=Continuation, fin=false）
        for (i, part) in parts.iter().enumerate().take(parts.len() - 1).skip(1) {
            let mask_idx = i.min(mask_keys.len() - 1);
            let mut frame = Frame::new(
                shiguredo_websocket::Opcode::Continuation,
                part.as_bytes().to_vec(),
            );
            frame.fin = false;
            let encoded = frame.encode(mask_keys[mask_idx]);
            conn.feed_recv_buf(&encoded)
                .expect("受信データの投入に失敗してはならない");
        }

        // 最後のフラグメント（opcode=Continuation, fin=true）
        let last_idx = parts.len() - 1;
        let mask_idx = last_idx.min(mask_keys.len() - 1);
        let mut frame = Frame::new(
            shiguredo_websocket::Opcode::Continuation,
            parts[last_idx].as_bytes().to_vec(),
        );
        frame.fin = true;
        let encoded = frame.encode(mask_keys[mask_idx]);
        conn.feed_recv_buf(&encoded)
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
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        // 開始フレームなしで Continuation を送る
        let cont = Frame::new(shiguredo_websocket::Opcode::Continuation, data).encode(mask_key);

        let result = conn.feed_recv_buf(&cont);
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
        let mask_key1 = noprop::sample_bytes::<4>(ctx);
        let mask_key2 = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        // フラグメント開始（fin=false）
        let mut frame1 = Frame::text(&first_part);
        frame1.fin = false;
        conn.feed_recv_buf(&frame1.encode(mask_key1))
            .expect("エンコードに失敗してはならない");

        // 完了前に新しいメッセージ開始
        let frame2 = Frame::text(&new_message).encode(mask_key2);
        let result = conn.feed_recv_buf(&frame2);

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
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let chunk_size = pbt::sample_len(ctx, 1, 10);
        let mut conn = setup_connected_server();

        let frame = create_masked_text_frame(&text, mask_key);

        // チャンクに分割して送信
        for chunk in frame.chunks(chunk_size) {
            conn.feed_recv_buf(chunk)
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

// ==== deflate 拡張ネゴシエーションのテスト ====

use shiguredo_websocket::ServerHandshakeResponse;

/// permessage-deflate 拡張がネゴシエートされる
#[test]
fn prop_deflate_negotiation() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let config = PerMessageDeflateConfig::default();
        let mut conn =
            WebSocketServerConnection::new(ServerConnectionOptions::new().deflate(config));
        let request = create_valid_handshake_request(&key, None, Some("permessage-deflate"));

        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");
        conn.accept_handshake_auto()
            .expect("ハンドシェイクの受諾に失敗してはならない");

        assert_eq!(conn.state(), ConnectionState::Connected);
        // 拡張がネゴシエートされている
        assert!(!conn.extensions().is_empty());
        Ok(())
    })?;
    Ok(())
}

/// クライアントが deflate を要求しない場合はネゴシエートされない
#[test]
fn prop_deflate_not_negotiated_without_client_request() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let config = PerMessageDeflateConfig::default();
        let mut conn =
            WebSocketServerConnection::new(ServerConnectionOptions::new().deflate(config));
        // 拡張なしのリクエスト
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");
        conn.accept_handshake_auto()
            .expect("ハンドシェイクの受諾に失敗してはならない");

        assert_eq!(conn.state(), ConnectionState::Connected);
        // 拡張はネゴシエートされない
        assert!(conn.extensions().is_empty());
        Ok(())
    })?;
    Ok(())
}

// ==== 追加ヘッダー設定のテスト ====

/// 追加ヘッダーがレスポンスに含まれる
#[test]
fn prop_additional_headers_in_response() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let mut conn = WebSocketServerConnection::new(
            ServerConnectionOptions::new()
                .header("X-Custom-Header", "custom-value")
                .header("X-Another", "another-value"),
        );
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");
        conn.accept_handshake_auto()
            .expect("ハンドシェイクの受諾に失敗してはならない");

        // レスポンスを確認
        let mut found_response = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(data) = output {
                let response = String::from_utf8_lossy(&data);
                if response.contains("X-Custom-Header: custom-value") {
                    found_response = true;
                }
                break;
            }
        }
        assert!(found_response, "Custom header not found in response");
        Ok(())
    })?;
    Ok(())
}

// ==== accept_handshake でのエラーテスト ====

/// accept_handshake でクライアントが要求していないプロトコルを指定するとエラー
#[test]
fn prop_accept_handshake_unsupported_protocol_error() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        // クライアントは "chat" を要求
        let request = create_valid_handshake_request(&key, Some("chat"), None);

        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");

        // サーバーは "other" を返そうとする（クライアントが要求していない）
        let response = ServerHandshakeResponse::new().protocol("other");
        let result = conn.accept_handshake(response);

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// accept_handshake でクライアントが要求していない拡張を指定するとエラー
#[test]
fn prop_accept_handshake_unsupported_extension_error() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        // クライアントは拡張を要求しない
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");

        // サーバーは拡張を返そうとする（クライアントが要求していない）
        let response = ServerHandshakeResponse::new().extension("permessage-deflate");
        let result = conn.accept_handshake(response);

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// ==== pending_frame_data 処理のテスト ====

/// ハンドシェイクリクエストの後にフレームデータが続く場合
#[test]
fn prop_pending_frame_data_after_handshake() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let text = pbt::sample_printable_ascii(ctx, 1, 50);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        // ハンドシェイクリクエストとフレームを一緒に送る
        let frame = Frame::text(&text).encode(mask_key);
        let mut combined = request;
        combined.extend_from_slice(&frame);

        conn.feed_recv_buf(&combined)
            .expect("受信データの投入に失敗してはならない");
        conn.accept_handshake_auto()
            .expect("ハンドシェイクの受諾に失敗してはならない");

        // イベントを確認
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(msg) = event {
                assert_eq!(msg, text);
                found = true;
                break;
            }
        }
        assert!(found, "Pending frame data not processed");
        Ok(())
    })?;
    Ok(())
}

/// ハンドシェイク完了前に追加データが来た場合
#[test]
fn prop_additional_data_during_handshake() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let text = pbt::sample_printable_ascii(ctx, 1, 50);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        // ハンドシェイクリクエストを送る
        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");

        // ハンドシェイク完了前に追加データを送る
        let frame = Frame::text(&text).encode(mask_key);
        conn.feed_recv_buf(&frame)
            .expect("受信データの投入に失敗してはならない");

        // ハンドシェイクを完了
        conn.accept_handshake_auto()
            .expect("ハンドシェイクの受諾に失敗してはならない");

        // 追加データが処理される
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(msg) = event {
                assert_eq!(msg, text);
                found = true;
                break;
            }
        }
        assert!(found, "Early frame data not processed");
        Ok(())
    })?;
    Ok(())
}

// ==== close() エラーのテスト ====

/// 既に Closed 状態で close() を呼ぶとエラー
#[test]
fn prop_close_on_closed_connection_fails() -> noprop::TestResult {
    pbt::run(|ctx| {
        let close_code = pbt::sample_sendable_close_code(ctx);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        // Close フレームを受信して Closed 状態にする
        let close = create_masked_close_frame(Some(close_code), "", mask_key);
        conn.feed_recv_buf(&close)
            .expect("受信データの投入に失敗してはならない");

        assert_eq!(conn.state(), ConnectionState::Closed);

        // Closed 状態で close() を呼ぶとエラー
        let result = conn.close(CloseCode::NORMAL, "");
        assert!(result.is_err());
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
        let key = noprop::sample_bytes::<16>(ctx);
        let code = noprop::sample_choice(ctx, &CLOSE_CODE_CHOICES);
        let reason = pbt::sample_printable_ascii(ctx, 0, 50);
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");
        // accept() を呼ばず Connecting 状態のまま
        assert_eq!(conn.state(), ConnectionState::Connecting);

        // close() はエラーになるはず
        let result = conn.close(code, &reason);
        assert!(result.is_err(), "close() should fail in Connecting state");

        assert_eq!(conn.state(), ConnectionState::Connecting);
        Ok(())
    })?;
    Ok(())
}

/// Disconnected 状態で close() を呼び出すとエラーになる
///
/// RFC 6455 Section 7.1.2: Close フレームは established connection 上でのみ送信可能
#[test]
fn prop_close_rejected_in_disconnected_state() -> noprop::TestResult {
    pbt::run(|ctx| {
        let code = noprop::sample_choice(ctx, &CLOSE_CODE_CHOICES);
        let reason = pbt::sample_printable_ascii(ctx, 0, 50);
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        assert_eq!(conn.state(), ConnectionState::Disconnected);

        let result = conn.close(code, &reason);
        assert!(result.is_err(), "close() should fail in Disconnected state");
        Ok(())
    })?;
    Ok(())
}

// ==== UTF-8 不正エラーのテスト ====

/// 不正な UTF-8 バイト列は適切にエラーを返す
#[test]
fn prop_invalid_utf8_sequences() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 確実に無効な UTF-8 シーケンス: 単独の継続バイト (0x80-0xBF) または不正な開始バイト
        let first_byte = noprop::sample_choice(ctx, &INVALID_UTF8_LEAD_BYTES);
        let extra_bytes = pbt::sample_byte_vec(ctx, 0, 5);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        let mut invalid_bytes = vec![first_byte];
        invalid_bytes.extend(extra_bytes);

        let mut frame = Frame::new(shiguredo_websocket::Opcode::Text, invalid_bytes);
        frame.fin = true;
        let encoded = frame.encode(mask_key);

        // RFC 6455 準拠: 無効な UTF-8 は即座にエラーを返す
        let result = conn.feed_recv_buf(&encoded);
        assert!(result.is_err(), "Invalid UTF-8 should return error");
        Ok(())
    })?;
    Ok(())
}

// ==== accept_handshake 予約済みヘッダー拒否のテスト ====

/// accept_handshake で予約済みヘッダーを additional_headers に渡すとエラーになる
///
/// RFC 6455 Section 4.2.2: サーバーハンドシェイクの予約済みヘッダーは重複してはならない
#[test]
fn prop_accept_handshake_rejects_reserved_header() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let reserved = noprop::sample_choice(ctx, &RESERVED_HEADER_NAMES);
        let value = pbt::sample_alphabet(ctx, ASCII_ALNUM, 1, 20);
        use shiguredo_websocket::ServerHandshakeResponse;

        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);
        conn.feed_recv_buf(&request)
            .expect("受信データの投入に失敗してはならない");

        let response = ServerHandshakeResponse::new().header(reserved, &value);
        let result = conn.accept_handshake(response);

        assert!(
            result.is_err(),
            "accept_handshake() should reject reserved header '{}'",
            reserved
        );
        Ok(())
    })?;
    Ok(())
}

// ==== handle_close で close_sent が既に true の場合のテスト ====

/// サーバーが先に Close を送った後にクライアントから Close を受信する
#[test]
fn prop_close_sent_then_received() -> noprop::TestResult {
    pbt::run(|ctx| {
        let reason = pbt::sample_printable_ascii(ctx, 0, 50);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        // サーバーが先に Close を送信
        conn.close(CloseCode::NORMAL, &reason)
            .expect("クローズ処理に失敗してはならない");
        while conn.poll_output().is_some() {}
        while conn.poll_event().is_some() {}

        assert_eq!(conn.state(), ConnectionState::Closing);

        // クライアントから Close を受信
        let close = create_masked_close_frame(Some(1000), "", mask_key);
        conn.feed_recv_buf(&close)
            .expect("受信データの投入に失敗してはならない");

        // 状態が Closed に変わる（close_sent が true なので返信は送られない）
        assert_eq!(conn.state(), ConnectionState::Closed);
        Ok(())
    })?;
    Ok(())
}

// ==== RFC 6455 Section 7.1.7: failed フラグのテスト ====

/// エラー後に feed_recv_buf() を呼ぶと即座に Err が返る
///
/// RFC 6455 Section 7.1.7: Fail the WebSocket Connection 後は
/// データ処理を継続してはならない (MUST NOT)
#[test]
fn prop_failed_flag_prevents_reprocessing() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 1, 50);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        // RSV2 ビットが立ったフレームでエラーを発生させる
        let len = payload.len() as u8;
        let mut bad_frame = vec![0xA1, 0x80 | len];
        bad_frame.extend_from_slice(&mask_key);
        let masked: Vec<u8> = payload
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ mask_key[i % 4])
            .collect();
        bad_frame.extend_from_slice(&masked);

        // 最初の呼び出しはエラーになる
        let first = conn.feed_recv_buf(&bad_frame);
        assert!(first.is_err());

        // 2 回目以降も即座にエラーになる (failed フラグ)
        let second = conn.feed_recv_buf(&[]);
        assert!(second.is_err());
        Ok(())
    })?;
    Ok(())
}

// ==== RFC 6455 Section 4.2.2 / 4.4: VersionNotSupported 時の 426 応答テスト ====

/// バージョン不一致のハンドシェイクリクエストを生成
fn create_wrong_version_handshake_request(key: &[u8; 16], version: u8) -> Vec<u8> {
    let encoded_key = Base64::encode_string(key.as_slice());
    format!(
        "GET /websocket HTTP/1.1\r\n\
         Host: example.com\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {}\r\n\
         Sec-WebSocket-Version: {}\r\n\
         \r\n",
        encoded_key, version
    )
    .into_bytes()
}

/// バージョン不一致時に 426 応答と Sec-WebSocket-Version: 13 が送信される
///
/// RFC 6455 Section 4.4: サーバーはサポートするバージョンを
/// Sec-WebSocket-Version ヘッダーで返さなければならない (MUST)
#[test]
fn prop_version_not_supported_sends_426() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let version = noprop::sample_choice(ctx, &UNSUPPORTED_VERSIONS);
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_wrong_version_handshake_request(&key, version);

        let result = conn.feed_recv_buf(&request);

        // VersionNotSupported エラーが返る
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind, ErrorKind::VersionNotSupported);

        // 状態は Closed になる
        assert_eq!(conn.state(), ConnectionState::Closed);

        // 出力に 426 応答が含まれ、Sec-WebSocket-Version: 13 ヘッダーが存在する
        let mut found_426 = false;
        let mut found_version_header = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(data) = output {
                let response = String::from_utf8_lossy(&data);
                if response.contains("426") {
                    found_426 = true;
                }
                if response.contains("Sec-WebSocket-Version: 13") {
                    found_version_header = true;
                }
            }
        }
        assert!(found_426, "426 response not sent for version mismatch");
        assert!(
            found_version_header,
            "Sec-WebSocket-Version: 13 header not sent"
        );
        Ok(())
    })?;
    Ok(())
}

/// バージョン不一致後に feed_recv_buf() を呼ぶと即座に Err が返る
#[test]
fn prop_version_not_supported_failed_flag() -> noprop::TestResult {
    pbt::run(|ctx| {
        let key = noprop::sample_bytes::<16>(ctx);
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        // バージョン 0 を送る（不正）
        let request = create_wrong_version_handshake_request(&key, 0);

        let first = conn.feed_recv_buf(&request);
        assert!(first.is_err());

        // failed フラグが立っているので 2 回目も即座にエラー
        let second = conn.feed_recv_buf(&[]);
        assert!(second.is_err());
        Ok(())
    })?;
    Ok(())
}

// ==== Close フレームの接続層検証 ====

/// RFC 6455 Section 5.5.1: Close ペイロードは 0 または 2 以上でなければならない。
/// 接続済みサーバーが 1 バイト Close フレームを受信した場合、
/// `handle_close` 経路で `ProtocolViolation` を返し、Close フレーム (1002) を送出する
#[test]
fn prop_close_frame_single_byte_payload_at_application_layer() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload_byte = noprop::sample_u8(ctx);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();

        let masked_payload = payload_byte ^ mask_key[0];
        let frame = vec![
            0x88,     // FIN=1 + Close opcode
            0x80 | 1, // MASK=1 + length=1
            mask_key[0],
            mask_key[1],
            mask_key[2],
            mask_key[3],
            masked_payload,
        ];

        let result = conn.feed_recv_buf(&frame);
        assert!(
            result.is_err(),
            "1 バイトの Close ペイロードはアプリ層でエラーになるべき"
        );
        let err = result.unwrap_err();
        assert_eq!(err.kind, ErrorKind::ProtocolViolation);

        // サーバーは応答 Close (1002 Protocol Error) を送出する
        let mut found_close = false;
        while let Some(output) = conn.poll_output() {
            if let ConnectionOutput::SendData(data) = output {
                let mut decoder = shiguredo_websocket::FrameDecoder::new();
                decoder.feed(&data);
                if let Ok(Some(decoded)) = decoder.decode()
                    && decoded.opcode == shiguredo_websocket::Opcode::Close
                    && decoded.payload.len() >= 2
                {
                    let code = u16::from_be_bytes([decoded.payload[0], decoded.payload[1]]);
                    assert_eq!(code, CloseCode::PROTOCOL_ERROR.as_u16());
                    found_close = true;
                    break;
                }
            }
        }
        assert!(
            found_close,
            "1 バイト Close 受信後に応答 Close フレームが送出されていない"
        );
        Ok(())
    })?;
    Ok(())
}

// ==== MAX_PENDING_FRAME_DATA の境界値テスト ====

/// サーバー側 `MAX_PENDING_FRAME_DATA` (`src/websocket_server_connection.rs`) と同じ値。
/// 実装側と同期する必要がある (両方で更新する想定)
const MAX_PENDING_FRAME_DATA: usize = 1024 * 1024;

/// ハンドシェイクリクエスト直後の `pending_frame_data` をちょうど境界まで埋めるヘルパ。
/// `chunk_size` で分割しながら `total` バイトを feed する
fn feed_chunks_to_pending(
    conn: &mut WebSocketServerConnection,
    total: usize,
    chunk_size: usize,
) -> Result<(), shiguredo_websocket::Error> {
    let chunk = vec![0u8; chunk_size];
    let mut sent = 0;
    while sent + chunk_size <= total {
        conn.feed_recv_buf(&chunk)?;
        sent += chunk_size;
    }
    let remaining = total - sent;
    if remaining > 0 {
        conn.feed_recv_buf(&vec![0u8; remaining])?;
    }
    Ok(())
}

/// ハンドシェイクリクエストを送って Connecting 状態にし、`pending_request` を Some にする
fn setup_pending_request_server() -> WebSocketServerConnection {
    let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
    let key: [u8; 16] = [0; 16];
    let request = create_valid_handshake_request(&key, None, None);
    conn.feed_recv_buf(&request)
        .expect("ハンドシェイク応答の投入に失敗してはならない");
    // accept_handshake を呼ばず Connecting 状態のまま追加データを待つ
    conn
}

/// deflate 有効のサーバ接続を Connected 状態まで進めるヘルパ。
/// RSV1=1 が許容される (permessage-deflate が合意済み) 経路の検証に使う
fn setup_connected_server_with_deflate() -> WebSocketServerConnection {
    let options = ServerConnectionOptions::new().deflate(PerMessageDeflateConfig::default());
    let mut conn = WebSocketServerConnection::new(options);
    let key: [u8; 16] = [0; 16];
    let request = create_valid_handshake_request(&key, None, Some("permessage-deflate"));
    conn.feed_recv_buf(&request)
        .expect("ハンドシェイク応答の投入に失敗してはならない");
    conn.accept_handshake_auto()
        .expect("ハンドシェイクの受諾に失敗してはならない");
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}
    conn
}

// ==== RSV ビット違反の検出 ====

/// RFC 6455 Section 5.2: クライアントから RSV2=1 のフレームは ProtocolViolation で拒否される
#[test]
fn prop_rsv2_rejected_at_server() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &FRAME_OPCODES);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();
        // byte 0: FIN=1 (0x80) + RSV2=1 (0x20) + opcode
        // byte 1: MASK=1 (0x80) + length=0
        let frame = vec![
            0x80 | 0x20 | opcode,
            0x80,
            mask_key[0],
            mask_key[1],
            mask_key[2],
            mask_key[3],
        ];
        let result = conn.feed_recv_buf(&frame);
        assert!(result.is_err(), "RSV2=1 は拒否されるべき");
        assert_eq!(result.unwrap_err().kind, ErrorKind::ProtocolViolation);
        Ok(())
    })?;
    Ok(())
}

/// RFC 6455 Section 5.2: クライアントから RSV3=1 のフレームは ProtocolViolation で拒否される
#[test]
fn prop_rsv3_rejected_at_server() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &FRAME_OPCODES);
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server();
        // byte 0: FIN=1 (0x80) + RSV3=1 (0x10) + opcode
        let frame = vec![
            0x80 | 0x10 | opcode,
            0x80,
            mask_key[0],
            mask_key[1],
            mask_key[2],
            mask_key[3],
        ];
        let result = conn.feed_recv_buf(&frame);
        assert!(result.is_err(), "RSV3=1 は拒否されるべき");
        assert_eq!(result.unwrap_err().kind, ErrorKind::ProtocolViolation);
        Ok(())
    })?;
    Ok(())
}

/// RFC 7692 Section 6: permessage-deflate 合意済みでも、制御フレームに RSV1=1 を設定するのは禁止
#[test]
fn prop_rsv1_on_control_frame_rejected_with_deflate() -> noprop::TestResult {
    pbt::run(|ctx| {
        let control_opcode = noprop::sample_choice(ctx, &CONTROL_OPCODES); // Close, Ping, Pong
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server_with_deflate();
        // byte 0: FIN=1 (0x80) + RSV1=1 (0x40) + control opcode
        let frame = vec![
            0x80 | 0x40 | control_opcode,
            0x80,
            mask_key[0],
            mask_key[1],
            mask_key[2],
            mask_key[3],
        ];
        let result = conn.feed_recv_buf(&frame);
        assert!(result.is_err(), "制御フレーム + RSV1=1 は拒否されるべき");
        assert_eq!(result.unwrap_err().kind, ErrorKind::ProtocolViolation);
        Ok(())
    })?;
    Ok(())
}

/// RFC 7692 Section 6: permessage-deflate 合意済みでも、Continuation フレームに RSV1=1 は禁止
/// (RSV1 はメッセージの最初のフレームにのみ設定される)
#[test]
fn prop_rsv1_on_continuation_frame_rejected_with_deflate() -> noprop::TestResult {
    pbt::run(|ctx| {
        let mask_key = noprop::sample_bytes::<4>(ctx);
        let mut conn = setup_connected_server_with_deflate();
        // 先に Text フレーム (RSV1=0, FIN=0) を送って Continuation 待ち状態にする
        let text_frame_first = vec![
            0x01, // FIN=0 + Text opcode
            0x80, // MASK=1 + length=0
            mask_key[0],
            mask_key[1],
            mask_key[2],
            mask_key[3],
        ];
        conn.feed_recv_buf(&text_frame_first)
            .expect("最初のフラグメントは受理されるべき");

        // Continuation + RSV1=1 を送る (違反)
        let continuation_with_rsv1 = vec![
            0x80 | 0x40, // FIN=1 + RSV1=1 + Continuation opcode (0)
            0x80,        // MASK=1 + length=0
            mask_key[0],
            mask_key[1],
            mask_key[2],
            mask_key[3],
        ];
        let result = conn.feed_recv_buf(&continuation_with_rsv1);
        assert!(result.is_err(), "Continuation + RSV1=1 は拒否されるべき");
        assert_eq!(result.unwrap_err().kind, ErrorKind::ProtocolViolation);
        Ok(())
    })?;
    Ok(())
}

/// 合計バイト数が MAX_PENDING_FRAME_DATA ちょうどなら受理される
/// (ハンドシェイク受理前にバッファされたフレームデータの上限境界)
#[test]
fn prop_pending_frame_data_at_exact_limit_is_accepted() -> noprop::TestResult {
    pbt::run(|ctx| {
        let chunk_size = pbt::sample_len_inclusive(ctx, 1, 65_536);
        let mut conn = setup_pending_request_server();
        // ハンドシェイクリクエスト直後の pending_frame_data は 0 のはず (残余なし)
        let result = feed_chunks_to_pending(&mut conn, MAX_PENDING_FRAME_DATA, chunk_size);
        assert!(
            result.is_ok(),
            "ちょうど MAX_PENDING_FRAME_DATA は受理されるべき (chunk_size={}, err={:?})",
            chunk_size,
            result.err()
        );
        Ok(())
    })?;
    Ok(())
}

/// 合計バイト数が MAX_PENDING_FRAME_DATA を 1 バイト超えると ProtocolViolation で拒否される
#[test]
fn prop_pending_frame_data_over_limit_by_one_is_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let chunk_size = pbt::sample_len_inclusive(ctx, 1, 65_536);
        let mut conn = setup_pending_request_server();
        // 境界ちょうどまで詰める
        feed_chunks_to_pending(&mut conn, MAX_PENDING_FRAME_DATA, chunk_size)
            .expect("境界まで詰めるのは成功するはず");
        // 1 バイト追加で超過させる
        let result = conn.feed_recv_buf(&[0u8]);
        assert!(
            result.is_err(),
            "MAX_PENDING_FRAME_DATA + 1 は拒否されるべき"
        );
        assert_eq!(result.unwrap_err().kind, ErrorKind::ProtocolViolation);
        Ok(())
    })?;
    Ok(())
}
