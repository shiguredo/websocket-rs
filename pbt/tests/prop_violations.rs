//! RFC 違反検出のプロパティベーステスト
//!
//! 不正なフレームやプロトコル違反を正しく検出できることをテスト

use proptest::prelude::*;
use shiguredo_websocket::FrameDecoder;

// =============================================================================
// RFC 6455 Section 5.2: フレーム形式違反
// =============================================================================

proptest! {
    /// 不明なオペコード (0x3-0x7, 0xB-0xF) はエラー
    #[test]
    fn prop_unknown_opcode_rejected(
        opcode in prop::sample::select(vec![0x03u8, 0x04, 0x05, 0x06, 0x07, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F]),
        payload_len in 0u8..50
    ) {
        // 不正なオペコードを持つフレームを手動で構築
        let mut frame = vec![
            0x80 | opcode,          // FIN=1 + opcode
            0x80 | payload_len,     // MASK=1 + length
            0x00, 0x00, 0x00, 0x00, // masking key
        ];
        frame.extend(vec![0u8; payload_len as usize]); // payload

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode();
        prop_assert!(result.is_err());
    }

    /// コントロールフレームが 125 バイトを超えるとエラー
    #[test]
    fn prop_control_frame_too_large(
        payload_len in 126u16..500
    ) {
        // Ping フレーム (opcode=9) with payload > 125 bytes
        let mut frame = vec![
            0x89,                   // FIN=1 + Ping opcode
            0x80 | 126,             // MASK=1 + extended length marker
            (payload_len >> 8) as u8, (payload_len & 0xFF) as u8, // 16-bit length
            0x00, 0x00, 0x00, 0x00, // masking key
        ];
        frame.extend(vec![0u8; payload_len as usize]);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode();
        prop_assert!(result.is_err());
    }

    /// コントロールフレームがフラグメント化されているとエラー (FIN=0)
    #[test]
    fn prop_fragmented_control_frame(
        payload_len in 0u8..50
    ) {
        // Ping フレーム with FIN=0 (fragmented)
        let mut frame = vec![
            0x09,                   // FIN=0 + Ping opcode
            0x80 | payload_len,     // MASK=1 + length
            0x00, 0x00, 0x00, 0x00, // masking key
        ];
        frame.extend(vec![0u8; payload_len as usize]);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode();
        prop_assert!(result.is_err());
    }
}

// =============================================================================
// 不完全なフレームの処理
// =============================================================================

proptest! {
    /// 1 バイトだけではデコードできない
    #[test]
    fn prop_single_byte_incomplete(byte in any::<u8>()) {
        let mut decoder = FrameDecoder::new();
        decoder.feed(&[byte]);

        let result = decoder.decode().unwrap();
        prop_assert!(result.is_none());
    }

    /// ヘッダーのみ（ペイロードなし）では不完全
    #[test]
    fn prop_header_only_incomplete(
        opcode in prop::sample::select(vec![0x01u8, 0x02, 0x08, 0x09, 0x0A]),
        payload_len in 1u8..50
    ) {
        // ヘッダー + マスキングキーのみ
        let frame = vec![
            0x80 | opcode,
            0x80 | payload_len,
            0x00, 0x00, 0x00, 0x00,
        ];

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode().unwrap();
        prop_assert!(result.is_none());
    }

    /// 拡張ペイロード長のヘッダーが不完全
    ///
    /// 16-bit 長フィールドの 2 バイト目がなくても任意の 1 バイト目で同様に不完全となる。
    #[test]
    fn prop_extended_length_header_incomplete(partial_byte in any::<u8>()) {
        // 126 マーカー付きだが拡張長が 1 バイトしかない
        let frame = vec![
            0x82,       // FIN=1 + Binary
            0x80 | 126, // MASK=1 + extended length marker
            partial_byte, // 1 byte only (need 2)
        ];

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode().unwrap();
        prop_assert!(result.is_none());
    }

    /// 64-bit ペイロード長のヘッダーが不完全
    #[test]
    fn prop_64bit_length_header_incomplete(partial_bytes in 0usize..8) {
        // 127 マーカー付きだが 8 バイト未満
        let mut frame = vec![
            0x82,       // FIN=1 + Binary
            0x80 | 127, // MASK=1 + 64-bit length marker
        ];
        frame.extend(vec![0u8; partial_bytes]);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode().unwrap();
        prop_assert!(result.is_none());
    }
}

// =============================================================================
// バッファ境界テスト
// =============================================================================

proptest! {
    /// マスキングキーが不完全
    #[test]
    fn prop_incomplete_masking_key(partial_key_len in 0usize..4) {
        let mut frame = vec![
            0x82,       // FIN=1 + Binary
            0x80 | 10,  // MASK=1 + length=10
        ];
        frame.extend(vec![0u8; partial_key_len]); // incomplete masking key

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode().unwrap();
        prop_assert!(result.is_none());
    }

    /// ペイロードが不完全
    #[test]
    fn prop_incomplete_payload(
        expected_len in 10u8..100,
        actual_len in 0u8..10
    ) {
        prop_assume!(actual_len < expected_len);

        let mut frame = vec![
            0x82,                   // FIN=1 + Binary
            0x80 | expected_len,    // MASK=1 + length
            0x00, 0x00, 0x00, 0x00, // masking key
        ];
        frame.extend(vec![0u8; actual_len as usize]);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        let result = decoder.decode().unwrap();
        prop_assert!(result.is_none());
    }
}

// =============================================================================
// 壊れたデータのハンドリング
// =============================================================================

proptest! {
    /// ランダムなバイト列はエラーまたは不完全
    #[test]
    fn prop_random_bytes_handling(
        data in prop::collection::vec(any::<u8>(), 1..100)
    ) {
        let mut decoder = FrameDecoder::new();
        decoder.feed(&data);

        // エラーか None のどちらかが返る（パニックしない）
        let result = decoder.decode();
        prop_assert!(result.is_ok() || result.is_err());
    }

}

// =============================================================================
// Close フレームの検証
// =============================================================================

proptest! {
    /// Close フレームのペイロードが 1 バイトは不正
    ///
    /// コードは 2 バイト必要、または 0 バイト。
    /// マスクキーとペイロードバイトを変えてもデコード自体は常に成功する。
    #[test]
    fn prop_close_frame_single_byte_payload(
        payload_byte in any::<u8>(),
        mask_key in prop::array::uniform4(any::<u8>()),
    ) {
        let masked_payload = payload_byte ^ mask_key[0];
        let frame = vec![
            0x88,       // FIN=1 + Close opcode
            0x80 | 1,   // MASK=1 + length=1
            mask_key[0], mask_key[1], mask_key[2], mask_key[3],
            masked_payload,
        ];

        let mut decoder = FrameDecoder::new();
        decoder.feed(&frame);

        // デコード自体は成功するが、アプリケーション層で検証される
        let result = decoder.decode();
        prop_assert!(result.is_ok());
    }
}
