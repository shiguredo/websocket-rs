//! RFC 6455 準拠のプロパティベーステスト
//!
//! WebSocket プロトコル (RFC 6455) の各セクションに対応したテスト
//! このテストが全て通れば RFC 6455 の主要な要件を満たしている

use base64ct::{Base64, Encoding};
use shiguredo_websocket::{
    CloseCode, ErrorKind, Frame, FrameDecoder, HandshakeRequest, HandshakeRequestValidator,
    HandshakeValidator, Opcode, ServerHandshakeResponse,
};

// =============================================================================
// RFC 6455 Section 1.3: Opening Handshake (概要)
// =============================================================================
// ハンドシェイクは HTTP Upgrade リクエストから始まる

/// Section 1.3: サーバーハンドシェイクは 101 Switching Protocols
#[test]
fn prop_section_1_3_server_handshake_is_101() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
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

        assert!(result.is_ok());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// RFC 6455 Section 4.1: Client Requirements (クライアント要件)
// =============================================================================

/// Section 4.1 item 7: Sec-WebSocket-Key は 16 バイトの base64 エンコード
#[test]
fn prop_section_4_1_item_7_websocket_key_format() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
        let request = HandshakeRequest::new("/", "example.com");
        let encoded = request
            .build(nonce)
            .expect("ハンドシェイクリクエストの構築に失敗してはならない");
        let s = String::from_utf8(encoded).expect("UTF-8 への変換に失敗してはならない");

        // Sec-WebSocket-Key を抽出
        let key_line = s
            .lines()
            .find(|l| l.starts_with("Sec-WebSocket-Key:"))
            .expect("Sec-WebSocket-Key ヘッダー行が存在するはず");
        let key = key_line
            .split(':')
            .nth(1)
            .expect("要素が存在するはず")
            .trim();

        // base64 デコードして 16 バイトであることを確認
        let decoded = Base64::decode_vec(key).expect("base64 デコードに失敗してはならない");
        assert_eq!(decoded.len(), 16);
        Ok(())
    })?;
    Ok(())
}

/// Section 4.1 item 10: Sec-WebSocket-Protocol はオプション
#[test]
fn prop_section_4_1_item_10_websocket_protocol_optional() -> noprop::TestResult {
    pbt::run(|ctx| {
        let protocol = pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz", 3, 15);
        let nonce = noprop::sample_bytes::<16>(ctx);
        // プロトコルなし
        let request1 = HandshakeRequest::new("/", "example.com");
        let encoded1 = request1
            .build(nonce)
            .expect("ハンドシェイクリクエストの構築に失敗してはならない");
        let s1 = String::from_utf8(encoded1).expect("UTF-8 への変換に失敗してはならない");
        assert!(!s1.contains("Sec-WebSocket-Protocol"));

        // プロトコルあり
        let request2 = HandshakeRequest::new("/", "example.com").protocol(&protocol);
        let encoded2 = request2
            .build(nonce)
            .expect("ハンドシェイクリクエストの構築に失敗してはならない");
        let s2 = String::from_utf8(encoded2).expect("UTF-8 への変換に失敗してはならない");
        let expected = format!("Sec-WebSocket-Protocol: {}", protocol);
        assert!(s2.contains(&expected));
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// RFC 6455 Section 4.2: Server-Side Requirements (サーバー要件)
// =============================================================================

/// Sec-WebSocket-Accept の計算
fn calculate_accept(nonce: &[u8; 16]) -> String {
    const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let key = Base64::encode_string(nonce.as_slice());
    let combined = format!("{}{}", key, WEBSOCKET_GUID);
    let hash = aws_lc_rs::digest::digest(
        &aws_lc_rs::digest::SHA1_FOR_LEGACY_USE_ONLY,
        combined.as_bytes(),
    );
    Base64::encode_string(hash.as_ref())
}

/// 有効な WebSocket キーを生成
fn generate_valid_ws_key() -> String {
    Base64::encode_string(b"0123456789ABCDEF")
}

/// Section 4.2.1: サーバーは GET メソッドのみ受け入れる
#[test]
fn prop_section_4_2_1_get_method_only() -> noprop::TestResult {
    pbt::run(|ctx| {
        let method = noprop::sample_choice(ctx, &["POST", "PUT", "DELETE", "PATCH", "HEAD"]);
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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Section 4.2.1: サーバーは HTTP/1.1 以上を要求
#[test]
fn prop_section_4_2_1_http_version_required() -> noprop::TestResult {
    pbt::run(|ctx| {
        let version = noprop::sample_choice(ctx, &["HTTP/1.0", "HTTP/0.9"]);
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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Section 4.2.1: Host ヘッダーは必須
#[test]
fn prop_section_4_2_1_host_required() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。入力は結果に使わない。
        let _dummy = noprop::sample_u8_in(ctx, 0u8..1);
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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Section 4.2.1 item 2: Host はサーバーの authority を含む (空・空白のみは拒否)
#[test]
fn prop_section_4_2_1_host_authority_required() -> noprop::TestResult {
    pbt::run(|ctx| {
        let host_value = noprop::sample_choice(ctx, &["", " ", "  ", "\t", " \t "]);
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host:{}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            host_value, key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        let err = result.expect_err("空または空白だけの Host は拒否される");
        assert_eq!(err.kind, ErrorKind::HandshakeRejected);
        Ok(())
    })?;
    Ok(())
}

/// Section 4.2.1 item 2: authority を含む Host は受理される
#[test]
fn prop_section_4_2_1_host_authority_accepts_authority() -> noprop::TestResult {
    pbt::run(|ctx| {
        let host = noprop::sample_choice(
            ctx,
            &[
                "example.com",
                "example.com:8080",
                "127.0.0.1",
                "127.0.0.1:80",
            ],
        );
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: {}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            host, key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        assert!(result.is_ok(), "authority を含む Host は受理される: {host}");
        Ok(())
    })?;
    Ok(())
}

/// Section 4.2.1: Upgrade ヘッダーは "websocket" (大文字小文字無視)
#[test]
fn prop_section_4_2_1_upgrade_case_insensitive() -> noprop::TestResult {
    pbt::run(|ctx| {
        let upgrade =
            noprop::sample_choice(ctx, &["websocket", "WebSocket", "WEBSOCKET", "WeBsOcKeT"]);
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

        assert!(result.is_ok());
        Ok(())
    })?;
    Ok(())
}

/// Section 4.2.1: Connection ヘッダーは "Upgrade" を含む (大文字小文字無視)
#[test]
fn prop_section_4_2_1_connection_case_insensitive() -> noprop::TestResult {
    pbt::run(|ctx| {
        let connection = noprop::sample_choice(ctx, &["Upgrade", "upgrade", "UPGRADE"]);
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

        assert!(result.is_ok());
        Ok(())
    })?;
    Ok(())
}

/// Section 4.2.1: Sec-WebSocket-Key は 16 バイトの base64
#[test]
fn prop_section_4_2_1_websocket_key_16_bytes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
        let key = Base64::encode_string(nonce.as_slice());
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

        assert!(result.is_ok());
        Ok(())
    })?;
    Ok(())
}

/// Section 4.2.1: Sec-WebSocket-Version は 13
#[test]
fn prop_section_4_2_1_version_must_be_13() -> noprop::TestResult {
    pbt::run(|ctx| {
        let version = noprop::sample_choice(ctx, &["8", "9", "10", "11", "12", "14", "15"]);
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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Section 4.2.2: Sec-WebSocket-Accept の計算
#[test]
fn prop_section_4_2_2_accept_calculation() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
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

        assert!(result.is_ok());
        Ok(())
    })?;
    Ok(())
}

/// Section 4.2.2: 不正な Sec-WebSocket-Accept は拒否される
#[test]
fn prop_section_4_2_2_invalid_accept_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
        let wrong_nonce = noprop::sample_bytes::<16>(ctx);
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

            assert!(result.is_err());
        }
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// RFC 6455 Section 5.2: Base Framing Protocol (フレーム構造)
// =============================================================================

/// Section 5.2: ペイロード長 0-125 は 1 バイトエンコード
#[test]
fn prop_section_5_2_payload_length_7bit() -> noprop::TestResult {
    pbt::run(|ctx| {
        let len = pbt::sample_len(ctx, 0, 126);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let payload = vec![0u8; len];
        let frame = Frame::binary(payload);
        let encoded = frame.encode(masking_key);

        // 2番目のバイトの下位7ビットがペイロード長
        let encoded_len = encoded[1] & 0x7F;
        assert_eq!(encoded_len as usize, len);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.2: ペイロード長 126-65535 は 16 ビットエンコード (126 を使用)
#[test]
fn prop_section_5_2_payload_length_16bit() -> noprop::TestResult {
    pbt::run(|ctx| {
        let len = pbt::sample_len(ctx, 126, 65536);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let payload = vec![0u8; len];
        let frame = Frame::binary(payload);
        let encoded = frame.encode(masking_key);

        // 2番目のバイトは 126 (+ MASK ビット)
        assert_eq!(encoded[1] & 0x7F, 126);

        // 次の 2 バイトがペイロード長（ビッグエンディアン）
        let encoded_len = u16::from_be_bytes([encoded[2], encoded[3]]) as usize;
        assert_eq!(encoded_len, len);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.2: ペイロード長 65536 以上は 64 ビットエンコード (127 を使用)
#[test]
fn prop_section_5_2_payload_length_64bit() -> noprop::TestResult {
    pbt::run(|ctx| {
        // メモリ節約のため 65536-70000 の範囲でテスト
        let len = pbt::sample_len(ctx, 65536, 70000);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let payload = vec![0u8; len];
        let frame = Frame::binary(payload);
        let encoded = frame.encode(masking_key);

        // 2番目のバイトは 127 (+ MASK ビット)
        assert_eq!(encoded[1] & 0x7F, 127);

        // 次の 8 バイトがペイロード長（ビッグエンディアン）
        let encoded_len = u64::from_be_bytes([
            encoded[2], encoded[3], encoded[4], encoded[5], encoded[6], encoded[7], encoded[8],
            encoded[9],
        ]) as usize;
        assert_eq!(encoded_len, len);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.2: RSV ビットはデフォルトで 0
#[test]
fn prop_section_5_2_rsv_bits_default_zero() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 0, 100);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::binary(payload);
        let encoded = frame.encode(masking_key);

        // RSV1, RSV2, RSV3 は 0
        assert_eq!(encoded[0] & 0x70, 0);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.2: RSV ビットのエンコード
#[test]
fn prop_section_5_2_rsv_bits_encoding() -> noprop::TestResult {
    pbt::run(|ctx| {
        let rsv1 = noprop::sample_bool(ctx);
        let rsv2 = noprop::sample_bool(ctx);
        let rsv3 = noprop::sample_bool(ctx);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let mut frame = Frame::binary(vec![1, 2, 3]);
        frame.rsv1 = rsv1;
        frame.rsv2 = rsv2;
        frame.rsv3 = rsv3;

        let encoded = frame.encode(masking_key);

        assert_eq!((encoded[0] & 0x40) != 0, rsv1);
        assert_eq!((encoded[0] & 0x20) != 0, rsv2);
        assert_eq!((encoded[0] & 0x10) != 0, rsv3);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.2: オペコードは 4 ビット
#[test]
fn prop_section_5_2_opcode_4bits() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 0, 100);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let text_frame = Frame::text(String::from_utf8_lossy(&payload).as_ref());
        let binary_frame = Frame::binary(payload.clone());
        let ping_frame = Frame::ping(if payload.len() > 125 {
            payload[..125].to_vec()
        } else {
            payload.clone()
        })
        .expect("Ping フレームの生成に失敗してはならない");
        let pong_frame = Frame::pong(if payload.len() > 125 {
            payload[..125].to_vec()
        } else {
            payload
        })
        .expect("Pong フレームの生成に失敗してはならない");

        let text_encoded = text_frame.encode(masking_key);
        let binary_encoded = binary_frame.encode(masking_key);
        let ping_encoded = ping_frame.encode(masking_key);
        let pong_encoded = pong_frame.encode(masking_key);

        assert_eq!(text_encoded[0] & 0x0F, 0x01); // Text
        assert_eq!(binary_encoded[0] & 0x0F, 0x02); // Binary
        assert_eq!(ping_encoded[0] & 0x0F, 0x09); // Ping
        assert_eq!(pong_encoded[0] & 0x0F, 0x0A); // Pong
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// RFC 6455 Section 5.3: Client-to-Server Masking (マスキング)
// =============================================================================

/// Section 5.3: マスキングは XOR 演算
#[test]
fn prop_section_5_3_masking_xor_operation() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 1, 100);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::binary(payload.clone());
        let encoded = frame.encode(masking_key);

        // ヘッダーサイズを計算
        let header_len = if payload.len() < 126 {
            2
        } else if payload.len() < 65536 {
            4
        } else {
            10
        };
        let payload_start = header_len + 4; // +4 for masking key

        // マスクされたペイロードを検証
        for (i, &original) in payload.iter().enumerate() {
            let masked = encoded[payload_start + i];
            let expected = original ^ masking_key[i % 4];
            assert_eq!(masked, expected);
        }
        Ok(())
    })?;
    Ok(())
}

/// Section 5.3: マスキングキーは 4 バイト
#[test]
fn prop_section_5_3_masking_key_is_4_bytes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 1, 100);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::binary(payload.clone());
        let encoded = frame.encode(masking_key);

        // マスキングキーの位置を計算
        let key_start = if payload.len() < 126 {
            2
        } else if payload.len() < 65536 {
            4
        } else {
            10
        };

        // マスキングキーを抽出して検証
        let extracted_key = &encoded[key_start..key_start + 4];
        assert_eq!(extracted_key, &masking_key);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.3: マスキングは可逆的（同じ操作でアンマスク）
#[test]
fn prop_section_5_3_masking_is_reversible() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 1, 1000);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::binary(payload.clone());
        let encoded = frame.encode(masking_key);

        // デコード
        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");

        assert_eq!(decoded.payload, payload);
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// RFC 6455 Section 5.4: Fragmentation (フラグメンテーション)
// =============================================================================

/// Section 5.4: 非分割メッセージは FIN=1 でオペコード != 0
#[test]
fn prop_section_5_4_unfragmented_message() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 0, 100);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::binary(payload);
        let encoded = frame.encode(masking_key);

        // FIN=1
        assert!((encoded[0] & 0x80) != 0);
        // オペコード != 0 (Binary = 0x02)
        assert!((encoded[0] & 0x0F) != 0);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.4: フラグメント開始は FIN=0 でオペコード != 0
#[test]
fn prop_section_5_4_fragment_start() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 0, 100);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let mut frame = Frame::binary(payload);
        frame.fin = false;
        let encoded = frame.encode(masking_key);

        // FIN=0
        assert!((encoded[0] & 0x80) == 0);
        // オペコード != 0 (Binary = 0x02)
        assert!((encoded[0] & 0x0F) != 0);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.4: 継続フレームはオペコード 0
#[test]
fn prop_section_5_4_continuation_opcode() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 0, 100);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let mut frame = Frame::new(Opcode::Continuation, payload);
        frame.fin = false;
        let encoded = frame.encode(masking_key);

        // オペコード = 0
        assert_eq!(encoded[0] & 0x0F, 0x00);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.4: フラグメント終了は FIN=1 でオペコード 0
#[test]
fn prop_section_5_4_fragment_end() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 0, 100);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::new(Opcode::Continuation, payload);
        // fin はデフォルトで true
        let encoded = frame.encode(masking_key);

        // FIN=1
        assert!((encoded[0] & 0x80) != 0);
        // オペコード = 0
        assert_eq!(encoded[0] & 0x0F, 0x00);
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// RFC 6455 Section 5.5: Control Frames (コントロールフレーム)
// =============================================================================

/// Section 5.5: コントロールフレームのペイロードは 125 バイト以下
#[test]
fn prop_section_5_5_control_frame_max_125_bytes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 125);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let ping = Frame::ping(data.clone()).expect("Ping フレームの生成に失敗してはならない");
        let encoded = ping.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");

        assert!(decoded.payload.len() <= 125);
        assert_eq!(decoded.payload.len(), data.len());
        Ok(())
    })?;
    Ok(())
}

/// Section 5.5: コントロールフレームは FIN=1 (分割禁止)
#[test]
fn prop_section_5_5_control_frames_must_be_final() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 50);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let ping = Frame::ping(data.clone()).expect("Ping フレームの生成に失敗してはならない");
        let pong = Frame::pong(data.clone()).expect("Pong フレームの生成に失敗してはならない");
        let close = Frame::close(Some(1000), "").expect("Close フレームの生成に失敗してはならない");

        let ping_encoded = ping.encode(masking_key);
        let pong_encoded = pong.encode(masking_key);
        let close_encoded = close.encode(masking_key);

        // 全て FIN=1
        assert!((ping_encoded[0] & 0x80) != 0);
        assert!((pong_encoded[0] & 0x80) != 0);
        assert!((close_encoded[0] & 0x80) != 0);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.5.1: Close フレームのオペコードは 0x8
#[test]
fn prop_section_5_5_1_close_opcode() -> noprop::TestResult {
    pbt::run(|ctx| {
        let code = pbt::sample_frame_close_code(ctx);
        let reason = pbt::sample_alphabet(
            ctx,
            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ",
            0,
            50,
        );
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let close =
            Frame::close(Some(code), &reason).expect("Close フレームの生成に失敗してはならない");
        let encoded = close.encode(masking_key);

        assert_eq!(encoded[0] & 0x0F, 0x08);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.5.1: Close フレームのペイロード構造
#[test]
fn prop_section_5_5_1_close_payload_structure() -> noprop::TestResult {
    pbt::run(|ctx| {
        let code = pbt::sample_frame_close_code(ctx);
        let reason = pbt::sample_alphabet(
            ctx,
            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
            0,
            100,
        );
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let close =
            Frame::close(Some(code), &reason).expect("Close フレームの生成に失敗してはならない");
        let encoded = close.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");

        // ペイロードは 2 + reason.len() バイト
        assert_eq!(decoded.payload.len(), 2 + reason.len());

        // 最初の 2 バイトはコード（ビッグエンディアン）
        let parsed_code = u16::from_be_bytes([decoded.payload[0], decoded.payload[1]]);
        assert_eq!(parsed_code, code);

        // 残りは理由 (UTF-8)
        let parsed_reason = String::from_utf8_lossy(&decoded.payload[2..]);
        assert_eq!(parsed_reason.as_ref(), reason);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.5.1: Close フレームはコードなしも可能
#[test]
fn prop_section_5_5_1_close_without_code() -> noprop::TestResult {
    pbt::run(|ctx| {
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let close = Frame::close(None, "").expect("Close フレームの生成に失敗してはならない");
        let encoded = close.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");

        assert!(decoded.payload.is_empty());
        Ok(())
    })?;
    Ok(())
}

/// Section 5.5.2: Ping フレームのオペコードは 0x9
#[test]
fn prop_section_5_5_2_ping_opcode() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 50);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let ping = Frame::ping(data).expect("Ping フレームの生成に失敗してはならない");
        let encoded = ping.encode(masking_key);

        assert_eq!(encoded[0] & 0x0F, 0x09);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.5.3: Pong フレームのオペコードは 0xA
#[test]
fn prop_section_5_5_3_pong_opcode() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 50);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let pong = Frame::pong(data).expect("Pong フレームの生成に失敗してはならない");
        let encoded = pong.encode(masking_key);

        assert_eq!(encoded[0] & 0x0F, 0x0A);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.5: コントロールフレームのペイロードは 125 バイトより大きい場合はエラー
#[test]
fn prop_section_5_5_control_frame_rejects_over_125_bytes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let extra_len = pbt::sample_len(ctx, 1, 100);
        let data = vec![0x42; 125 + extra_len];

        // Ping は 126 バイト以上でエラー
        assert!(Frame::ping(data.clone()).is_err());
        // Pong も同様
        assert!(Frame::pong(data).is_err());
        Ok(())
    })?;
    Ok(())
}

/// Section 5.5.1: Close フレームの理由は 123 バイトより大きい場合はエラー
#[test]
fn prop_section_5_5_1_close_reason_rejects_over_123_bytes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let extra_len = pbt::sample_len(ctx, 1, 50);
        let reason = "a".repeat(123 + extra_len);
        assert!(Frame::close(Some(1000), &reason).is_err());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// RFC 6455 Section 5.6: Data Frames (データフレーム)
// =============================================================================

/// Section 5.6: テキストフレームのオペコードは 0x1
#[test]
fn prop_section_5_6_text_opcode() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_text(ctx, 0, 100);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::text(&text);
        let encoded = frame.encode(masking_key);

        assert_eq!(encoded[0] & 0x0F, 0x01);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.6: バイナリフレームのオペコードは 0x2
#[test]
fn prop_section_5_6_binary_opcode() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 100);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::binary(data);
        let encoded = frame.encode(masking_key);

        assert_eq!(encoded[0] & 0x0F, 0x02);
        Ok(())
    })?;
    Ok(())
}

/// Section 5.6: テキストフレームのペイロードは UTF-8
#[test]
fn prop_section_5_6_text_payload_is_utf8() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_text(ctx, 0, 500);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::text(&text);
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");

        // UTF-8 として有効
        let parsed = String::from_utf8(decoded.payload.clone());
        assert!(parsed.is_ok());
        assert_eq!(parsed.expect("直前の is_ok の確認どおり成功するはず"), text);
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// RFC 6455 Section 7.4: Status Codes (ステータスコード)
// =============================================================================

/// Section 7.4.1: 1000 (NORMAL) は有効なクローズコード
#[test]
fn prop_section_7_4_1_code_1000_normal() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。入力は結果に使わない。
        let _dummy = noprop::sample_u8_in(ctx, 0u8..1);
        let close_code = CloseCode::NORMAL;
        assert!(close_code.is_valid());
        assert!(close_code.is_sendable());
        assert_eq!(close_code.as_u16(), 1000);
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.1: 1001 (GOING_AWAY) は有効なクローズコード
#[test]
fn prop_section_7_4_1_code_1001_going_away() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。入力は結果に使わない。
        let _dummy = noprop::sample_u8_in(ctx, 0u8..1);
        let close_code = CloseCode::GOING_AWAY;
        assert!(close_code.is_valid());
        assert!(close_code.is_sendable());
        assert_eq!(close_code.as_u16(), 1001);
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.1: 1002 (PROTOCOL_ERROR) は有効なクローズコード
#[test]
fn prop_section_7_4_1_code_1002_protocol_error() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。入力は結果に使わない。
        let _dummy = noprop::sample_u8_in(ctx, 0u8..1);
        let close_code = CloseCode::PROTOCOL_ERROR;
        assert!(close_code.is_valid());
        assert!(close_code.is_sendable());
        assert_eq!(close_code.as_u16(), 1002);
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.1: 1003 (UNSUPPORTED_DATA) は有効なクローズコード
#[test]
fn prop_section_7_4_1_code_1003_unsupported_data() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。入力は結果に使わない。
        let _dummy = noprop::sample_u8_in(ctx, 0u8..1);
        let close_code = CloseCode::UNSUPPORTED_DATA;
        assert!(close_code.is_valid());
        assert!(close_code.is_sendable());
        assert_eq!(close_code.as_u16(), 1003);
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.1: 1005 (NO_STATUS_RECEIVED) は送信禁止
#[test]
fn prop_section_7_4_1_code_1005_not_sendable() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。入力は結果に使わない。
        let _dummy = noprop::sample_u8_in(ctx, 0u8..1);
        let close_code = CloseCode::NO_STATUS_RECEIVED;
        assert!(!close_code.is_sendable());
        assert_eq!(close_code.as_u16(), 1005);
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.1: 1006 (ABNORMAL_CLOSURE) は送信禁止
#[test]
fn prop_section_7_4_1_code_1006_not_sendable() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。入力は結果に使わない。
        let _dummy = noprop::sample_u8_in(ctx, 0u8..1);
        let close_code = CloseCode::ABNORMAL_CLOSURE;
        assert!(!close_code.is_sendable());
        assert_eq!(close_code.as_u16(), 1006);
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.1: 1007 (INVALID_PAYLOAD) は有効なクローズコード
#[test]
fn prop_section_7_4_1_code_1007_invalid_payload() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。入力は結果に使わない。
        let _dummy = noprop::sample_u8_in(ctx, 0u8..1);
        let close_code = CloseCode::INVALID_PAYLOAD;
        assert!(close_code.is_valid());
        assert!(close_code.is_sendable());
        assert_eq!(close_code.as_u16(), 1007);
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.1: 1008 (POLICY_VIOLATION) は有効なクローズコード
#[test]
fn prop_section_7_4_1_code_1008_policy_violation() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。入力は結果に使わない。
        let _dummy = noprop::sample_u8_in(ctx, 0u8..1);
        let close_code = CloseCode::POLICY_VIOLATION;
        assert!(close_code.is_valid());
        assert!(close_code.is_sendable());
        assert_eq!(close_code.as_u16(), 1008);
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.1: 1009 (MESSAGE_TOO_BIG) は有効なクローズコード
#[test]
fn prop_section_7_4_1_code_1009_message_too_big() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。入力は結果に使わない。
        let _dummy = noprop::sample_u8_in(ctx, 0u8..1);
        let close_code = CloseCode::MESSAGE_TOO_BIG;
        assert!(close_code.is_valid());
        assert!(close_code.is_sendable());
        assert_eq!(close_code.as_u16(), 1009);
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.1: 1010 (MANDATORY_EXTENSION) は有効なクローズコード
#[test]
fn prop_section_7_4_1_code_1010_mandatory_extension() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。入力は結果に使わない。
        let _dummy = noprop::sample_u8_in(ctx, 0u8..1);
        let close_code = CloseCode::MANDATORY_EXTENSION;
        assert!(close_code.is_valid());
        assert!(close_code.is_sendable());
        assert_eq!(close_code.as_u16(), 1010);
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.1: 1011 (INTERNAL_ERROR) は有効なクローズコード
#[test]
fn prop_section_7_4_1_code_1011_internal_error() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。入力は結果に使わない。
        let _dummy = noprop::sample_u8_in(ctx, 0u8..1);
        let close_code = CloseCode::INTERNAL_ERROR;
        assert!(close_code.is_valid());
        assert!(close_code.is_sendable());
        assert_eq!(close_code.as_u16(), 1011);
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.1: 1015 (TLS_HANDSHAKE) は送信禁止
#[test]
fn prop_section_7_4_1_code_1015_not_sendable() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。入力は結果に使わない。
        let _dummy = noprop::sample_u8_in(ctx, 0u8..1);
        let close_code = CloseCode::TLS_HANDSHAKE;
        assert!(!close_code.is_sendable());
        assert_eq!(close_code.as_u16(), 1015);
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.2: 0-999 は無効なクローズコード
#[test]
fn prop_section_7_4_2_codes_0_999_invalid() -> noprop::TestResult {
    pbt::run(|ctx| {
        let code = noprop::sample_u16_in(ctx, 0u16..1000);
        let close_code = CloseCode::new(code);
        assert!(!close_code.is_valid());
        assert!(!close_code.is_sendable());
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.2: 3000-3999 はライブラリ/フレームワーク用
#[test]
fn prop_section_7_4_2_library_codes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let code = noprop::sample_u16_in(ctx, 3000u16..4000);
        let close_code = CloseCode::new(code);
        assert!(close_code.is_valid());
        assert!(close_code.is_sendable());
        Ok(())
    })?;
    Ok(())
}

/// Section 7.4.2: 4000-4999 はアプリケーション用
#[test]
fn prop_section_7_4_2_application_codes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let code = noprop::sample_u16_in(ctx, 4000u16..5000);
        let close_code = CloseCode::new(code);
        assert!(close_code.is_valid());
        assert!(close_code.is_sendable());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// エンコード/デコードのラウンドトリップテスト
// =============================================================================

/// フレームのエンコード/デコードは可逆的
#[test]
fn prop_frame_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 0, 5000);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::binary(payload.clone());
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");

        assert_eq!(decoded.opcode, Opcode::Binary);
        assert_eq!(decoded.payload, payload);
        assert!(decoded.fin);
        Ok(())
    })?;
    Ok(())
}

/// テキストフレームのラウンドトリップ
#[test]
fn prop_text_frame_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_text(ctx, 0, 1000);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::text(&text);
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");

        assert_eq!(decoded.opcode, Opcode::Text);
        let decoded_text =
            String::from_utf8(decoded.payload).expect("UTF-8 への変換に失敗してはならない");
        assert_eq!(decoded_text, text);
        Ok(())
    })?;
    Ok(())
}

/// 任意のチャンクサイズでフィードしても正しくデコードされる
#[test]
fn prop_decoder_arbitrary_chunks() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload = pbt::sample_byte_vec(ctx, 1, 5000);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let chunk_sizes = pbt::sample_vec(ctx, 1, 50, |ctx| noprop::sample_usize_in(ctx, 1..100));
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

        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");
        assert_eq!(decoded.payload, payload);
        Ok(())
    })?;
    Ok(())
}

/// 連続フレームのデコード
#[test]
fn prop_multiple_frames_decode() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payloads = pbt::sample_vec(ctx, 2, 5, |ctx| pbt::sample_byte_vec(ctx, 1, 100));
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let mut all_encoded = Vec::new();
        for payload in &payloads {
            let frame = Frame::binary(payload.clone());
            all_encoded.extend(frame.encode(masking_key));
        }

        let mut decoder = FrameDecoder::new();
        decoder.feed(&all_encoded);

        for expected_payload in &payloads {
            let decoded = decoder
                .decode()
                .expect("デコードに失敗してはならない")
                .expect("デコード結果が取得できるはず");
            assert_eq!(&decoded.payload, expected_payload);
        }
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// 境界値テスト
// =============================================================================

/// 空のペイロード
#[test]
fn prop_empty_payload() -> noprop::TestResult {
    pbt::run(|ctx| {
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::binary(vec![]);
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");

        assert!(decoded.payload.is_empty());
        Ok(())
    })?;
    Ok(())
}

/// 125 バイト境界 (7-bit → 16-bit 境界)
#[test]
fn prop_payload_boundary_125() -> noprop::TestResult {
    pbt::run(|ctx| {
        let masking_key = noprop::sample_bytes::<4>(ctx);
        for len in [124, 125, 126] {
            let payload = vec![0xAB; len];
            let frame = Frame::binary(payload.clone());
            let encoded = frame.encode(masking_key);

            let mut decoder = FrameDecoder::new();
            decoder.feed(&encoded);
            let decoded = decoder
                .decode()
                .expect("デコードに失敗してはならない")
                .expect("デコード結果が取得できるはず");

            assert_eq!(decoded.payload.len(), len);
        }
        Ok(())
    })?;
    Ok(())
}

/// 65535 バイト境界 (16-bit → 64-bit 境界)
#[test]
fn prop_payload_boundary_65535() -> noprop::TestResult {
    pbt::run(|ctx| {
        let masking_key = noprop::sample_bytes::<4>(ctx);
        for len in [65534, 65535, 65536] {
            let payload = vec![0xCD; len];
            let frame = Frame::binary(payload.clone());
            let encoded = frame.encode(masking_key);

            let mut decoder = FrameDecoder::new();
            decoder.feed(&encoded);
            let decoded = decoder
                .decode()
                .expect("デコードに失敗してはならない")
                .expect("デコード結果が取得できるはず");

            assert_eq!(decoded.payload.len(), len);
        }
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// ハンドシェイクのチャンク送信テスト
// =============================================================================

/// ハンドシェイクリクエストをチャンクで送っても正しくパースされる
#[test]
fn prop_chunked_handshake_request() -> noprop::TestResult {
    pbt::run(|ctx| {
        let chunk_size = pbt::sample_len(ctx, 1, 20);
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

        assert!(result.is_ok());
        Ok(())
    })?;
    Ok(())
}

/// ハンドシェイクレスポンスをチャンクで送っても正しくパースされる
#[test]
fn prop_chunked_handshake_response() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
        let chunk_size = pbt::sample_len(ctx, 1, 20);
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

        assert!(result.is_ok());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// ServerHandshakeResponse のテスト
// =============================================================================

/// ServerHandshakeResponse のビルダーパターン
#[test]
fn prop_server_response_builder() -> noprop::TestResult {
    pbt::run(|ctx| {
        let protocol = pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz", 3, 15);
        let extension = pbt::sample_alphabet(ctx, b"abcdefghijklmnopqrstuvwxyz", 3, 15);
        let header_name = pbt::sample_prefixed(
            ctx,
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZ",
            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ-",
            3,
            15,
        );
        let header_value = pbt::sample_alphabet(
            ctx,
            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ",
            1,
            30,
        );
        let response = ServerHandshakeResponse::new()
            .protocol(&protocol)
            .extension(&extension)
            .header(&header_name, &header_value);

        assert_eq!(response.protocol, Some(protocol));
        assert_eq!(response.extensions, vec![extension]);
        assert_eq!(response.additional_headers.len(), 1);
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// RFC 6455 Section 5.2: 最小表現チェックのテスト
// =============================================================================

/// Section 5.2: 16 ビット表現で 126 未満のペイロード長は拒否される
#[test]
fn prop_section_5_2_reject_non_minimal_16bit() -> noprop::TestResult {
    pbt::run(|ctx| {
        let len = pbt::sample_len(ctx, 0, 126);
        // 手動で不正なフレームを構築: mask=0, length=126 (16-bit), actual=len
        let mut invalid_frame = vec![
            0x82, // FIN + Binary
            0x7E, // 126 = 16-bit length follows
        ];
        invalid_frame.extend_from_slice(&(len as u16).to_be_bytes());
        invalid_frame.extend(vec![0x00; len]);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&invalid_frame);
        let result = decoder.decode();

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Section 5.2: 64 ビット表現で 65535 以下のペイロード長は拒否される
#[test]
fn prop_section_5_2_reject_non_minimal_64bit() -> noprop::TestResult {
    pbt::run(|ctx| {
        let len = pbt::sample_len(ctx, 0, 65536);
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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Section 5.2: 正しい最小表現は受け入れられる
#[test]
fn prop_section_5_2_accept_minimal_encoding() -> noprop::TestResult {
    pbt::run(|ctx| {
        let len = pbt::sample_len(ctx, 0, 1000);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let payload = vec![0xAB; len];
        let frame = Frame::binary(payload.clone());
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let result = decoder.decode();

        assert!(result.is_ok());
        let decoded = result
            .expect("直前の is_ok の確認どおり成功するはず")
            .expect("デコード結果が取得できるはず");
        assert_eq!(decoded.payload.len(), len);
        Ok(())
    })?;
    Ok(())
}
