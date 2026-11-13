use std::cell::Cell;

use shiguredo_websocket::{PerMessageDeflate, PerMessageDeflateConfig};

const DEFLATE_TRAILER: [u8; 4] = [0x00, 0x00, 0xFF, 0xFF];

/// テスト用の最大解凍サイズ（16MB）
const TEST_MAX_DECOMPRESS_SIZE: usize = 16 * 1024 * 1024;

const PATTERN_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ";

fn sample_optional_window_bits(ctx: &mut noprop::TestCaseContext) -> Option<u8> {
    if noprop::sample_bool(ctx) {
        Some(noprop::sample_u8_in(ctx, 8..=15))
    } else {
        None
    }
}

/// 圧縮・解凍のラウンドトリップ（クライアント/サーバーペア）
#[test]
fn prop_compress_decompress_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 0, 10000);
        let config = PerMessageDeflateConfig::default();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        // クライアント -> サーバー
        let compressed = client.compress(&data).unwrap();
        let decompressed = server
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();

        assert_eq!(decompressed, data);
        Ok(())
    })?;
    Ok(())
}

#[test]
fn prop_no_trailer_in_compressed() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 1, 1000);
        let config = PerMessageDeflateConfig::default();
        let mut codec = PerMessageDeflate::new_client(config);

        let compressed = codec.compress(&data).unwrap();

        assert!(!compressed.ends_with(&DEFLATE_TRAILER));
        Ok(())
    })?;
    Ok(())
}

/// 繰り返しデータの圧縮効率（クライアント/サーバーペア）
#[test]
fn prop_repetitive_data_compresses_well() -> noprop::TestResult {
    // 圧縮が元データより小さくなる表明は data.len() > 100 のときだけ走る。
    // pattern 1..=49、repeats 10..=99 なので、その積が 100 を超える確率は 0.5 を大きく超える。
    // 256 ケースで一度も超えない確率は無視できる。
    let compressed_long = Cell::new(0usize);
    let runner = pbt::run(|ctx| {
        let pattern = pbt::sample_byte_vec(ctx, 1, 50);
        let repeats = pbt::sample_len(ctx, 10, 100);
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
            assert!(compressed.len() < data.len());
            compressed_long.set(compressed_long.get() + 1);
        }

        let decompressed = server
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed, data);
        Ok(())
    })?;
    assert!(
        compressed_long.get() > 0,
        "100 バイトを超える繰り返しデータを圧縮していない\n{runner}"
    );
    Ok(())
}

/// 複数回の圧縮・解凍（クライアント/サーバーペア）
#[test]
fn prop_multiple_compress_decompress() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 1, 500);
        let iterations = pbt::sample_len(ctx, 2, 5);
        let config = PerMessageDeflateConfig::default();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let mut current = data.clone();
        for _ in 0..iterations {
            // クライアント -> サーバー
            let compressed = client.compress(&current).unwrap();
            current = server
                .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
                .unwrap();
        }

        assert_eq!(current, data);
        Ok(())
    })?;
    Ok(())
}

/// no_context_takeover の動作確認（クライアント/サーバーペア）
#[test]
fn prop_no_context_takeover() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 1, 500);
        let config = PerMessageDeflateConfig::new()
            .client_no_context_takeover()
            .server_no_context_takeover();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let compressed = client.compress(&data).unwrap();
        let decompressed = server
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();

        assert_eq!(decompressed, data);
        Ok(())
    })?;
    Ok(())
}

/// window_bits のバリエーション（no_context_takeover でテスト）
#[test]
fn prop_window_bits_variations() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 1, 500);
        let server_bits = noprop::sample_u8_in(ctx, 8..=15);
        let client_bits = noprop::sample_u8_in(ctx, 8..=15);
        let config = PerMessageDeflateConfig::new()
            .server_max_window_bits(server_bits)
            .client_max_window_bits(client_bits)
            .client_no_context_takeover()
            .server_no_context_takeover();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let compressed = client.compress(&data).unwrap();
        let decompressed = server
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();

        assert_eq!(decompressed, data);
        Ok(())
    })?;
    Ok(())
}

/// サーバーコーデックのテスト（クライアント/サーバーペア）
#[test]
fn prop_server_codec() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 1, 500);
        let config = PerMessageDeflateConfig::default();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        // サーバー -> クライアント
        let compressed = server.compress(&data).unwrap();
        let decompressed = client
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();

        assert_eq!(decompressed, data);
        Ok(())
    })?;
    Ok(())
}

#[test]
fn prop_should_compress_threshold() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data_len = pbt::sample_len(ctx, 0, 1000);
        let threshold = pbt::sample_len(ctx, 0, 1000);
        let config = PerMessageDeflateConfig::default();
        let codec = PerMessageDeflate::new_client(config);

        let data = vec![0u8; data_len];
        let should = codec.should_compress(&data, threshold);

        assert_eq!(should, data_len >= threshold);
        Ok(())
    })?;
    Ok(())
}

/// 最大解凍サイズを超えるデータはエラーになることを確認（クライアント/サーバーペア）
#[test]
fn prop_decompress_max_size_limit() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 1000 バイト以上のデータを生成。1000..5000 なので上限は 4999。
        let data = pbt::sample_byte_vec(ctx, 1000, 5000);
        let config = PerMessageDeflateConfig::default();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let compressed = client.compress(&data).unwrap();

        // 元データより小さい上限を設定
        let small_limit = data.len() / 2;
        let result = server.decompress(&compressed, small_limit);

        // エラーになることを確認
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// 十分な上限があれば解凍が成功することを確認（クライアント/サーバーペア）
#[test]
fn prop_decompress_within_limit() -> noprop::TestResult {
    pbt::run(|ctx| {
        let data = pbt::sample_byte_vec(ctx, 1, 1000);
        let config = PerMessageDeflateConfig::default();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let compressed = client.compress(&data).unwrap();

        // 元データと同じサイズの上限を設定
        let result = server.decompress(&compressed, data.len());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), data);
        Ok(())
    })?;
    Ok(())
}

// ==== 追加テスト: 空データ・コンテキスト再利用・双方向通信 ====

/// 空データの圧縮・解凍
#[test]
fn prop_compress_decompress_empty() {
    let config = PerMessageDeflateConfig::default();
    let mut client = PerMessageDeflate::new_client(config.clone());
    let mut server = PerMessageDeflate::new_server(config);

    let compressed = client.compress(b"").unwrap();
    let decompressed = server
        .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
        .unwrap();

    assert!(decompressed.is_empty());
}

/// コンテキスト再利用による圧縮効率向上の検証
#[test]
fn prop_context_takeover_improves_compression() -> noprop::TestResult {
    pbt::run(|ctx| {
        let pattern = pbt::sample_alphabet(ctx, PATTERN_ALPHABET, 20, 50);
        let repeats = pbt::sample_len(ctx, 3, 6);
        // Context Takeover 有効（デフォルト）
        let config = PerMessageDeflateConfig::new();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let msg = pattern.repeat(repeats);
        let mut compressed_sizes = Vec::new();

        for _ in 0..3 {
            let compressed = client.compress(msg.as_bytes()).unwrap();
            compressed_sizes.push(compressed.len());

            let decompressed = server
                .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
                .unwrap();
            assert_eq!(decompressed, msg.as_bytes());
        }

        // 全て元データより小さいこと
        for &size in &compressed_sizes {
            assert!(size < msg.len());
        }
        Ok(())
    })?;
    Ok(())
}

/// 双方向通信（クライアント->サーバーとサーバー->クライアント交互）
#[test]
fn prop_bidirectional_communication() -> noprop::TestResult {
    pbt::run(|ctx| {
        let messages = pbt::sample_vec(ctx, 2, 6, |ctx| pbt::sample_byte_vec(ctx, 1, 200));
        let config = PerMessageDeflateConfig::new();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        for (i, msg) in messages.iter().enumerate() {
            if i % 2 == 0 {
                // クライアント -> サーバー
                let compressed = client.compress(msg).unwrap();
                let decompressed = server
                    .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
                    .unwrap();
                assert_eq!(&decompressed, msg);
            } else {
                // サーバー -> クライアント
                let compressed = server.compress(msg).unwrap();
                let decompressed = client
                    .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
                    .unwrap();
                assert_eq!(&decompressed, msg);
            }
        }
        Ok(())
    })?;
    Ok(())
}

// ==== config() ゲッターのテスト ====

#[test]
fn prop_config_getter() -> noprop::TestResult {
    pbt::run(|ctx| {
        let server_bits = sample_optional_window_bits(ctx);
        let client_bits = sample_optional_window_bits(ctx);
        let server_no_takeover = noprop::sample_bool(ctx);
        let client_no_takeover = noprop::sample_bool(ctx);
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

        assert_eq!(
            retrieved.server_max_window_bits,
            config.server_max_window_bits
        );
        assert_eq!(
            retrieved.client_max_window_bits,
            config.client_max_window_bits
        );
        assert_eq!(
            retrieved.server_no_context_takeover,
            config.server_no_context_takeover
        );
        assert_eq!(
            retrieved.client_no_context_takeover,
            config.client_no_context_takeover
        );
        Ok(())
    })?;
    Ok(())
}

// ==== negotiate() が RFC 7692 に従い no_context_takeover を設定することを確認 ====

#[test]
fn prop_negotiate_or_logic_for_no_context_takeover() -> noprop::TestResult {
    pbt::run(|ctx| {
        let client_server_no_takeover = noprop::sample_bool(ctx);
        let client_client_no_takeover = noprop::sample_bool(ctx);
        let server_server_no_takeover = noprop::sample_bool(ctx);
        let server_client_no_takeover = noprop::sample_bool(ctx);
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
        assert_eq!(
            negotiated.server_no_context_takeover,
            client_server_no_takeover || server_server_no_takeover
        );
        assert_eq!(
            negotiated.client_no_context_takeover,
            client_client_no_takeover || server_client_no_takeover
        );
        Ok(())
    })?;
    Ok(())
}
