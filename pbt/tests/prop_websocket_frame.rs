//! WebSocket フレームのプロパティベーステスト

use shiguredo_websocket::{Frame, FrameDecoder, Opcode};

const REASON_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ";

/// テキストフレームのエンコード・デコードラウンドトリップ
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

        assert!(decoded.fin);
        assert_eq!(decoded.opcode, Opcode::Text);
        assert_eq!(decoded.payload, text.as_bytes());
        Ok(())
    })?;
    Ok(())
}

/// バイナリフレームのエンコード・デコードラウンドトリップ
#[test]
fn prop_binary_frame_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 10000);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::binary(data.clone());
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");

        assert!(decoded.fin);
        assert_eq!(decoded.opcode, Opcode::Binary);
        assert_eq!(decoded.payload, data);
        Ok(())
    })?;
    Ok(())
}

/// Ping フレームのラウンドトリップ（ペイロードは125バイト以下）
#[test]
fn prop_ping_frame_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..125 なので 125 自体は含まない。
        let data = pbt::sample_byte_vec(ctx, 0, 125);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::ping(data.clone()).expect("Ping フレームの生成に失敗してはならない");
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");

        assert!(decoded.fin);
        assert_eq!(decoded.opcode, Opcode::Ping);
        assert_eq!(decoded.payload, data);
        Ok(())
    })?;
    Ok(())
}

/// Pong フレームのラウンドトリップ
#[test]
fn prop_pong_frame_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 125);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::pong(data.clone()).expect("Pong フレームの生成に失敗してはならない");
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");

        assert!(decoded.fin);
        assert_eq!(decoded.opcode, Opcode::Pong);
        assert_eq!(decoded.payload, data);
        Ok(())
    })?;
    Ok(())
}

/// Close フレームのラウンドトリップ
/// コントロールフレームは 125 バイト以下、Close は code (2バイト) + reason なので
/// reason は 123 バイト以下に制限
/// 有効な Close コード: 1000-1003, 1007-1011, 3000-4999
#[test]
fn prop_close_frame_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let code = pbt::sample_frame_close_code(ctx);
        // ASCII のみで 50 文字以下
        let reason = pbt::sample_alphabet(ctx, REASON_ALPHABET, 0, 50);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame =
            Frame::close(Some(code), &reason).expect("Close フレームの生成に失敗してはならない");
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");

        assert!(decoded.fin);
        assert_eq!(decoded.opcode, Opcode::Close);

        // ペイロードを検証
        assert!(decoded.payload.len() >= 2);
        let parsed_code = u16::from_be_bytes([decoded.payload[0], decoded.payload[1]]);
        assert_eq!(parsed_code, code);

        let parsed_reason = String::from_utf8_lossy(&decoded.payload[2..]);
        assert_eq!(parsed_reason.as_ref(), reason);
        Ok(())
    })?;
    Ok(())
}

/// パーシャルデータでのデコード
#[test]
fn prop_partial_decode() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 1, 1000);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let chunk_size = pbt::sample_len(ctx, 1, 50);
        let frame = Frame::binary(data.clone());
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();

        // チャンクごとにフィード
        for chunk in encoded.chunks(chunk_size) {
            decoder.feed(chunk);
        }

        let decoded = decoder
            .decode()
            .expect("デコードに失敗してはならない")
            .expect("デコード結果が取得できるはず");
        assert_eq!(decoded.payload, data);
        Ok(())
    })?;
    Ok(())
}

/// マスキングキーの一貫性
#[test]
fn prop_masking_consistency() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 1000);
        let masking_key = noprop::sample_bytes::<4>(ctx);
        let frame = Frame::binary(data.clone());
        let encoded1 = frame.encode(masking_key);
        let encoded2 = frame.encode(masking_key);

        // 同じマスキングキーなら同じ結果
        assert_eq!(encoded1, encoded2);
        Ok(())
    })?;
    Ok(())
}

/// 異なるマスキングキーでの結果
#[test]
fn prop_different_masking_keys() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 1..100 なのでペイロードは空にならない。
        let data = pbt::sample_byte_vec(ctx, 1, 100);
        let (key1, key2) = pbt::sample_distinct_bytes::<4>(ctx);

        let frame = Frame::binary(data.clone());
        let encoded1 = frame.encode(key1);
        let encoded2 = frame.encode(key2);

        // ヘッダーは同じ
        assert_eq!(encoded1[0], encoded2[0]);

        // マスキングキーとペイロードは異なる（空でなければ）
        if !data.is_empty() {
            assert_ne!(encoded1, encoded2);
        }
        Ok(())
    })?;
    Ok(())
}
