//! RFC 違反検出のプロパティベーステスト
//!
//! 不正なフレームやプロトコル違反を正しく検出できることをテスト

use shiguredo_websocket::FrameDecoder;

const UNKNOWN_OPCODES: &[u8] = &[0x03, 0x04, 0x05, 0x06, 0x07, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F];
const KNOWN_OPCODES: &[u8] = &[0x01, 0x02, 0x08, 0x09, 0x0A];

// =============================================================================
// RFC 6455 Section 5.2: フレーム形式違反
// =============================================================================

/// 不明なオペコード (0x3-0x7, 0xB-0xF) はエラー
#[test]
fn prop_unknown_opcode_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, UNKNOWN_OPCODES);
        let payload_len = noprop::sample_u8_in(ctx, 0..50);
        // 不正なオペコードを持つフレームを手動で構築
        let mut frame = vec![
            0x80 | opcode,      // FIN=1 + opcode
            0x80 | payload_len, // MASK=1 + length
            0x00,
            0x00,
            0x00,
            0x00, // masking key
        ];
        frame.extend(vec![0u8; payload_len as usize]); // payload

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode();
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// コントロールフレームが 125 バイトを超えるとエラー
#[test]
fn prop_control_frame_too_large() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload_len = noprop::sample_u16_in(ctx, 126..500);
        // Ping フレーム (opcode=9) with payload > 125 bytes
        let mut frame = vec![
            0x89,       // FIN=1 + Ping opcode
            0x80 | 126, // MASK=1 + extended length marker
            (payload_len >> 8) as u8,
            (payload_len & 0xFF) as u8, // 16-bit length
            0x00,
            0x00,
            0x00,
            0x00, // masking key
        ];
        frame.extend(vec![0u8; payload_len as usize]);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode();
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// コントロールフレームがフラグメント化されているとエラー (FIN=0)
#[test]
fn prop_fragmented_control_frame() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload_len = noprop::sample_u8_in(ctx, 0..50);
        // Ping フレーム with FIN=0 (fragmented)
        let mut frame = vec![
            0x09,               // FIN=0 + Ping opcode
            0x80 | payload_len, // MASK=1 + length
            0x00,
            0x00,
            0x00,
            0x00, // masking key
        ];
        frame.extend(vec![0u8; payload_len as usize]);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode();
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// 不完全なフレームの処理
// =============================================================================

/// 1 バイトだけではデコードできない
#[test]
fn prop_single_byte_incomplete() -> noprop::TestResult {
    pbt::run(|ctx| {
        let byte = noprop::sample_u8(ctx);
        let mut decoder = FrameDecoder::new();
        decoder.feed(&[byte]);

        let result = decoder.decode().unwrap();
        assert!(result.is_none());
        Ok(())
    })?;
    Ok(())
}

/// ヘッダーのみ（ペイロードなし）では不完全
#[test]
fn prop_header_only_incomplete() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, KNOWN_OPCODES);
        let payload_len = noprop::sample_u8_in(ctx, 1..50);
        // ヘッダー + マスキングキーのみ
        let frame = vec![0x80 | opcode, 0x80 | payload_len, 0x00, 0x00, 0x00, 0x00];

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode().unwrap();
        assert!(result.is_none());
        Ok(())
    })?;
    Ok(())
}

/// 拡張ペイロード長のヘッダーが不完全
///
/// 16-bit 長フィールドの 2 バイト目がなくても任意の 1 バイト目で同様に不完全となる。
#[test]
fn prop_extended_length_header_incomplete() -> noprop::TestResult {
    pbt::run(|ctx| {
        let partial_byte = noprop::sample_u8(ctx);
        // 126 マーカー付きだが拡張長が 1 バイトしかない
        let frame = vec![
            0x82,         // FIN=1 + Binary
            0x80 | 126,   // MASK=1 + extended length marker
            partial_byte, // 1 byte only (need 2)
        ];

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode().unwrap();
        assert!(result.is_none());
        Ok(())
    })?;
    Ok(())
}

/// 64-bit ペイロード長のヘッダーが不完全
#[test]
fn prop_64bit_length_header_incomplete() -> noprop::TestResult {
    pbt::run(|ctx| {
        let partial_bytes = pbt::sample_len(ctx, 0, 8);
        // 127 マーカー付きだが 8 バイト未満
        let mut frame = vec![
            0x82,       // FIN=1 + Binary
            0x80 | 127, // MASK=1 + 64-bit length marker
        ];
        frame.extend(vec![0u8; partial_bytes]);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode().unwrap();
        assert!(result.is_none());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// バッファ境界テスト
// =============================================================================

/// マスキングキーが不完全
#[test]
fn prop_incomplete_masking_key() -> noprop::TestResult {
    pbt::run(|ctx| {
        let partial_key_len = pbt::sample_len(ctx, 0, 4);
        let mut frame = vec![
            0x82,      // FIN=1 + Binary
            0x80 | 10, // MASK=1 + length=10
        ];
        frame.extend(vec![0u8; partial_key_len]); // incomplete masking key

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode().unwrap();
        assert!(result.is_none());
        Ok(())
    })?;
    Ok(())
}

/// ペイロードが不完全
#[test]
fn prop_incomplete_payload() -> noprop::TestResult {
    pbt::run(|ctx| {
        // expected は 10 以上、actual は 10 未満なので、常に actual < expected。
        let expected_len = noprop::sample_u8_in(ctx, 10..100);
        let actual_len = noprop::sample_u8_in(ctx, 0..10);

        let mut frame = vec![
            0x82,                // FIN=1 + Binary
            0x80 | expected_len, // MASK=1 + length
            0x00,
            0x00,
            0x00,
            0x00, // masking key
        ];
        frame.extend(vec![0u8; actual_len as usize]);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode().unwrap();
        assert!(result.is_none());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// Close フレームの検証
// =============================================================================

/// Close フレームのペイロードが 1 バイトは不正
///
/// コードは 2 バイト必要、または 0 バイト。
/// マスクキーとペイロードバイトを変えてもデコード自体は常に成功する。
#[test]
fn prop_close_frame_single_byte_payload() -> noprop::TestResult {
    pbt::run(|ctx| {
        let payload_byte = noprop::sample_u8(ctx);
        let mask_key = noprop::sample_bytes::<4>(ctx);
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

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        // デコード自体は成功するが、アプリケーション層で検証される
        let result = decoder.decode();
        assert!(result.is_ok());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// RFC 6455 Section 5.2: 64 ビットペイロード長の MSB 検証
// =============================================================================

/// RFC 6455 Section 5.2: 64 ビットペイロード長フィールドの最上位ビット (MSB) は 0 でなければならない。
/// MSB=1 のフレームは `FrameDecoder` で `ProtocolViolation` として拒否される。
/// 負の長さ解釈や `usize` 変換オーバーフロー前に弾く必要がある
#[test]
fn prop_payload_length_64bit_msb_set_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        // Text, Binary, Close, Ping, Pong のいずれか
        let opcode = noprop::sample_choice(ctx, KNOWN_OPCODES);
        // 最上位バイトの残り 7 ビット
        let high_low_7_bits = noprop::sample_u8(ctx);
        // 後続 7 バイトの長さ表現は任意
        let rest_bytes = noprop::sample_bytes::<7>(ctx);
        // 最上位バイトの MSB を強制的に 1 にする
        let high_byte_with_msb = high_low_7_bits | 0x80;

        let mut frame = vec![
            0x80 | opcode, // FIN=1 + opcode
            0x80 | 127,    // MASK=1 + length=127 (64-bit 長形式)
            high_byte_with_msb,
        ];
        frame.extend_from_slice(&rest_bytes);
        // フレームは 10 バイトまで埋まったので、デコーダーは 8 バイト長を読みに行く

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);
        let result = decoder.decode();
        assert!(
            result.is_err(),
            "64-bit 長の MSB=1 は ProtocolViolation で拒否されるべき"
        );
        Ok(())
    })?;
    Ok(())
}
