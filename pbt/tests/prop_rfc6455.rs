//! RFC 6455 準拠のプロパティベーステスト
//!
//! WebSocket プロトコル (RFC 6455) の各セクションに対応したテスト
//! このテストが全て通れば RFC 6455 の主要な要件を満たしている

use base64::Engine;
use proptest::prelude::*;
use sha1::{Digest, Sha1};
use shiguredo_websocket::{
    CloseCode, Frame, FrameDecoder, HandshakeRequest, HandshakeRequestValidator,
    HandshakeValidator, Opcode, ServerHandshakeResponse,
};

// =============================================================================
// RFC 6455 Section 1.3: Opening Handshake (概要)
// =============================================================================
// ハンドシェイクは HTTP Upgrade リクエストから始まる

proptest! {
    /// Section 1.3: サーバーハンドシェイクは 101 Switching Protocols
    #[test]
    fn prop_section_1_3_server_handshake_is_101(nonce in any::<[u8; 16]>()) {
        let accept = calculate_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             \r\n",
            accept
        );

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_ok());
    }
}

// =============================================================================
// RFC 6455 Section 4.1: Client Requirements (クライアント要件)
// =============================================================================

proptest! {
    /// Section 4.1.7: Sec-WebSocket-Key は 16 バイトの base64 エンコード
    #[test]
    fn prop_section_4_1_7_websocket_key_format(nonce in any::<[u8; 16]>()) {
        let request = HandshakeRequest::new("/", "example.com");
        let encoded = request.build(nonce);
        let s = String::from_utf8(encoded).unwrap();

        // Sec-WebSocket-Key を抽出
        let key_line = s.lines()
            .find(|l| l.starts_with("Sec-WebSocket-Key:"))
            .unwrap();
        let key = key_line.split(':').nth(1).unwrap().trim();

        // base64 デコードして 16 バイトであることを確認
        let decoded = base64::engine::general_purpose::STANDARD.decode(key).unwrap();
        prop_assert_eq!(decoded.len(), 16);
    }

    /// Section 4.1.10: Sec-WebSocket-Protocol はオプション
    #[test]
    fn prop_section_4_1_10_websocket_protocol_optional(
        protocol in "[a-z]{3,15}",
        nonce in any::<[u8; 16]>()
    ) {
        // プロトコルなし
        let request1 = HandshakeRequest::new("/", "example.com");
        let encoded1 = request1.build(nonce);
        let s1 = String::from_utf8(encoded1).unwrap();
        prop_assert!(!s1.contains("Sec-WebSocket-Protocol"));

        // プロトコルあり
        let request2 = HandshakeRequest::new("/", "example.com").protocol(&protocol);
        let encoded2 = request2.build(nonce);
        let s2 = String::from_utf8(encoded2).unwrap();
        let expected = format!("Sec-WebSocket-Protocol: {}", protocol);
        prop_assert!(s2.contains(&expected));
    }
}

// =============================================================================
// RFC 6455 Section 4.2: Server-Side Requirements (サーバー要件)
// =============================================================================

/// Sec-WebSocket-Accept の計算
fn calculate_accept(nonce: &[u8; 16]) -> String {
    const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let key = base64::engine::general_purpose::STANDARD.encode(nonce);
    let combined = format!("{}{}", key, WEBSOCKET_GUID);
    let mut hasher = Sha1::new();
    hasher.update(combined.as_bytes());
    let hash = hasher.finalize();
    base64::engine::general_purpose::STANDARD.encode(hash)
}

/// 有効な WebSocket キーを生成
fn generate_valid_ws_key() -> String {
    base64::engine::general_purpose::STANDARD.encode(b"0123456789ABCDEF")
}

proptest! {
    /// Section 4.2.1: サーバーは GET メソッドのみ受け入れる
    #[test]
    fn prop_section_4_2_1_get_method_only(
        method in "(POST|PUT|DELETE|PATCH|HEAD)"
    ) {
        let key = generate_valid_ws_key();
        let request = format!(
            "{} / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            method, key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// Section 4.2.1: サーバーは HTTP/1.1 以上を要求
    #[test]
    fn prop_section_4_2_1_http_version_required(
        version in "(HTTP/1.0|HTTP/0.9)"
    ) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / {}\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            version, key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// Section 4.2.1: Host ヘッダーは必須
    #[test]
    fn prop_section_4_2_1_host_required(_dummy in 0u8..1) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// Section 4.2.1: Upgrade ヘッダーは "websocket" (大文字小文字無視)
    #[test]
    fn prop_section_4_2_1_upgrade_case_insensitive(
        upgrade in "(websocket|WebSocket|WEBSOCKET|WeBsOcKeT)"
    ) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: {}\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            upgrade, key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_ok());
    }

    /// Section 4.2.1: Connection ヘッダーは "Upgrade" を含む (大文字小文字無視)
    #[test]
    fn prop_section_4_2_1_connection_case_insensitive(
        connection in "(Upgrade|upgrade|UPGRADE)"
    ) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: {}\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            connection, key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_ok());
    }

    /// Section 4.2.1: Sec-WebSocket-Key は 16 バイトの base64
    #[test]
    fn prop_section_4_2_1_websocket_key_16_bytes(nonce in any::<[u8; 16]>()) {
        let key = base64::engine::general_purpose::STANDARD.encode(nonce);
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_ok());
    }

    /// Section 4.2.1: Sec-WebSocket-Version は 13
    #[test]
    fn prop_section_4_2_1_version_must_be_13(
        version in "(8|9|10|11|12|14|15)"
    ) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: {}\r\n\
             \r\n",
            key, version
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// Section 4.2.2: Sec-WebSocket-Accept の計算
    #[test]
    fn prop_section_4_2_2_accept_calculation(nonce in any::<[u8; 16]>()) {
        let accept = calculate_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             \r\n",
            accept
        );

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_ok());
    }

    /// Section 4.2.2: 不正な Sec-WebSocket-Accept は拒否される
    #[test]
    fn prop_section_4_2_2_invalid_accept_rejected(
        nonce in any::<[u8; 16]>(),
        wrong_nonce in any::<[u8; 16]>()
    ) {
        // 異なる nonce から計算された accept
        if nonce != wrong_nonce {
            let wrong_accept = calculate_accept(&wrong_nonce);
            let response = format!(
                "HTTP/1.1 101 Switching Protocols\r\n\
                 Upgrade: websocket\r\n\
                 Connection: Upgrade\r\n\
                 Sec-WebSocket-Accept: {}\r\n\
                 \r\n",
                wrong_accept
            );

            let mut validator = HandshakeValidator::new(nonce);
            validator.feed(response.as_bytes());
            let result = validator.validate();

            prop_assert!(result.is_err());
        }
    }
}

// =============================================================================
// RFC 6455 Section 5.2: Base Framing Protocol (フレーム構造)
// =============================================================================

proptest! {
    /// Section 5.2: ペイロード長 0-125 は 1 バイトエンコード
    #[test]
    fn prop_section_5_2_payload_length_7bit(
        len in 0usize..126,
        masking_key in any::<[u8; 4]>()
    ) {
        let payload = vec![0u8; len];
        let frame = Frame::binary(payload);
        let encoded = frame.encode(masking_key);

        // 2番目のバイトの下位7ビットがペイロード長
        let encoded_len = encoded[1] & 0x7F;
        prop_assert_eq!(encoded_len as usize, len);
    }

    /// Section 5.2: ペイロード長 126-65535 は 16 ビットエンコード (126 を使用)
    #[test]
    fn prop_section_5_2_payload_length_16bit(
        len in 126usize..65536,
        masking_key in any::<[u8; 4]>()
    ) {
        let payload = vec![0u8; len];
        let frame = Frame::binary(payload);
        let encoded = frame.encode(masking_key);

        // 2番目のバイトは 126 (+ MASK ビット)
        prop_assert_eq!(encoded[1] & 0x7F, 126);

        // 次の 2 バイトがペイロード長（ビッグエンディアン）
        let encoded_len = u16::from_be_bytes([encoded[2], encoded[3]]) as usize;
        prop_assert_eq!(encoded_len, len);
    }

    /// Section 5.2: ペイロード長 65536 以上は 64 ビットエンコード (127 を使用)
    #[test]
    fn prop_section_5_2_payload_length_64bit(
        // メモリ節約のため 65536-70000 の範囲でテスト
        len in 65536usize..70000,
        masking_key in any::<[u8; 4]>()
    ) {
        let payload = vec![0u8; len];
        let frame = Frame::binary(payload);
        let encoded = frame.encode(masking_key);

        // 2番目のバイトは 127 (+ MASK ビット)
        prop_assert_eq!(encoded[1] & 0x7F, 127);

        // 次の 8 バイトがペイロード長（ビッグエンディアン）
        let encoded_len = u64::from_be_bytes([
            encoded[2], encoded[3], encoded[4], encoded[5],
            encoded[6], encoded[7], encoded[8], encoded[9],
        ]) as usize;
        prop_assert_eq!(encoded_len, len);
    }

    /// Section 5.2: RSV ビットはデフォルトで 0
    #[test]
    fn prop_section_5_2_rsv_bits_default_zero(
        payload in prop::collection::vec(any::<u8>(), 0..100),
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::binary(payload);
        let encoded = frame.encode(masking_key);

        // RSV1, RSV2, RSV3 は 0
        prop_assert_eq!(encoded[0] & 0x70, 0);
    }

    /// Section 5.2: RSV ビットのエンコード
    #[test]
    fn prop_section_5_2_rsv_bits_encoding(
        rsv1 in any::<bool>(),
        rsv2 in any::<bool>(),
        rsv3 in any::<bool>(),
        masking_key in any::<[u8; 4]>()
    ) {
        let mut frame = Frame::binary(vec![1, 2, 3]);
        frame.rsv1 = rsv1;
        frame.rsv2 = rsv2;
        frame.rsv3 = rsv3;

        let encoded = frame.encode(masking_key);

        prop_assert_eq!((encoded[0] & 0x40) != 0, rsv1);
        prop_assert_eq!((encoded[0] & 0x20) != 0, rsv2);
        prop_assert_eq!((encoded[0] & 0x10) != 0, rsv3);
    }

    /// Section 5.2: オペコードは 4 ビット
    #[test]
    fn prop_section_5_2_opcode_4bits(
        payload in prop::collection::vec(any::<u8>(), 0..100),
        masking_key in any::<[u8; 4]>()
    ) {
        let text_frame = Frame::text(String::from_utf8_lossy(&payload).as_ref());
        let binary_frame = Frame::binary(payload.clone());
        let ping_frame = Frame::ping(if payload.len() > 125 { payload[..125].to_vec() } else { payload.clone() }).unwrap();
        let pong_frame = Frame::pong(if payload.len() > 125 { payload[..125].to_vec() } else { payload }).unwrap();

        let text_encoded = text_frame.encode(masking_key);
        let binary_encoded = binary_frame.encode(masking_key);
        let ping_encoded = ping_frame.encode(masking_key);
        let pong_encoded = pong_frame.encode(masking_key);

        prop_assert_eq!(text_encoded[0] & 0x0F, 0x01);   // Text
        prop_assert_eq!(binary_encoded[0] & 0x0F, 0x02); // Binary
        prop_assert_eq!(ping_encoded[0] & 0x0F, 0x09);   // Ping
        prop_assert_eq!(pong_encoded[0] & 0x0F, 0x0A);   // Pong
    }
}

// =============================================================================
// RFC 6455 Section 5.3: Client-to-Server Masking (マスキング)
// =============================================================================

proptest! {
    /// Section 5.3: マスキングは XOR 演算
    #[test]
    fn prop_section_5_3_masking_xor_operation(
        payload in prop::collection::vec(any::<u8>(), 1..100),
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::binary(payload.clone());
        let encoded = frame.encode(masking_key);

        // ヘッダーサイズを計算
        let header_len = if payload.len() < 126 { 2 } else if payload.len() < 65536 { 4 } else { 10 };
        let payload_start = header_len + 4; // +4 for masking key

        // マスクされたペイロードを検証
        for (i, &original) in payload.iter().enumerate() {
            let masked = encoded[payload_start + i];
            let expected = original ^ masking_key[i % 4];
            prop_assert_eq!(masked, expected);
        }
    }

    /// Section 5.3: マスキングキーは 4 バイト
    #[test]
    fn prop_section_5_3_masking_key_is_4_bytes(
        payload in prop::collection::vec(any::<u8>(), 1..100),
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::binary(payload.clone());
        let encoded = frame.encode(masking_key);

        // マスキングキーの位置を計算
        let key_start = if payload.len() < 126 { 2 } else if payload.len() < 65536 { 4 } else { 10 };

        // マスキングキーを抽出して検証
        let extracted_key = &encoded[key_start..key_start + 4];
        prop_assert_eq!(extracted_key, &masking_key);
    }

    /// Section 5.3: マスキングは可逆的（同じ操作でアンマスク）
    #[test]
    fn prop_section_5_3_masking_is_reversible(
        payload in prop::collection::vec(any::<u8>(), 1..1000),
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::binary(payload.clone());
        let encoded = frame.encode(masking_key);

        // デコード
        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        prop_assert_eq!(decoded.payload, payload);
    }
}

// =============================================================================
// RFC 6455 Section 5.4: Fragmentation (フラグメンテーション)
// =============================================================================

proptest! {
    /// Section 5.4: 非分割メッセージは FIN=1 でオペコード != 0
    #[test]
    fn prop_section_5_4_unfragmented_message(
        payload in prop::collection::vec(any::<u8>(), 0..100),
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::binary(payload);
        let encoded = frame.encode(masking_key);

        // FIN=1
        prop_assert!((encoded[0] & 0x80) != 0);
        // オペコード != 0 (Binary = 0x02)
        prop_assert!((encoded[0] & 0x0F) != 0);
    }

    /// Section 5.4: フラグメント開始は FIN=0 でオペコード != 0
    #[test]
    fn prop_section_5_4_fragment_start(
        payload in prop::collection::vec(any::<u8>(), 0..100),
        masking_key in any::<[u8; 4]>()
    ) {
        let mut frame = Frame::binary(payload);
        frame.fin = false;
        let encoded = frame.encode(masking_key);

        // FIN=0
        prop_assert!((encoded[0] & 0x80) == 0);
        // オペコード != 0 (Binary = 0x02)
        prop_assert!((encoded[0] & 0x0F) != 0);
    }

    /// Section 5.4: 継続フレームはオペコード 0
    #[test]
    fn prop_section_5_4_continuation_opcode(
        payload in prop::collection::vec(any::<u8>(), 0..100),
        masking_key in any::<[u8; 4]>()
    ) {
        let mut frame = Frame::new(Opcode::Continuation, payload);
        frame.fin = false;
        let encoded = frame.encode(masking_key);

        // オペコード = 0
        prop_assert_eq!(encoded[0] & 0x0F, 0x00);
    }

    /// Section 5.4: フラグメント終了は FIN=1 でオペコード 0
    #[test]
    fn prop_section_5_4_fragment_end(
        payload in prop::collection::vec(any::<u8>(), 0..100),
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::new(Opcode::Continuation, payload);
        // fin はデフォルトで true
        let encoded = frame.encode(masking_key);

        // FIN=1
        prop_assert!((encoded[0] & 0x80) != 0);
        // オペコード = 0
        prop_assert_eq!(encoded[0] & 0x0F, 0x00);
    }
}

// =============================================================================
// RFC 6455 Section 5.5: Control Frames (コントロールフレーム)
// =============================================================================

proptest! {
    /// Section 5.5: コントロールフレームのペイロードは 125 バイト以下
    #[test]
    fn prop_section_5_5_control_frame_max_125_bytes(
        data in prop::collection::vec(any::<u8>(), 0..125),
        masking_key in any::<[u8; 4]>()
    ) {
        let ping = Frame::ping(data.clone()).unwrap();
        let encoded = ping.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        prop_assert!(decoded.payload.len() <= 125);
        prop_assert_eq!(decoded.payload.len(), data.len());
    }

    /// Section 5.5: コントロールフレームは FIN=1 (分割禁止)
    #[test]
    fn prop_section_5_5_control_frames_must_be_final(
        data in prop::collection::vec(any::<u8>(), 0..50),
        masking_key in any::<[u8; 4]>()
    ) {
        let ping = Frame::ping(data.clone()).unwrap();
        let pong = Frame::pong(data.clone()).unwrap();
        let close = Frame::close(Some(1000), "").unwrap();

        let ping_encoded = ping.encode(masking_key);
        let pong_encoded = pong.encode(masking_key);
        let close_encoded = close.encode(masking_key);

        // 全て FIN=1
        prop_assert!((ping_encoded[0] & 0x80) != 0);
        prop_assert!((pong_encoded[0] & 0x80) != 0);
        prop_assert!((close_encoded[0] & 0x80) != 0);
    }

    /// Section 5.5.1: Close フレームのオペコードは 0x8
    #[test]
    fn prop_section_5_5_1_close_opcode(
        code in prop_oneof![
            1000u16..=1003,
            1007u16..=1011,
            3000u16..5000
        ],
        reason in "[a-zA-Z0-9 ]{0,50}",
        masking_key in any::<[u8; 4]>()
    ) {
        let close = Frame::close(Some(code), &reason).unwrap();
        let encoded = close.encode(masking_key);

        prop_assert_eq!(encoded[0] & 0x0F, 0x08);
    }

    /// Section 5.5.1: Close フレームのペイロード構造
    #[test]
    fn prop_section_5_5_1_close_payload_structure(
        code in prop_oneof![
            1000u16..=1003,
            1007u16..=1011,
            3000u16..5000
        ],
        reason in "[a-zA-Z0-9]{0,100}",
        masking_key in any::<[u8; 4]>()
    ) {
        let close = Frame::close(Some(code), &reason).unwrap();
        let encoded = close.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        // ペイロードは 2 + reason.len() バイト
        prop_assert_eq!(decoded.payload.len(), 2 + reason.len());

        // 最初の 2 バイトはコード（ビッグエンディアン）
        let parsed_code = u16::from_be_bytes([decoded.payload[0], decoded.payload[1]]);
        prop_assert_eq!(parsed_code, code);

        // 残りは理由 (UTF-8)
        let parsed_reason = String::from_utf8_lossy(&decoded.payload[2..]);
        prop_assert_eq!(parsed_reason.as_ref(), reason);
    }

    /// Section 5.5.1: Close フレームはコードなしも可能
    #[test]
    fn prop_section_5_5_1_close_without_code(masking_key in any::<[u8; 4]>()) {
        let close = Frame::close(None, "").unwrap();
        let encoded = close.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        prop_assert!(decoded.payload.is_empty());
    }

    /// Section 5.5.2: Ping フレームのオペコードは 0x9
    #[test]
    fn prop_section_5_5_2_ping_opcode(
        data in prop::collection::vec(any::<u8>(), 0..50),
        masking_key in any::<[u8; 4]>()
    ) {
        let ping = Frame::ping(data).unwrap();
        let encoded = ping.encode(masking_key);

        prop_assert_eq!(encoded[0] & 0x0F, 0x09);
    }

    /// Section 5.5.3: Pong フレームのオペコードは 0xA
    #[test]
    fn prop_section_5_5_3_pong_opcode(
        data in prop::collection::vec(any::<u8>(), 0..50),
        masking_key in any::<[u8; 4]>()
    ) {
        let pong = Frame::pong(data).unwrap();
        let encoded = pong.encode(masking_key);

        prop_assert_eq!(encoded[0] & 0x0F, 0x0A);
    }

    /// Section 5.5: コントロールフレームのペイロードは 125 バイトより大きい場合はエラー
    #[test]
    fn prop_section_5_5_control_frame_rejects_over_125_bytes(
        extra_len in 1usize..100
    ) {
        let data = vec![0x42; 125 + extra_len];

        // Ping は 126 バイト以上でエラー
        prop_assert!(Frame::ping(data.clone()).is_err());
        // Pong も同様
        prop_assert!(Frame::pong(data).is_err());
    }

    /// Section 5.5.1: Close フレームの理由は 123 バイトより大きい場合はエラー
    #[test]
    fn prop_section_5_5_1_close_reason_rejects_over_123_bytes(
        extra_len in 1usize..50
    ) {
        let reason = "a".repeat(123 + extra_len);
        prop_assert!(Frame::close(Some(1000), &reason).is_err());
    }
}

// =============================================================================
// RFC 6455 Section 5.6: Data Frames (データフレーム)
// =============================================================================

proptest! {
    /// Section 5.6: テキストフレームのオペコードは 0x1
    #[test]
    fn prop_section_5_6_text_opcode(
        text in "\\PC{0,100}",
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::text(&text);
        let encoded = frame.encode(masking_key);

        prop_assert_eq!(encoded[0] & 0x0F, 0x01);
    }

    /// Section 5.6: バイナリフレームのオペコードは 0x2
    #[test]
    fn prop_section_5_6_binary_opcode(
        data in prop::collection::vec(any::<u8>(), 0..100),
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::binary(data);
        let encoded = frame.encode(masking_key);

        prop_assert_eq!(encoded[0] & 0x0F, 0x02);
    }

    /// Section 5.6: テキストフレームのペイロードは UTF-8
    #[test]
    fn prop_section_5_6_text_payload_is_utf8(
        text in "\\PC{0,500}",
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::text(&text);
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        // UTF-8 として有効
        let parsed = String::from_utf8(decoded.payload.clone());
        prop_assert!(parsed.is_ok());
        prop_assert_eq!(parsed.unwrap(), text);
    }
}

// =============================================================================
// RFC 6455 Section 7.4: Status Codes (ステータスコード)
// =============================================================================

proptest! {
    /// Section 7.4.1: 1000 (NORMAL) は有効なクローズコード
    #[test]
    fn prop_section_7_4_1_code_1000_normal(_dummy in 0u8..1) {
        let close_code = CloseCode::NORMAL;
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
        prop_assert_eq!(close_code.as_u16(), 1000);
    }

    /// Section 7.4.1: 1001 (GOING_AWAY) は有効なクローズコード
    #[test]
    fn prop_section_7_4_1_code_1001_going_away(_dummy in 0u8..1) {
        let close_code = CloseCode::GOING_AWAY;
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
        prop_assert_eq!(close_code.as_u16(), 1001);
    }

    /// Section 7.4.1: 1002 (PROTOCOL_ERROR) は有効なクローズコード
    #[test]
    fn prop_section_7_4_1_code_1002_protocol_error(_dummy in 0u8..1) {
        let close_code = CloseCode::PROTOCOL_ERROR;
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
        prop_assert_eq!(close_code.as_u16(), 1002);
    }

    /// Section 7.4.1: 1003 (UNSUPPORTED_DATA) は有効なクローズコード
    #[test]
    fn prop_section_7_4_1_code_1003_unsupported_data(_dummy in 0u8..1) {
        let close_code = CloseCode::UNSUPPORTED_DATA;
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
        prop_assert_eq!(close_code.as_u16(), 1003);
    }

    /// Section 7.4.1: 1005 (NO_STATUS_RECEIVED) は送信禁止
    #[test]
    fn prop_section_7_4_1_code_1005_not_sendable(_dummy in 0u8..1) {
        let close_code = CloseCode::NO_STATUS_RECEIVED;
        prop_assert!(!close_code.is_sendable());
        prop_assert_eq!(close_code.as_u16(), 1005);
    }

    /// Section 7.4.1: 1006 (ABNORMAL_CLOSURE) は送信禁止
    #[test]
    fn prop_section_7_4_1_code_1006_not_sendable(_dummy in 0u8..1) {
        let close_code = CloseCode::ABNORMAL_CLOSURE;
        prop_assert!(!close_code.is_sendable());
        prop_assert_eq!(close_code.as_u16(), 1006);
    }

    /// Section 7.4.1: 1007 (INVALID_PAYLOAD) は有効なクローズコード
    #[test]
    fn prop_section_7_4_1_code_1007_invalid_payload(_dummy in 0u8..1) {
        let close_code = CloseCode::INVALID_PAYLOAD;
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
        prop_assert_eq!(close_code.as_u16(), 1007);
    }

    /// Section 7.4.1: 1008 (POLICY_VIOLATION) は有効なクローズコード
    #[test]
    fn prop_section_7_4_1_code_1008_policy_violation(_dummy in 0u8..1) {
        let close_code = CloseCode::POLICY_VIOLATION;
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
        prop_assert_eq!(close_code.as_u16(), 1008);
    }

    /// Section 7.4.1: 1009 (MESSAGE_TOO_BIG) は有効なクローズコード
    #[test]
    fn prop_section_7_4_1_code_1009_message_too_big(_dummy in 0u8..1) {
        let close_code = CloseCode::MESSAGE_TOO_BIG;
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
        prop_assert_eq!(close_code.as_u16(), 1009);
    }

    /// Section 7.4.1: 1010 (MANDATORY_EXTENSION) は有効なクローズコード
    #[test]
    fn prop_section_7_4_1_code_1010_mandatory_extension(_dummy in 0u8..1) {
        let close_code = CloseCode::MANDATORY_EXTENSION;
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
        prop_assert_eq!(close_code.as_u16(), 1010);
    }

    /// Section 7.4.1: 1011 (INTERNAL_ERROR) は有効なクローズコード
    #[test]
    fn prop_section_7_4_1_code_1011_internal_error(_dummy in 0u8..1) {
        let close_code = CloseCode::INTERNAL_ERROR;
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
        prop_assert_eq!(close_code.as_u16(), 1011);
    }

    /// Section 7.4.1: 1015 (TLS_HANDSHAKE) は送信禁止
    #[test]
    fn prop_section_7_4_1_code_1015_not_sendable(_dummy in 0u8..1) {
        let close_code = CloseCode::TLS_HANDSHAKE;
        prop_assert!(!close_code.is_sendable());
        prop_assert_eq!(close_code.as_u16(), 1015);
    }

    /// Section 7.4.2: 0-999 は無効なクローズコード
    #[test]
    fn prop_section_7_4_2_codes_0_999_invalid(code in 0u16..1000) {
        let close_code = CloseCode::new(code);
        prop_assert!(!close_code.is_valid());
        prop_assert!(!close_code.is_sendable());
    }

    /// Section 7.4.2: 3000-3999 はライブラリ/フレームワーク用
    #[test]
    fn prop_section_7_4_2_library_codes(code in 3000u16..4000) {
        let close_code = CloseCode::new(code);
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    /// Section 7.4.2: 4000-4999 はアプリケーション用
    #[test]
    fn prop_section_7_4_2_application_codes(code in 4000u16..5000) {
        let close_code = CloseCode::new(code);
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }
}

// =============================================================================
// エンコード/デコードのラウンドトリップテスト
// =============================================================================

proptest! {
    /// フレームのエンコード/デコードは可逆的
    #[test]
    fn prop_frame_roundtrip(
        payload in prop::collection::vec(any::<u8>(), 0..5000),
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::binary(payload.clone());
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        prop_assert_eq!(decoded.opcode, Opcode::Binary);
        prop_assert_eq!(decoded.payload, payload);
        prop_assert!(decoded.fin);
    }

    /// テキストフレームのラウンドトリップ
    #[test]
    fn prop_text_frame_roundtrip(
        text in "\\PC{0,1000}",
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::text(&text);
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        prop_assert_eq!(decoded.opcode, Opcode::Text);
        let decoded_text = String::from_utf8(decoded.payload).unwrap();
        prop_assert_eq!(decoded_text, text);
    }

    /// 任意のチャンクサイズでフィードしても正しくデコードされる
    #[test]
    fn prop_decoder_arbitrary_chunks(
        payload in prop::collection::vec(any::<u8>(), 1..5000),
        masking_key in any::<[u8; 4]>(),
        chunk_sizes in prop::collection::vec(1usize..100, 1..50)
    ) {
        let frame = Frame::binary(payload.clone());
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        let mut pos = 0;

        for chunk_size in chunk_sizes {
            if pos >= encoded.len() {
                break;
            }
            let end = (pos + chunk_size).min(encoded.len());
            decoder.feed(&encoded[pos..end]);
            pos = end;
        }

        // 残りをフィード
        if pos < encoded.len() {
            decoder.feed(&encoded[pos..]);
        }

        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(decoded.payload, payload);
    }

    /// 連続フレームのデコード
    #[test]
    fn prop_multiple_frames_decode(
        payloads in prop::collection::vec(prop::collection::vec(any::<u8>(), 1..100), 2..5),
        masking_key in any::<[u8; 4]>()
    ) {
        let mut all_encoded = Vec::new();
        for payload in &payloads {
            let frame = Frame::binary(payload.clone());
            all_encoded.extend(frame.encode(masking_key));
        }

        let mut decoder = FrameDecoder::new();
        decoder.feed(&all_encoded);

        for expected_payload in &payloads {
            let decoded = decoder.decode().unwrap().unwrap();
            prop_assert_eq!(&decoded.payload, expected_payload);
        }
    }
}

// =============================================================================
// 境界値テスト
// =============================================================================

proptest! {
    /// 空のペイロード
    #[test]
    fn prop_empty_payload(masking_key in any::<[u8; 4]>()) {
        let frame = Frame::binary(vec![]);
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        prop_assert!(decoded.payload.is_empty());
    }

    /// 125 バイト境界 (7-bit → 16-bit 境界)
    #[test]
    fn prop_payload_boundary_125(masking_key in any::<[u8; 4]>()) {
        for len in [124, 125, 126] {
            let payload = vec![0xAB; len];
            let frame = Frame::binary(payload.clone());
            let encoded = frame.encode(masking_key);

            let mut decoder = FrameDecoder::new();
            decoder.feed(&encoded);
            let decoded = decoder.decode().unwrap().unwrap();

            prop_assert_eq!(decoded.payload.len(), len);
        }
    }

    /// 65535 バイト境界 (16-bit → 64-bit 境界)
    #[test]
    fn prop_payload_boundary_65535(masking_key in any::<[u8; 4]>()) {
        for len in [65534, 65535, 65536] {
            let payload = vec![0xCD; len];
            let frame = Frame::binary(payload.clone());
            let encoded = frame.encode(masking_key);

            let mut decoder = FrameDecoder::new();
            decoder.feed(&encoded);
            let decoded = decoder.decode().unwrap().unwrap();

            prop_assert_eq!(decoded.payload.len(), len);
        }
    }
}

// =============================================================================
// ハンドシェイクのチャンク送信テスト
// =============================================================================

proptest! {
    /// ハンドシェイクリクエストをチャンクで送っても正しくパースされる
    #[test]
    fn prop_chunked_handshake_request(chunk_size in 1usize..20) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            key
        );

        let mut validator = HandshakeRequestValidator::new();
        for chunk in request.as_bytes().chunks(chunk_size) {
            validator.feed(chunk);
        }
        let result = validator.validate();

        prop_assert!(result.is_ok());
    }

    /// ハンドシェイクレスポンスをチャンクで送っても正しくパースされる
    #[test]
    fn prop_chunked_handshake_response(
        nonce in any::<[u8; 16]>(),
        chunk_size in 1usize..20
    ) {
        let accept = calculate_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             \r\n",
            accept
        );

        let mut validator = HandshakeValidator::new(nonce);
        for chunk in response.as_bytes().chunks(chunk_size) {
            validator.feed(chunk);
        }
        let result = validator.validate();

        prop_assert!(result.is_ok());
    }
}

// =============================================================================
// ServerHandshakeResponse のテスト
// =============================================================================

proptest! {
    /// ServerHandshakeResponse のビルダーパターン
    #[test]
    fn prop_server_response_builder(
        protocol in "[a-z]{3,15}",
        extension in "[a-z]{3,15}",
        header_name in "[A-Z][a-zA-Z-]{3,15}",
        header_value in "[a-zA-Z0-9 ]{1,30}"
    ) {
        let response = ServerHandshakeResponse::new()
            .protocol(&protocol)
            .extension(&extension)
            .header(&header_name, &header_value);

        prop_assert_eq!(response.protocol, Some(protocol));
        prop_assert_eq!(response.extensions, vec![extension]);
        prop_assert_eq!(response.additional_headers.len(), 1);
    }
}

// =============================================================================
// RFC 6455 Section 5.2: 最小表現チェックのテスト
// =============================================================================

proptest! {
    /// Section 5.2: 16 ビット表現で 126 未満のペイロード長は拒否される
    #[test]
    fn prop_section_5_2_reject_non_minimal_16bit(
        len in 0usize..126
    ) {
        // 手動で不正なフレームを構築: mask=0, length=126 (16-bit), actual=len
        let mut invalid_frame = vec![
            0x82,       // FIN + Binary
            0x7E,       // 126 = 16-bit length follows
        ];
        invalid_frame.extend_from_slice(&(len as u16).to_be_bytes());
        invalid_frame.extend(vec![0x00; len]);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&invalid_frame);
        let result = decoder.decode();

        prop_assert!(result.is_err());
    }

    /// Section 5.2: 64 ビット表現で 65535 以下のペイロード長は拒否される
    #[test]
    fn prop_section_5_2_reject_non_minimal_64bit(
        len in 0usize..65536
    ) {
        // 手動で不正なフレームを構築: mask=0, length=127 (64-bit), actual=len
        let mut invalid_frame = vec![
            0x82, // FIN + Binary
            0x7F, // 127 = 64-bit length follows
        ];
        invalid_frame.extend_from_slice(&(len as u64).to_be_bytes());
        invalid_frame.extend(vec![0x00; len]);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&invalid_frame);
        let result = decoder.decode();

        prop_assert!(result.is_err());
    }

    /// Section 5.2: 正しい最小表現は受け入れられる
    #[test]
    fn prop_section_5_2_accept_minimal_encoding(
        len in 0usize..1000,
        masking_key in any::<[u8; 4]>()
    ) {
        let payload = vec![0xAB; len];
        let frame = Frame::binary(payload.clone());
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let result = decoder.decode();

        prop_assert!(result.is_ok());
        let decoded = result.unwrap().unwrap();
        prop_assert_eq!(decoded.payload.len(), len);
    }
}
