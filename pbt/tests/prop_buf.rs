use proptest::prelude::*;
use shiguredo_websocket::{ByteSliceExt, VecExt};

proptest! {
    #[test]
    fn test_read_u8_roundtrip(v in any::<u8>()) {
        let buf = vec![v];
        let mut slice = buf.as_slice();
        let got = slice.read_u8().unwrap();
        prop_assert_eq!(got, v);
        prop_assert!(slice.is_empty());
    }

    #[test]
    fn test_read_u16_roundtrip(v in any::<u16>()) {
        let mut buf = vec![];
        buf.write_u16(v);
        let mut slice = buf.as_slice();
        let got = slice.read_u16().unwrap();
        prop_assert_eq!(got, v);
        prop_assert!(slice.is_empty());
    }

    #[test]
    fn test_read_u32_roundtrip(v in any::<u32>()) {
        let mut buf = vec![];
        buf.write_u32(v);
        let mut slice = buf.as_slice();
        let got = slice.read_u32().unwrap();
        prop_assert_eq!(got, v);
        prop_assert!(slice.is_empty());
    }

    #[test]
    fn test_read_u64_roundtrip(v in any::<u64>()) {
        let mut buf = vec![];
        buf.write_u64(v);
        let mut slice = buf.as_slice();
        let got = slice.read_u64().unwrap();
        prop_assert_eq!(got, v);
        prop_assert!(slice.is_empty());
    }

    #[test]
    fn test_read_bytes_roundtrip(data in any::<Vec<u8>>(), extra in any::<Vec<u8>>()) {
        let mut buf = data.clone();
        buf.extend_from_slice(&extra);
        let mut slice = buf.as_slice();
        let got = slice.read_bytes(data.len()).unwrap();
        prop_assert_eq!(got, data);
        prop_assert_eq!(slice, extra.as_slice());
    }

    #[test]
    fn test_read_utf8_roundtrip(s in any::<String>(), extra in any::<Vec<u8>>()) {
        let mut buf = s.as_bytes().to_vec();
        buf.extend_from_slice(&extra);
        let mut slice = buf.as_slice();
        let got = slice.read_utf8(s.as_bytes().len()).unwrap();
        prop_assert_eq!(got, s);
        prop_assert_eq!(slice, extra.as_slice());
    }

    #[test]
    fn test_read_bytes_insufficient(data in any::<Vec<u8>>()) {
        let len = data.len().saturating_add(1);
        let mut slice = data.as_slice();
        let result = slice.read_bytes(len);
        prop_assert!(result.is_err());
    }

    #[test]
    fn test_read_utf8_invalid(data in any::<Vec<u8>>()) {
        prop_assume!(String::from_utf8(data.clone()).is_err());
        let mut slice = data.as_slice();
        let result = slice.read_utf8(data.len());
        prop_assert!(result.is_err());
    }

    #[test]
    fn test_vec_write_then_read_roundtrip(
        v8 in any::<u8>(),
        v16 in any::<u16>(),
        v32 in any::<u32>(),
        v64 in any::<u64>(),
        bytes in any::<Vec<u8>>(),
    ) {
        let mut buf = Vec::new();
        buf.write_u8(v8);
        buf.write_u16(v16);
        buf.write_u32(v32);
        buf.write_u64(v64);
        buf.write_bytes(&bytes);

        let mut slice = buf.as_slice();
        prop_assert_eq!(slice.read_u8().unwrap(), v8);
        prop_assert_eq!(slice.read_u16().unwrap(), v16);
        prop_assert_eq!(slice.read_u32().unwrap(), v32);
        prop_assert_eq!(slice.read_u64().unwrap(), v64);
        prop_assert_eq!(slice.read_bytes(bytes.len()).unwrap(), bytes);
        prop_assert!(slice.is_empty());
    }
}
