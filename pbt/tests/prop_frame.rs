//! WebSocket フレームのプロパティベーステスト

use proptest::prelude::*;
use shiguredo_websocket::{Frame, FrameDecoder, Opcode};

proptest! {
    /// テキストフレームのエンコード・デコードラウンドトリップ
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

        prop_assert!(decoded.fin);
        prop_assert_eq!(decoded.opcode, Opcode::Text);
        prop_assert_eq!(decoded.payload, text.as_bytes());
    }

    /// バイナリフレームのエンコード・デコードラウンドトリップ
    #[test]
    fn prop_binary_frame_roundtrip(
        data in prop::collection::vec(any::<u8>(), 0..10000),
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::binary(data.clone());
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        prop_assert!(decoded.fin);
        prop_assert_eq!(decoded.opcode, Opcode::Binary);
        prop_assert_eq!(decoded.payload, data);
    }

    /// Ping フレームのラウンドトリップ（ペイロードは125バイト以下）
    #[test]
    fn prop_ping_frame_roundtrip(
        data in prop::collection::vec(any::<u8>(), 0..125),
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::ping(data.clone()).unwrap();
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        prop_assert!(decoded.fin);
        prop_assert_eq!(decoded.opcode, Opcode::Ping);
        prop_assert_eq!(decoded.payload, data);
    }

    /// Pong フレームのラウンドトリップ
    #[test]
    fn prop_pong_frame_roundtrip(
        data in prop::collection::vec(any::<u8>(), 0..125),
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::pong(data.clone()).unwrap();
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        prop_assert!(decoded.fin);
        prop_assert_eq!(decoded.opcode, Opcode::Pong);
        prop_assert_eq!(decoded.payload, data);
    }

    /// Close フレームのラウンドトリップ
    /// コントロールフレームは 125 バイト以下、Close は code (2バイト) + reason なので
    /// reason は 123 バイト以下に制限
    /// 有効な Close コード: 1000-1003, 1007-1011, 3000-4999
    #[test]
    fn prop_close_frame_roundtrip(
        code in prop_oneof![
            1000u16..=1003,
            1007u16..=1011,
            3000u16..5000
        ],
        reason in "[a-zA-Z0-9 ]{0,50}",  // ASCII のみで50文字以下
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::close(Some(code), &reason).unwrap();
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        prop_assert!(decoded.fin);
        prop_assert_eq!(decoded.opcode, Opcode::Close);

        // ペイロードを検証
        prop_assert!(decoded.payload.len() >= 2);
        let parsed_code = u16::from_be_bytes([decoded.payload[0], decoded.payload[1]]);
        prop_assert_eq!(parsed_code, code);

        let parsed_reason = String::from_utf8_lossy(&decoded.payload[2..]);
        prop_assert_eq!(parsed_reason.as_ref(), reason);
    }

    /// パーシャルデータでのデコード
    #[test]
    fn prop_partial_decode(
        data in prop::collection::vec(any::<u8>(), 1..1000),
        masking_key in any::<[u8; 4]>(),
        chunk_size in 1usize..50
    ) {
        let frame = Frame::binary(data.clone());
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();

        // チャンクごとにフィード
        for chunk in encoded.chunks(chunk_size) {
            decoder.feed(chunk);
        }

        let decoded = decoder.decode().unwrap().unwrap();
        prop_assert_eq!(decoded.payload, data);
    }

    /// マスキングキーの一貫性
    #[test]
    fn prop_masking_consistency(
        data in prop::collection::vec(any::<u8>(), 0..1000),
        masking_key in any::<[u8; 4]>()
    ) {
        let frame = Frame::binary(data.clone());
        let encoded1 = frame.encode(masking_key);
        let encoded2 = frame.encode(masking_key);

        // 同じマスキングキーなら同じ結果
        prop_assert_eq!(encoded1, encoded2);
    }

    /// 異なるマスキングキーでの結果
    #[test]
    fn prop_different_masking_keys(
        data in prop::collection::vec(any::<u8>(), 1..100),
        key1 in any::<[u8; 4]>(),
        key2 in any::<[u8; 4]>()
    ) {
        prop_assume!(key1 != key2);

        let frame = Frame::binary(data.clone());
        let encoded1 = frame.encode(key1);
        let encoded2 = frame.encode(key2);

        // ヘッダーは同じ
        prop_assert_eq!(encoded1[0], encoded2[0]);

        // マスキングキーとペイロードは異なる（空でなければ）
        if !data.is_empty() {
            prop_assert_ne!(encoded1, encoded2);
        }
    }
}
