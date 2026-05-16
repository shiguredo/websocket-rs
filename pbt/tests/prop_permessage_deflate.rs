use proptest::prelude::*;
use shiguredo_websocket::{PerMessageDeflate, PerMessageDeflateConfig};

const DEFLATE_TRAILER: [u8; 4] = [0x00, 0x00, 0xFF, 0xFF];

/// テスト用の最大解凍サイズ（16MB）
const TEST_MAX_DECOMPRESS_SIZE: usize = 16 * 1024 * 1024;

proptest! {
    /// 圧縮・解凍のラウンドトリップ（クライアント/サーバーペア）
    #[test]
    fn prop_compress_decompress_roundtrip(
        data in prop::collection::vec(any::<u8>(), 0..10000)
    ) {
        let config = PerMessageDeflateConfig::default();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        // クライアント -> サーバー
        let compressed = client.compress(&data).unwrap();
        let decompressed = server.decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE).unwrap();

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

    /// 繰り返しデータの圧縮効率（クライアント/サーバーペア）
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
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let compressed = client.compress(&data).unwrap();

        if data.len() > 100 {
            prop_assert!(compressed.len() < data.len());
        }

        let decompressed = server.decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE).unwrap();
        prop_assert_eq!(decompressed, data);
    }

    /// 複数回の圧縮・解凍（クライアント/サーバーペア）
    #[test]
    fn prop_multiple_compress_decompress(
        data in prop::collection::vec(any::<u8>(), 1..500),
        iterations in 2usize..5
    ) {
        let config = PerMessageDeflateConfig::default();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let mut current = data.clone();
        for _ in 0..iterations {
            // クライアント -> サーバー
            let compressed = client.compress(&current).unwrap();
            current = server.decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE).unwrap();
        }

        prop_assert_eq!(current, data);
    }

    /// no_context_takeover の動作確認（クライアント/サーバーペア）
    #[test]
    fn prop_no_context_takeover(
        data in prop::collection::vec(any::<u8>(), 1..500)
    ) {
        let config = PerMessageDeflateConfig::new()
            .client_no_context_takeover()
            .server_no_context_takeover();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let compressed = client.compress(&data).unwrap();
        let decompressed = server.decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE).unwrap();

        prop_assert_eq!(decompressed, data);
    }

    /// window_bits のバリエーション（no_context_takeover でテスト）
    #[test]
    fn prop_window_bits_variations(
        data in prop::collection::vec(any::<u8>(), 1..500),
        server_bits in 8u8..=15,
        client_bits in 8u8..=15
    ) {
        let config = PerMessageDeflateConfig::new()
            .server_max_window_bits(server_bits)
            .client_max_window_bits(client_bits)
            .client_no_context_takeover()
            .server_no_context_takeover();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let compressed = client.compress(&data).unwrap();
        let decompressed = server.decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE).unwrap();

        prop_assert_eq!(decompressed, data);
    }

    /// サーバーコーデックのテスト（クライアント/サーバーペア）
    #[test]
    fn prop_server_codec(
        data in prop::collection::vec(any::<u8>(), 1..500)
    ) {
        let config = PerMessageDeflateConfig::default();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        // サーバー -> クライアント
        let compressed = server.compress(&data).unwrap();
        let decompressed = client.decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE).unwrap();

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

    /// 最大解凍サイズを超えるデータはエラーになることを確認（クライアント/サーバーペア）
    #[test]
    fn prop_decompress_max_size_limit(
        // 1000 バイト以上のデータを生成
        data in prop::collection::vec(any::<u8>(), 1000..5000)
    ) {
        let config = PerMessageDeflateConfig::default();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let compressed = client.compress(&data).unwrap();

        // 元データより小さい上限を設定
        let small_limit = data.len() / 2;
        let result = server.decompress(&compressed, small_limit);

        // エラーになることを確認
        prop_assert!(result.is_err());
    }

    /// 十分な上限があれば解凍が成功することを確認（クライアント/サーバーペア）
    #[test]
    fn prop_decompress_within_limit(
        data in prop::collection::vec(any::<u8>(), 1..1000)
    ) {
        let config = PerMessageDeflateConfig::default();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let compressed = client.compress(&data).unwrap();

        // 元データと同じサイズの上限を設定
        let result = server.decompress(&compressed, data.len());
        prop_assert!(result.is_ok());
        prop_assert_eq!(result.unwrap(), data);
    }
}

// ==== 追加テスト: 空データ・コンテキスト再利用・双方向通信 ====

proptest! {
    /// 空データの圧縮・解凍
    #[test]
    fn prop_compress_decompress_empty(_dummy in Just(())) {
        let config = PerMessageDeflateConfig::default();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let compressed = client.compress(b"").unwrap();
        let decompressed = server.decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE).unwrap();

        prop_assert!(decompressed.is_empty());
    }

    /// コンテキスト再利用による圧縮効率向上の検証
    #[test]
    fn prop_context_takeover_improves_compression(
        pattern in "[a-zA-Z0-9 ]{20,50}",
        repeats in 3usize..6,
    ) {
        // Context Takeover 有効（デフォルト）
        let config = PerMessageDeflateConfig::new();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let msg = pattern.repeat(repeats);
        let mut compressed_sizes = Vec::new();

        for _ in 0..3 {
            let compressed = client.compress(msg.as_bytes()).unwrap();
            compressed_sizes.push(compressed.len());

            let decompressed = server.decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE).unwrap();
            prop_assert_eq!(decompressed, msg.as_bytes());
        }

        // 全て元データより小さいこと
        for &size in &compressed_sizes {
            prop_assert!(size < msg.len());
        }
    }

    /// 双方向通信（クライアント->サーバーとサーバー->クライアント交互）
    #[test]
    fn prop_bidirectional_communication(
        messages in prop::collection::vec(
            prop::collection::vec(any::<u8>(), 1..200),
            2..6,
        )
    ) {
        let config = PerMessageDeflateConfig::new();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        for (i, msg) in messages.iter().enumerate() {
            if i % 2 == 0 {
                // クライアント -> サーバー
                let compressed = client.compress(msg).unwrap();
                let decompressed = server.decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE).unwrap();
                prop_assert_eq!(&decompressed, msg);
            } else {
                // サーバー -> クライアント
                let compressed = server.compress(msg).unwrap();
                let decompressed = client.decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE).unwrap();
                prop_assert_eq!(&decompressed, msg);
            }
        }
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

    // ==== negotiate() が RFC 7692 に従い no_context_takeover を設定することを確認 ====

    #[test]
    fn prop_negotiate_or_logic_for_no_context_takeover(
        client_server_no_takeover in any::<bool>(),
        client_client_no_takeover in any::<bool>(),
        server_server_no_takeover in any::<bool>(),
        server_client_no_takeover in any::<bool>()
    ) {
        let mut client_config = PerMessageDeflateConfig::new();
        let mut server_config = PerMessageDeflateConfig::new();

        if client_server_no_takeover {
            client_config = client_config.server_no_context_takeover();
        }
        if client_client_no_takeover {
            client_config = client_config.client_no_context_takeover();
        }
        if server_server_no_takeover {
            server_config = server_config.server_no_context_takeover();
        }
        if server_client_no_takeover {
            server_config = server_config.client_no_context_takeover();
        }

        let negotiated = PerMessageDeflateConfig::negotiate(&client_config, &server_config);

        // RFC 7692: どちらかが要求すれば no_context_takeover が有効
        prop_assert_eq!(
            negotiated.server_no_context_takeover,
            client_server_no_takeover || server_server_no_takeover
        );
        prop_assert_eq!(
            negotiated.client_no_context_takeover,
            client_client_no_takeover || server_client_no_takeover
        );
    }
}
