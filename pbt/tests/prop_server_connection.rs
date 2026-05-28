//! websocket_server_connection.rs のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - 状態遷移の整合性
//! - フラグメント処理の堅牢性
//! - 不正な入力に対する耐性
//! - タイマー処理の整合性

use base64ct::{Base64, Encoding};
use proptest::prelude::*;
use shiguredo_websocket::{
    CloseCode, ConnectionEvent, ConnectionOutput, ConnectionState, ErrorKind, Frame,
    PerMessageDeflateConfig, ServerConnectionOptions, WebSocketServerConnection,
};

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
    Frame::ping(data.to_vec()).unwrap().encode(mask_key)
}

fn create_masked_pong_frame(data: &[u8], mask_key: [u8; 4]) -> Vec<u8> {
    Frame::pong(data.to_vec()).unwrap().encode(mask_key)
}

fn create_masked_close_frame(code: Option<u16>, reason: &str, mask_key: [u8; 4]) -> Vec<u8> {
    Frame::close(code, reason).unwrap().encode(mask_key)
}

// ==== ハンドシェイク処理のテスト ====

proptest! {
    /// 有効なハンドシェイクリクエストは正しく処理される
    #[test]
    fn prop_valid_handshake_accepted(
        key in prop::array::uniform16(any::<u8>()),
    ) {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request).unwrap();

        // ハンドシェイクリクエストが利用可能になるはず
        prop_assert!(conn.handshake_request().is_some());
        prop_assert_eq!(conn.state(), ConnectionState::Connecting);

        // accept_handshake_auto で受諾
        conn.accept_handshake_auto().unwrap();
        prop_assert_eq!(conn.state(), ConnectionState::Connected);
    }

    /// プロトコル付きのハンドシェイクリクエストが正しく処理される
    #[test]
    fn prop_handshake_with_protocol(
        key in prop::array::uniform16(any::<u8>()),
        protocol in "[a-z]{1,20}",
    ) {
        let mut conn = WebSocketServerConnection::new(
            ServerConnectionOptions::new().protocol(&protocol)
        );
        let request = create_valid_handshake_request(&key, Some(&protocol), None);

        conn.feed_recv_buf(&request).unwrap();
        conn.accept_handshake_auto().unwrap();

        prop_assert_eq!(conn.state(), ConnectionState::Connected);
        prop_assert_eq!(conn.protocol(), Some(protocol.as_str()));
    }

    /// 対応していないプロトコルは選択されない
    #[test]
    fn prop_handshake_unsupported_protocol(
        key in prop::array::uniform16(any::<u8>()),
        client_protocol in "[a-z]{1,20}",
        server_protocol in "[A-Z]{1,20}",
    ) {
        let mut conn = WebSocketServerConnection::new(
            ServerConnectionOptions::new().protocol(&server_protocol)
        );
        let request = create_valid_handshake_request(&key, Some(&client_protocol), None);

        conn.feed_recv_buf(&request).unwrap();
        conn.accept_handshake_auto().unwrap();

        // プロトコルが一致しないので None
        prop_assert!(conn.protocol().is_none());
    }

    /// ハンドシェイクを拒否すると Closed 状態になる
    #[test]
    fn prop_handshake_reject(
        key in prop::array::uniform16(any::<u8>()),
    ) {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request).unwrap();
        conn.reject_handshake(403, "Forbidden", &[]).unwrap();

        prop_assert_eq!(conn.state(), ConnectionState::Closed);
    }

    /// reject_handshake() で渡した任意のヘッダーが出力に含まれる
    ///
    /// RFC 6455 Section 4.4: バージョン不一致時は Sec-WebSocket-Version MUST
    #[test]
    fn prop_handshake_reject_with_headers(
        key in prop::array::uniform16(any::<u8>()),
        header_name in "[A-Za-z][A-Za-z0-9-]{2,15}",
        header_value in "[A-Za-z0-9]{1,20}",
    ) {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request).unwrap();
        conn.reject_handshake(426, "Upgrade Required", &[(header_name.as_str(), header_value.as_str())]).unwrap();

        prop_assert_eq!(conn.state(), ConnectionState::Closed);

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
        prop_assert!(found, "header '{}' must be present in reject response", expected);
    }
}

// ==== 接続状態でのデータ送受信テスト ====

fn setup_connected_server() -> WebSocketServerConnection {
    let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
    let key: [u8; 16] = [0; 16];
    let request = create_valid_handshake_request(&key, None, None);
    conn.feed_recv_buf(&request).unwrap();
    conn.accept_handshake_auto().unwrap();

    // イベントと出力を消費
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    conn
}

proptest! {
    /// テキストメッセージの送信
    #[test]
    fn prop_send_text_message(
        text in ".*",
    ) {
        let mut conn = setup_connected_server();

        let result = conn.send_text(&text);
        prop_assert!(result.is_ok());

        // 出力があるはず
        let output = conn.poll_output();
        prop_assert!(matches!(output, Some(ConnectionOutput::SendData(_))));
    }

    /// バイナリメッセージの送信
    #[test]
    fn prop_send_binary_message(
        data in prop::collection::vec(any::<u8>(), 0..1000),
    ) {
        let mut conn = setup_connected_server();

        let result = conn.send_binary(&data);
        prop_assert!(result.is_ok());

        let output = conn.poll_output();
        prop_assert!(matches!(output, Some(ConnectionOutput::SendData(_))));
    }

    /// テキストフレームの受信
    #[test]
    fn prop_receive_text_frame(
        text in "[\\x20-\\x7E]{0,100}",
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        let frame = create_masked_text_frame(&text, mask_key);
        let result = conn.feed_recv_buf(&frame);
        prop_assert!(result.is_ok());

        // TextMessage イベントが発生するはず
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
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        let frame = create_masked_binary_frame(&data, mask_key);
        let result = conn.feed_recv_buf(&frame);
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

// ==== 未マスクフレームの拒否テスト ====

proptest! {
    /// 未マスクのフレームは拒否される
    #[test]
    fn prop_unmasked_frame_rejected(
        payload in prop::collection::vec(any::<u8>(), 1..50),
    ) {
        let mut conn = setup_connected_server();

        // 未マスクのバイナリフレームを構築
        let mut frame = vec![0x82]; // FIN=1 + Binary opcode
        let len = payload.len() as u8;
        frame.push(len); // MASK=0 + length (no mask bit)
        frame.extend_from_slice(&payload);

        let result = conn.feed_recv_buf(&frame);
        prop_assert!(result.is_err());
    }
}

// ==== Ping/Pong のテスト ====

proptest! {
    /// Ping を受信すると Pong を自動返信する
    #[test]
    fn prop_ping_auto_pong(
        data in prop::collection::vec(any::<u8>(), 0..125),
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        let ping = create_masked_ping_frame(&data, mask_key);
        conn.feed_recv_buf(&ping).unwrap();

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
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        // まず Ping を送信
        conn.send_ping(&[]).unwrap();
        while conn.poll_output().is_some() {}

        // Pong を受信
        let pong = create_masked_pong_frame(&data, mask_key);
        conn.feed_recv_buf(&pong).unwrap();

        // Pong イベントが発生
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::Pong(_) = event {
                found = true;
                break;
            }
        }
        prop_assert!(found, "Pong event not found");
    }
}

// ==== Close のテスト ====

/// 有効な Close コードを生成する strategy
fn valid_close_code_strategy() -> impl Strategy<Value = u16> {
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
        code in valid_close_code_strategy(),
        reason in "[\\x20-\\x7E]{0,50}",
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        let close = create_masked_close_frame(Some(code), &reason, mask_key);
        conn.feed_recv_buf(&close).unwrap();

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
        let mut conn = setup_connected_server();

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

    /// コードなしの Close フレーム
    #[test]
    fn prop_close_without_code(
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        let close = create_masked_close_frame(None, "", mask_key);
        conn.feed_recv_buf(&close).unwrap();

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

// ==== 状態遷移のテスト ====

proptest! {
    /// Disconnected 状態では送信できない
    #[test]
    fn prop_cannot_send_while_disconnected(
        text in ".*",
    ) {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());

        let result = conn.send_text(&text);
        prop_assert!(result.is_err());
    }

    /// Connecting 状態では送信できない
    #[test]
    fn prop_cannot_send_while_connecting(
        key in prop::array::uniform16(any::<u8>()),
        text in ".*",
    ) {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request).unwrap();
        prop_assert_eq!(conn.state(), ConnectionState::Connecting);

        let result = conn.send_text(&text);
        prop_assert!(result.is_err());
    }

    /// Closed 状態ではデータを受信できない
    #[test]
    fn prop_feed_to_closed_connection_fails(
        extra in prop::collection::vec(any::<u8>(), 1..50),
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        // Close フレームを送受信
        let close = create_masked_close_frame(Some(1000), "", mask_key);
        conn.feed_recv_buf(&close).unwrap();

        prop_assert_eq!(conn.state(), ConnectionState::Closed);

        // Closed 状態でデータを送ろうとするとエラー
        let result = conn.feed_recv_buf(&extra);
        prop_assert!(result.is_err());
    }
}

// ==== RSV ビットのテスト ====

proptest! {
    /// RSV2 ビットが立ったフレームは拒否される
    #[test]
    fn prop_rsv2_bit_rejected(
        payload in prop::collection::vec(any::<u8>(), 1..50),
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        // RSV2 ビットが立ったフレーム（0x81 | 0x20 = 0xA1）
        let len = payload.len() as u8;
        let mut frame = vec![0xA1, 0x80 | len];
        frame.extend_from_slice(&mask_key);
        let masked: Vec<u8> = payload.iter().enumerate()
            .map(|(i, &b)| b ^ mask_key[i % 4])
            .collect();
        frame.extend_from_slice(&masked);

        let result = conn.feed_recv_buf(&frame);
        prop_assert!(result.is_err());
    }

    /// RSV3 ビットが立ったフレームは拒否される
    #[test]
    fn prop_rsv3_bit_rejected(
        payload in prop::collection::vec(any::<u8>(), 1..50),
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        // RSV3 ビットが立ったフレーム（0x81 | 0x10 = 0x91）
        let len = payload.len() as u8;
        let mut frame = vec![0x91, 0x80 | len];
        frame.extend_from_slice(&mask_key);
        let masked: Vec<u8> = payload.iter().enumerate()
            .map(|(i, &b)| b ^ mask_key[i % 4])
            .collect();
        frame.extend_from_slice(&masked);

        let result = conn.feed_recv_buf(&frame);
        prop_assert!(result.is_err());
    }

    /// permessage-deflate なしで RSV1 ビットは拒否される
    #[test]
    fn prop_rsv1_without_deflate_rejected(
        payload in prop::collection::vec(any::<u8>(), 1..50),
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        // RSV1 ビットが立ったフレーム（0x81 | 0x40 = 0xC1）
        // permessage-deflate がネゴシエートされていないのでエラー
        let len = payload.len() as u8;
        let mut frame = vec![0xC1, 0x80 | len];
        frame.extend_from_slice(&mask_key);
        let masked: Vec<u8> = payload.iter().enumerate()
            .map(|(i, &b)| b ^ mask_key[i % 4])
            .collect();
        frame.extend_from_slice(&masked);

        let result = conn.feed_recv_buf(&frame);
        prop_assert!(result.is_err());
    }
}

// ==== フラグメント処理のテスト ====

proptest! {
    /// フラグメントされたテキストメッセージが正しく再構築される
    #[test]
    fn prop_fragmented_text_message(
        parts in prop::collection::vec("[\\x20-\\x7E]{1,50}", 2..5),
        mask_keys in prop::collection::vec(prop::array::uniform4(any::<u8>()), 2..5),
    ) {
        let mut conn = setup_connected_server();

        let full_text: String = parts.iter().cloned().collect();

        // 最初のフラグメント（opcode=Text, fin=false）
        let first_frame = {
            let mut frame = Frame::text(&parts[0]);
            frame.fin = false;
            frame.encode(mask_keys[0])
        };
        conn.feed_recv_buf(&first_frame).unwrap();

        // 中間のフラグメント（opcode=Continuation, fin=false）
        for (i, part) in parts.iter().enumerate().take(parts.len() - 1).skip(1) {
            let mask_idx = i.min(mask_keys.len() - 1);
            let mut frame = Frame::new(shiguredo_websocket::Opcode::Continuation, part.as_bytes().to_vec());
            frame.fin = false;
            let encoded = frame.encode(mask_keys[mask_idx]);
            conn.feed_recv_buf(&encoded).unwrap();
        }

        // 最後のフラグメント（opcode=Continuation, fin=true）
        let last_idx = parts.len() - 1;
        let mask_idx = last_idx.min(mask_keys.len() - 1);
        let mut frame = Frame::new(shiguredo_websocket::Opcode::Continuation, parts[last_idx].as_bytes().to_vec());
        frame.fin = true;
        let encoded = frame.encode(mask_keys[mask_idx]);
        conn.feed_recv_buf(&encoded).unwrap();

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
    fn prop_continuation_without_start_fails(
        data in prop::collection::vec(any::<u8>(), 0..100),
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        // 開始フレームなしで Continuation を送る
        let cont = Frame::new(shiguredo_websocket::Opcode::Continuation, data)
            .encode(mask_key);

        let result = conn.feed_recv_buf(&cont);
        prop_assert!(result.is_err());
    }

    /// RFC 6455 Section 5.4: フラグメント中に新しいデータフレームは禁止
    #[test]
    fn prop_new_message_during_fragment_fails(
        first_part in "[\\x20-\\x7E]{1,50}",
        new_message in "[\\x20-\\x7E]{1,50}",
        mask_key1 in prop::array::uniform4(any::<u8>()),
        mask_key2 in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        // フラグメント開始（fin=false）
        let mut frame1 = Frame::text(&first_part);
        frame1.fin = false;
        conn.feed_recv_buf(&frame1.encode(mask_key1))
            .unwrap();

        // 完了前に新しいメッセージ開始
        let frame2 = Frame::text(&new_message).encode(mask_key2);
        let result = conn.feed_recv_buf(&frame2);

        prop_assert!(result.is_err());
    }
}

// ==== 部分的なデータ受信のテスト ====

proptest! {
    /// データを小さなチャンクで分割して送っても正しく処理される
    #[test]
    fn prop_chunked_frame_reception(
        text in "[\\x20-\\x7E]{10,100}",
        mask_key in prop::array::uniform4(any::<u8>()),
        chunk_size in 1usize..10,
    ) {
        let mut conn = setup_connected_server();

        let frame = create_masked_text_frame(&text, mask_key);

        // チャンクに分割して送信
        for chunk in frame.chunks(chunk_size) {
            conn.feed_recv_buf(chunk).unwrap();
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

// ==== deflate 拡張ネゴシエーションのテスト ====

use shiguredo_websocket::ServerHandshakeResponse;

proptest! {
    /// permessage-deflate 拡張がネゴシエートされる
    #[test]
    fn prop_deflate_negotiation(
        key in prop::array::uniform16(any::<u8>()),
    ) {
        let config = PerMessageDeflateConfig::default();
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new().deflate(config));
        let request = create_valid_handshake_request(&key, None, Some("permessage-deflate"));

        conn.feed_recv_buf(&request).unwrap();
        conn.accept_handshake_auto().unwrap();

        prop_assert_eq!(conn.state(), ConnectionState::Connected);
        // 拡張がネゴシエートされている
        prop_assert!(!conn.extensions().is_empty());
    }

    /// クライアントが deflate を要求しない場合はネゴシエートされない
    #[test]
    fn prop_deflate_not_negotiated_without_client_request(
        key in prop::array::uniform16(any::<u8>()),
    ) {
        let config = PerMessageDeflateConfig::default();
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new().deflate(config));
        // 拡張なしのリクエスト
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request).unwrap();
        conn.accept_handshake_auto().unwrap();

        prop_assert_eq!(conn.state(), ConnectionState::Connected);
        // 拡張はネゴシエートされない
        prop_assert!(conn.extensions().is_empty());
    }
}

// ==== 追加ヘッダー設定のテスト ====

proptest! {
    /// 追加ヘッダーがレスポンスに含まれる
    #[test]
    fn prop_additional_headers_in_response(
        key in prop::array::uniform16(any::<u8>()),
    ) {
        let mut conn = WebSocketServerConnection::new(
            ServerConnectionOptions::new()
                .header("X-Custom-Header", "custom-value")
                .header("X-Another", "another-value"),
        );
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request).unwrap();
        conn.accept_handshake_auto().unwrap();

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
        prop_assert!(found_response, "Custom header not found in response");
    }
}

// ==== accept_handshake でのエラーテスト ====

proptest! {
    /// accept_handshake でクライアントが要求していないプロトコルを指定するとエラー
    #[test]
    fn prop_accept_handshake_unsupported_protocol_error(
        key in prop::array::uniform16(any::<u8>()),
    ) {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        // クライアントは "chat" を要求
        let request = create_valid_handshake_request(&key, Some("chat"), None);

        conn.feed_recv_buf(&request).unwrap();

        // サーバーは "other" を返そうとする（クライアントが要求していない）
        let response = ServerHandshakeResponse::new().protocol("other");
        let result = conn.accept_handshake(response);

        prop_assert!(result.is_err());
    }

    /// accept_handshake でクライアントが要求していない拡張を指定するとエラー
    #[test]
    fn prop_accept_handshake_unsupported_extension_error(
        key in prop::array::uniform16(any::<u8>()),
    ) {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        // クライアントは拡張を要求しない
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request).unwrap();

        // サーバーは拡張を返そうとする（クライアントが要求していない）
        let response = ServerHandshakeResponse::new().extension("permessage-deflate");
        let result = conn.accept_handshake(response);

        prop_assert!(result.is_err());
    }
}

// ==== pending_frame_data 処理のテスト ====

proptest! {
    /// ハンドシェイクリクエストの後にフレームデータが続く場合
    #[test]
    fn prop_pending_frame_data_after_handshake(
        key in prop::array::uniform16(any::<u8>()),
        text in "[\\x20-\\x7E]{1,50}",
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        // ハンドシェイクリクエストとフレームを一緒に送る
        let frame = Frame::text(&text).encode(mask_key);
        let mut combined = request;
        combined.extend_from_slice(&frame);

        conn.feed_recv_buf(&combined).unwrap();
        conn.accept_handshake_auto().unwrap();

        // イベントを確認
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(msg) = event {
                prop_assert_eq!(msg, text);
                found = true;
                break;
            }
        }
        prop_assert!(found, "Pending frame data not processed");
    }

    /// ハンドシェイク完了前に追加データが来た場合
    #[test]
    fn prop_additional_data_during_handshake(
        key in prop::array::uniform16(any::<u8>()),
        text in "[\\x20-\\x7E]{1,50}",
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        // ハンドシェイクリクエストを送る
        conn.feed_recv_buf(&request).unwrap();

        // ハンドシェイク完了前に追加データを送る
        let frame = Frame::text(&text).encode(mask_key);
        conn.feed_recv_buf(&frame).unwrap();

        // ハンドシェイクを完了
        conn.accept_handshake_auto().unwrap();

        // 追加データが処理される
        let mut found = false;
        while let Some(event) = conn.poll_event() {
            if let ConnectionEvent::TextMessage(msg) = event {
                prop_assert_eq!(msg, text);
                found = true;
                break;
            }
        }
        prop_assert!(found, "Early frame data not processed");
    }
}

// ==== close() エラーのテスト ====

proptest! {
    /// 既に Closed 状態で close() を呼ぶとエラー
    #[test]
    fn prop_close_on_closed_connection_fails(
        close_code in valid_close_code_strategy(),
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        // Close フレームを受信して Closed 状態にする
        let close = create_masked_close_frame(Some(close_code), "", mask_key);
        conn.feed_recv_buf(&close).unwrap();

        prop_assert_eq!(conn.state(), ConnectionState::Closed);

        // Closed 状態で close() を呼ぶとエラー
        let result = conn.close(CloseCode::NORMAL, "");
        prop_assert!(result.is_err());
    }

    /// Connecting 状態で close() を呼び出すとエラーになる
    ///
    /// RFC 6455 Section 7.1.2: Close フレームは established connection 上でのみ送信可能
    #[test]
    fn prop_close_rejected_in_connecting_state(
        key in prop::array::uniform16(any::<u8>()),
        code in prop::sample::select(vec![
            CloseCode::NORMAL,
            CloseCode::GOING_AWAY,
            CloseCode::PROTOCOL_ERROR,
        ]),
        reason in "[\\x20-\\x7E]{0,50}".prop_map(|s| s.to_string()),
    ) {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);

        conn.feed_recv_buf(&request).unwrap();
        // accept() を呼ばず Connecting 状態のまま
        prop_assert_eq!(conn.state(), ConnectionState::Connecting);

        // close() はエラーになるはず
        let result = conn.close(code, &reason);
        prop_assert!(result.is_err(), "close() should fail in Connecting state");

        prop_assert_eq!(conn.state(), ConnectionState::Connecting);
    }

    /// Disconnected 状態で close() を呼び出すとエラーになる
    ///
    /// RFC 6455 Section 7.1.2: Close フレームは established connection 上でのみ送信可能
    #[test]
    fn prop_close_rejected_in_disconnected_state(
        code in prop::sample::select(vec![
            CloseCode::NORMAL,
            CloseCode::GOING_AWAY,
            CloseCode::PROTOCOL_ERROR,
        ]),
        reason in "[\\x20-\\x7E]{0,50}".prop_map(|s| s.to_string()),
    ) {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        prop_assert_eq!(conn.state(), ConnectionState::Disconnected);

        let result = conn.close(code, &reason);
        prop_assert!(result.is_err(), "close() should fail in Disconnected state");
    }
}

// ==== UTF-8 不正エラーのテスト ====

proptest! {
    /// 不正な UTF-8 バイト列は適切にエラーを返す
    #[test]
    fn prop_invalid_utf8_sequences(
        // 確実に無効な UTF-8 シーケンス: 単独の継続バイト (0x80-0xBF) または不正な開始バイト
        first_byte in prop::sample::select(vec![0x80u8, 0x81, 0xBF, 0xFE, 0xFF]),
        extra_bytes in prop::collection::vec(any::<u8>(), 0..5),
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        let mut invalid_bytes = vec![first_byte];
        invalid_bytes.extend(extra_bytes);

        let mut frame = Frame::new(shiguredo_websocket::Opcode::Text, invalid_bytes);
        frame.fin = true;
        let encoded = frame.encode(mask_key);

        // RFC 6455 準拠: 無効な UTF-8 は即座にエラーを返す
        let result = conn.feed_recv_buf(&encoded);
        prop_assert!(result.is_err(), "Invalid UTF-8 should return error");
    }
}

// ==== accept_handshake 予約済みヘッダー拒否のテスト ====

proptest! {
    /// accept_handshake で予約済みヘッダーを additional_headers に渡すとエラーになる
    ///
    /// RFC 6455 Section 4.2.2: サーバーハンドシェイクの予約済みヘッダーは重複してはならない
    #[test]
    fn prop_accept_handshake_rejects_reserved_header(
        key in prop::array::uniform16(any::<u8>()),
        reserved in prop::sample::select(vec![
            "Upgrade",
            "Connection",
            "Sec-WebSocket-Accept",
            "Sec-WebSocket-Protocol",
            "Sec-WebSocket-Extensions",
        ]),
        value in "[a-zA-Z0-9]{1,20}",
    ) {
        use shiguredo_websocket::ServerHandshakeResponse;

        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_valid_handshake_request(&key, None, None);
        conn.feed_recv_buf(&request).unwrap();

        let response = ServerHandshakeResponse::new().header(reserved, &value);
        let result = conn.accept_handshake(response);

        prop_assert!(result.is_err(), "accept_handshake() should reject reserved header '{}'", reserved);
    }
}

// ==== handle_close で close_sent が既に true の場合のテスト ====

proptest! {
    /// サーバーが先に Close を送った後にクライアントから Close を受信する
    #[test]
    fn prop_close_sent_then_received(
        reason in "[\\x20-\\x7E]{0,50}",
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        // サーバーが先に Close を送信
        conn.close(CloseCode::NORMAL, &reason).unwrap();
        while conn.poll_output().is_some() {}
        while conn.poll_event().is_some() {}

        prop_assert_eq!(conn.state(), ConnectionState::Closing);

        // クライアントから Close を受信
        let close = create_masked_close_frame(Some(1000), "", mask_key);
        conn.feed_recv_buf(&close).unwrap();

        // 状態が Closed に変わる（close_sent が true なので返信は送られない）
        prop_assert_eq!(conn.state(), ConnectionState::Closed);
    }
}

// ==== RFC 6455 Section 7.1.7: failed フラグのテスト ====

proptest! {
    /// エラー後に feed_recv_buf() を呼ぶと即座に Err が返る
    ///
    /// RFC 6455 Section 7.1.7: Fail the WebSocket Connection 後は
    /// データ処理を継続してはならない (MUST NOT)
    #[test]
    fn prop_failed_flag_prevents_reprocessing(
        payload in prop::collection::vec(any::<u8>(), 1..50),
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        // RSV2 ビットが立ったフレームでエラーを発生させる
        let len = payload.len() as u8;
        let mut bad_frame = vec![0xA1, 0x80 | len];
        bad_frame.extend_from_slice(&mask_key);
        let masked: Vec<u8> = payload.iter().enumerate()
            .map(|(i, &b)| b ^ mask_key[i % 4])
            .collect();
        bad_frame.extend_from_slice(&masked);

        // 最初の呼び出しはエラーになる
        let first = conn.feed_recv_buf(&bad_frame);
        prop_assert!(first.is_err());

        // 2 回目以降も即座にエラーになる (failed フラグ)
        let second = conn.feed_recv_buf(&[]);
        prop_assert!(second.is_err());
    }
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

proptest! {
    /// バージョン不一致時に 426 応答と Sec-WebSocket-Version: 13 が送信される
    ///
    /// RFC 6455 Section 4.4: サーバーはサポートするバージョンを
    /// Sec-WebSocket-Version ヘッダーで返さなければならない (MUST)
    #[test]
    fn prop_version_not_supported_sends_426(
        key in prop::array::uniform16(any::<u8>()),
        version in prop::sample::select(vec![0u8, 1, 7, 8, 12, 14, 255]),
    ) {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let request = create_wrong_version_handshake_request(&key, version);

        let result = conn.feed_recv_buf(&request);

        // VersionNotSupported エラーが返る
        prop_assert!(result.is_err());
        let err = result.unwrap_err();
        prop_assert_eq!(err.kind, ErrorKind::VersionNotSupported);

        // 状態は Closed になる
        prop_assert_eq!(conn.state(), ConnectionState::Closed);

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
        prop_assert!(found_426, "426 response not sent for version mismatch");
        prop_assert!(found_version_header, "Sec-WebSocket-Version: 13 header not sent");
    }

    /// バージョン不一致後に feed_recv_buf() を呼ぶと即座に Err が返る
    #[test]
    fn prop_version_not_supported_failed_flag(
        key in prop::array::uniform16(any::<u8>()),
    ) {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        // バージョン 0 を送る（不正）
        let request = create_wrong_version_handshake_request(&key, 0);

        let first = conn.feed_recv_buf(&request);
        prop_assert!(first.is_err());

        // failed フラグが立っているので 2 回目も即座にエラー
        let second = conn.feed_recv_buf(&[]);
        prop_assert!(second.is_err());
    }
}

// ==== Close フレームの接続層検証 ====

proptest! {
    /// RFC 6455 Section 5.5.1: Close ペイロードは 0 または 2 以上でなければならない。
    /// 接続済みサーバーが 1 バイト Close フレームを受信した場合、
    /// `handle_close` 経路で `ProtocolViolation` を返し、Close フレーム (1002) を送出する
    #[test]
    fn prop_close_frame_single_byte_payload_at_application_layer(
        payload_byte in any::<u8>(),
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let mut conn = setup_connected_server();

        let masked_payload = payload_byte ^ mask_key[0];
        let frame = vec![
            0x88,       // FIN=1 + Close opcode
            0x80 | 1,   // MASK=1 + length=1
            mask_key[0], mask_key[1], mask_key[2], mask_key[3],
            masked_payload,
        ];

        let result = conn.feed_recv_buf(&frame);
        prop_assert!(result.is_err(), "1 バイトの Close ペイロードはアプリ層でエラーになるべき");
        let err = result.unwrap_err();
        prop_assert_eq!(err.kind, ErrorKind::ProtocolViolation);

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
                    prop_assert_eq!(code, CloseCode::PROTOCOL_ERROR.as_u16());
                    found_close = true;
                    break;
                }
            }
        }
        prop_assert!(found_close, "1 バイト Close 受信後に応答 Close フレームが送出されていない");
    }
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
    conn.feed_recv_buf(&request).expect("handshake");
    // accept_handshake を呼ばず Connecting 状態のまま追加データを待つ
    conn
}

proptest! {
    /// 合計バイト数が MAX_PENDING_FRAME_DATA ちょうどなら受理される
    /// (ハンドシェイク受理前にバッファされたフレームデータの上限境界)
    #[test]
    fn prop_pending_frame_data_at_exact_limit_is_accepted(
        chunk_size in 1usize..=65_536,
    ) {
        let mut conn = setup_pending_request_server();
        // ハンドシェイクリクエスト直後の pending_frame_data は 0 のはず (残余なし)
        let result = feed_chunks_to_pending(&mut conn, MAX_PENDING_FRAME_DATA, chunk_size);
        prop_assert!(
            result.is_ok(),
            "ちょうど MAX_PENDING_FRAME_DATA は受理されるべき (chunk_size={}, err={:?})",
            chunk_size,
            result.err()
        );
    }

    /// 合計バイト数が MAX_PENDING_FRAME_DATA を 1 バイト超えると ProtocolViolation で拒否される
    #[test]
    fn prop_pending_frame_data_over_limit_by_one_is_rejected(
        chunk_size in 1usize..=65_536,
    ) {
        let mut conn = setup_pending_request_server();
        // 境界ちょうどまで詰める
        feed_chunks_to_pending(&mut conn, MAX_PENDING_FRAME_DATA, chunk_size)
            .expect("境界まで詰めるのは成功するはず");
        // 1 バイト追加で超過させる
        let result = conn.feed_recv_buf(&[0u8]);
        prop_assert!(
            result.is_err(),
            "MAX_PENDING_FRAME_DATA + 1 は拒否されるべき"
        );
        prop_assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::ProtocolViolation
        );
    }
}
