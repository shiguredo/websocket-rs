use proptest::prelude::*;
use shiguredo_websocket::{PerMessageDeflate, PerMessageDeflateConfig};

const DEFLATE_TRAILER: [u8; 4] = [0x00, 0x00, 0xFF, 0xFF];

proptest! {
    #[test]
    fn prop_compress_decompress_roundtrip(
        data in prop::collection::vec(any::<u8>(), 0..10000)
    ) {
        let config = PerMessageDeflateConfig::default();
        let mut codec = PerMessageDeflate::new_client(config);

        let compressed = codec.compress(&data).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();

        prop_assert_eq!(decompressed, data);
    }

    #[test]
    fn prop_no_trailer_in_compressed(
        data in prop::collection::vec(any::<u8>(), 1..1000)
    ) {
        let config = PerMessageDeflateConfig::default();
        let mut codec = PerMessageDeflate::new_client(config);

        let compressed = codec.compress(&data).unwrap();

        prop_assert!(!compressed.ends_with(&DEFLATE_TRAILER));
    }

    #[test]
    fn prop_repetitive_data_compresses_well(
        pattern in prop::collection::vec(any::<u8>(), 1..50),
        repeats in 10usize..100
    ) {
        let data: Vec<u8> = pattern
            .iter()
            .cycle()
            .take(pattern.len() * repeats)
            .copied()
            .collect();

        let config = PerMessageDeflateConfig::default();
        let mut codec = PerMessageDeflate::new_client(config);

        let compressed = codec.compress(&data).unwrap();

        if data.len() > 100 {
            prop_assert!(compressed.len() < data.len());
        }

        let decompressed = codec.decompress(&compressed).unwrap();
        prop_assert_eq!(decompressed, data);
    }

    #[test]
    fn prop_compression_levels_preserve_data(
        data in prop::collection::vec(any::<u8>(), 1..1000),
        level in 0u32..=9
    ) {
        let config = PerMessageDeflateConfig::default();
        let mut codec = PerMessageDeflate::new_client(config);
        codec.set_compression_level(level);

        let compressed = codec.compress(&data).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();

        prop_assert_eq!(decompressed, data);
    }

    #[test]
    fn prop_multiple_compress_decompress(
        data in prop::collection::vec(any::<u8>(), 1..500),
        iterations in 2usize..5
    ) {
        let config = PerMessageDeflateConfig::default();
        let mut codec = PerMessageDeflate::new_client(config);

        let mut current = data.clone();
        for _ in 0..iterations {
            let compressed = codec.compress(&current).unwrap();
            current = codec.decompress(&compressed).unwrap();
        }

        prop_assert_eq!(current, data);
    }

    #[test]
    fn prop_no_context_takeover(
        data in prop::collection::vec(any::<u8>(), 1..500)
    ) {
        let config = PerMessageDeflateConfig::new()
            .client_no_context_takeover()
            .server_no_context_takeover();
        let mut codec = PerMessageDeflate::new_client(config);

        let compressed = codec.compress(&data).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();

        prop_assert_eq!(decompressed, data);
    }

    #[test]
    fn prop_window_bits_variations(
        data in prop::collection::vec(any::<u8>(), 1..500),
        server_bits in 8u8..=15,
        client_bits in 8u8..=15
    ) {
        let config = PerMessageDeflateConfig::new()
            .server_max_window_bits(server_bits)
            .client_max_window_bits(client_bits);
        let mut codec = PerMessageDeflate::new_client(config);

        let compressed = codec.compress(&data).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();

        prop_assert_eq!(decompressed, data);
    }

    #[test]
    fn prop_server_codec(
        data in prop::collection::vec(any::<u8>(), 1..500)
    ) {
        let config = PerMessageDeflateConfig::default();
        let mut codec = PerMessageDeflate::new_server(config);

        let compressed = codec.compress(&data).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();

        prop_assert_eq!(decompressed, data);
    }

    #[test]
    fn prop_should_compress_threshold(
        data_len in 0usize..1000,
        threshold in 0usize..1000
    ) {
        let config = PerMessageDeflateConfig::default();
        let codec = PerMessageDeflate::new_client(config);

        let data = vec![0u8; data_len];
        let should = codec.should_compress(&data, threshold);

        prop_assert_eq!(should, data_len >= threshold);
    }
}

proptest! {
    // ==== config() ゲッターのテスト ====

    #[test]
    fn prop_config_getter(
        server_bits in prop::option::of(8u8..=15),
        client_bits in prop::option::of(8u8..=15),
        server_no_takeover in any::<bool>(),
        client_no_takeover in any::<bool>()
    ) {
        let mut config = PerMessageDeflateConfig::new();
        if let Some(bits) = server_bits {
            config = config.server_max_window_bits(bits);
        }
        if let Some(bits) = client_bits {
            config = config.client_max_window_bits(bits);
        }
        if server_no_takeover {
            config = config.server_no_context_takeover();
        }
        if client_no_takeover {
            config = config.client_no_context_takeover();
        }

        let codec = PerMessageDeflate::new_client(config.clone());
        let retrieved = codec.config();

        prop_assert_eq!(retrieved.server_max_window_bits, config.server_max_window_bits);
        prop_assert_eq!(retrieved.client_max_window_bits, config.client_max_window_bits);
        prop_assert_eq!(retrieved.server_no_context_takeover, config.server_no_context_takeover);
        prop_assert_eq!(retrieved.client_no_context_takeover, config.client_no_context_takeover);
    }
}
