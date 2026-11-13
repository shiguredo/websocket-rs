//! バイト列読み書きのプロパティベーステスト

use shiguredo_websocket::{ByteSliceExt, VecExt};

#[test]
fn prop_read_u8_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let value = noprop::sample_u8(ctx);
        let buf = vec![value];
        let mut slice = buf.as_slice();
        let got = slice.read_u8().unwrap();
        assert_eq!(got, value);
        assert!(slice.is_empty());
        Ok(())
    })?;
    Ok(())
}

#[test]
fn prop_read_u16_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let value = noprop::sample_u16(ctx);
        let mut buf = vec![];
        buf.write_u16(value);
        let mut slice = buf.as_slice();
        let got = slice.read_u16().unwrap();
        assert_eq!(got, value);
        assert!(slice.is_empty());
        Ok(())
    })?;
    Ok(())
}

#[test]
fn prop_read_u32_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let value = noprop::sample_u32(ctx);
        let mut buf = vec![];
        buf.write_u32(value);
        let mut slice = buf.as_slice();
        let got = slice.read_u32().unwrap();
        assert_eq!(got, value);
        assert!(slice.is_empty());
        Ok(())
    })?;
    Ok(())
}

#[test]
fn prop_read_u64_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let value = noprop::sample_u64(ctx);
        let mut buf = vec![];
        buf.write_u64(value);
        let mut slice = buf.as_slice();
        let got = slice.read_u64().unwrap();
        assert_eq!(got, value);
        assert!(slice.is_empty());
        Ok(())
    })?;
    Ok(())
}

#[test]
fn prop_read_bytes_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 長さ 0..=99。空と 1 バイトと上限を境界として厚く引く。
        let data = pbt::sample_byte_vec(ctx, 0, 100);
        let extra = pbt::sample_byte_vec(ctx, 0, 100);
        let mut buf = data.clone();
        buf.extend_from_slice(&extra);
        let mut slice = buf.as_slice();
        let got = slice.read_bytes(data.len()).unwrap();
        assert_eq!(got, data);
        assert_eq!(slice, extra.as_slice());
        Ok(())
    })?;
    Ok(())
}

#[test]
fn prop_read_utf8_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let text = pbt::sample_text(ctx, 0, 99);
        let extra = pbt::sample_byte_vec(ctx, 0, 100);
        let mut buf = text.as_bytes().to_vec();
        buf.extend_from_slice(&extra);
        let mut slice = buf.as_slice();
        let got = slice.read_utf8(text.len()).unwrap();
        assert_eq!(got, text);
        assert_eq!(slice, extra.as_slice());
        Ok(())
    })?;
    Ok(())
}

#[test]
fn prop_read_bytes_insufficient() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 100);
        let len = data.len().saturating_add(1);
        let mut slice = data.as_slice();
        let result = slice.read_bytes(len);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

#[test]
fn prop_read_utf8_invalid() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0xFF を含むので、常に不正な UTF-8 になる。
        let data = pbt::sample_invalid_utf8(ctx);
        let mut slice = data.as_slice();
        let result = slice.read_utf8(data.len());
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

#[test]
fn prop_vec_write_then_read_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let v8 = noprop::sample_u8(ctx);
        let v16 = noprop::sample_u16(ctx);
        let v32 = noprop::sample_u32(ctx);
        let v64 = noprop::sample_u64(ctx);
        let bytes = pbt::sample_byte_vec(ctx, 0, 100);
        let mut buf = Vec::new();
        buf.write_u8(v8);
        buf.write_u16(v16);
        buf.write_u32(v32);
        buf.write_u64(v64);
        buf.write_bytes(&bytes);

        let mut slice = buf.as_slice();
        assert_eq!(slice.read_u8().unwrap(), v8);
        assert_eq!(slice.read_u16().unwrap(), v16);
        assert_eq!(slice.read_u32().unwrap(), v32);
        assert_eq!(slice.read_u64().unwrap(), v64);
        assert_eq!(slice.read_bytes(bytes.len()).unwrap(), bytes);
        assert!(slice.is_empty());
        Ok(())
    })?;
    Ok(())
}
