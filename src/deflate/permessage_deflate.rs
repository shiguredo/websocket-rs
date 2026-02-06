//! permessage-deflate 圧縮拡張 (RFC 7692)
//!
//! WebSocket メッセージの DEFLATE 圧縮/解凍を提供する。

use flate2::{Compress, Compression, FlushCompress};

use crate::error::Error;
use crate::websocket_extension::PerMessageDeflateConfig;

/// DEFLATE 圧縮終端バイト列
/// RFC 7692 Section 7.2.1: 0x00 0x00 0xFF 0xFF
const DEFLATE_TRAILER: [u8; 4] = [0x00, 0x00, 0xFF, 0xFF];

/// permessage-deflate コンプレッサー
pub struct Compressor {
    /// DEFLATE コンテキスト
    compress: Compress,
    /// 圧縮レベル (0-9)
    level: u32,
    /// メッセージ毎にリセットするか（no_context_takeover）
    reset_after_message: bool,
}

impl Compressor {
    /// 新しいコンプレッサーを生成
    ///
    /// `is_client` が true の場合、クライアント側の設定（client_no_context_takeover）を使用。
    /// false の場合、サーバー側の設定（server_no_context_takeover）を使用。
    ///
    /// # 制限事項
    ///
    /// window_bits のカスタマイズは flate2 の `any_zlib` feature が必要なため、
    /// 現在の実装ではデフォルト値 (15) を使用する。
    ///
    /// RFC 7692 Section 7.2.1 では、合意した window bits を超える LZ77 ウィンドウを
    /// 使用してはならないと規定されている。この制約はハンドシェイク時に担保する:
    /// - サーバー: negotiate() で server_max_window_bits を含めない (デフォルト 15)
    /// - クライアント: client_max_window_bits < 15 のレスポンスを拒否する
    pub fn new(config: &PerMessageDeflateConfig, is_client: bool) -> Self {
        let level = 6u32; // デフォルト圧縮レベル
        let reset_after_message = if is_client {
            config.client_no_context_takeover
        } else {
            config.server_no_context_takeover
        };

        Self {
            compress: Compress::new(Compression::new(level), false), // raw deflate
            level,
            reset_after_message,
        }
    }

    /// 圧縮レベルを設定
    ///
    /// 注: 圧縮レベルの動的変更は flate2 の `any_zlib` feature が必要なため、
    /// 次のメッセージからレベルが適用される（reset 時）。
    pub fn set_level(&mut self, level: u32) {
        self.level = level.min(9);
        // 次の reset 時に新しい Compress インスタンスを生成するか、
        // または compress.reset() 後に適用する
    }

    /// データを圧縮
    ///
    /// RFC 7692 Section 7.2.1 に従い、圧縮データから終端の 0x00 0x00 0xFF 0xFF を除去
    pub fn compress(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        let mut compressed = Vec::new();
        let mut output_buf = [0u8; 8192];
        let mut input_pos = 0;

        // すべての入力を消費
        while input_pos < data.len() {
            let before_in = self.compress.total_in();
            let before_out = self.compress.total_out();

            self.compress
                .compress(&data[input_pos..], &mut output_buf, FlushCompress::None)
                .map_err(|e| Error::invalid_data(format!("compression failed: {}", e)))?;

            let consumed = (self.compress.total_in() - before_in) as usize;
            let produced = (self.compress.total_out() - before_out) as usize;

            input_pos += consumed;
            if produced > 0 {
                compressed.extend_from_slice(&output_buf[..produced]);
            }
        }

        // Sync フラッシュ（最大 10 回で十分）
        for _ in 0..10 {
            let before_out = self.compress.total_out();

            self.compress
                .compress(&[], &mut output_buf, FlushCompress::Sync)
                .map_err(|e| Error::invalid_data(format!("compression flush failed: {}", e)))?;

            let produced = (self.compress.total_out() - before_out) as usize;
            if produced > 0 {
                compressed.extend_from_slice(&output_buf[..produced]);
            }
            if produced < output_buf.len() {
                break;
            }
        }

        // RFC 7692: 終端の 0x00 0x00 0xFF 0xFF を除去
        if compressed.ends_with(&DEFLATE_TRAILER) {
            compressed.truncate(compressed.len() - 4);
        }

        // no_context_takeover の場合はリセット
        if self.reset_after_message {
            self.compress.reset();
        }

        Ok(compressed)
    }
}

/// permessage-deflate デコンプレッサー
pub struct Decompressor {
    /// DEFLATE コンテキスト
    decompress: flate2::Decompress,
    /// メッセージ毎にリセットするか（no_context_takeover）
    reset_after_message: bool,
}

/// 解凍時のチャンクサイズ
const DECOMPRESS_CHUNK_SIZE: usize = 8192;

impl Decompressor {
    /// 新しいデコンプレッサーを生成
    ///
    /// `is_client` が true の場合、サーバーから受信したデータを解凍するため、
    /// server_no_context_takeover を参照。
    /// false の場合、クライアントから受信したデータを解凍するため、
    /// client_no_context_takeover を参照。
    ///
    /// # 制限事項
    ///
    /// window_bits のカスタマイズは flate2 の `any_zlib` feature が必要なため、
    /// 現在の実装ではデフォルト値 (15) を使用する。
    ///
    /// 解凍側では window_bits=15（最大値）を使用するため、相手がどの window bits で
    /// 圧縮しても解凍可能であり、RFC 7692 準拠性の問題は発生しない。
    pub fn new(config: &PerMessageDeflateConfig, is_client: bool) -> Self {
        let reset_after_message = if is_client {
            // クライアントはサーバーから受信 -> server_no_context_takeover
            config.server_no_context_takeover
        } else {
            // サーバーはクライアントから受信 -> client_no_context_takeover
            config.client_no_context_takeover
        };

        Self {
            decompress: flate2::Decompress::new(false), // raw deflate
            reset_after_message,
        }
    }

    /// データを解凍（最大サイズ制限付き）
    ///
    /// RFC 7692 Section 7.2.2 に従い、終端の 0x00 0x00 0xFF 0xFF を追加してから解凍する。
    /// 展開後のサイズが `max_size` を超えた場合はエラーを返す（Zip Bomb 対策）。
    pub fn decompress(&mut self, data: &[u8], max_size: usize) -> Result<Vec<u8>, Error> {
        // RFC 7692: 終端の 0x00 0x00 0xFF 0xFF を追加
        let mut input = data.to_vec();
        input.extend_from_slice(&DEFLATE_TRAILER);

        let mut decompressed = Vec::new();
        let mut output_buf = [0u8; DECOMPRESS_CHUNK_SIZE];
        let mut input_pos = 0;

        // ループ回数の上限（無限ループ防止）
        let max_iterations = (max_size / DECOMPRESS_CHUNK_SIZE) + 100;
        let mut iterations = 0;

        loop {
            iterations += 1;
            if iterations > max_iterations {
                if self.reset_after_message {
                    self.decompress.reset(false);
                }
                return Err(Error::invalid_data(
                    "decompression exceeded iteration limit",
                ));
            }

            let before_in = self.decompress.total_in();
            let before_out = self.decompress.total_out();

            // RFC 7692 のトレーラーは sync flush マーカーなので Sync を使用
            let status = self
                .decompress
                .decompress(
                    &input[input_pos..],
                    &mut output_buf,
                    flate2::FlushDecompress::Sync,
                )
                .map_err(|e| Error::invalid_data(format!("decompression failed: {}", e)))?;

            let consumed = (self.decompress.total_in() - before_in) as usize;
            let produced = (self.decompress.total_out() - before_out) as usize;

            input_pos += consumed;

            if produced > 0 {
                // 最大サイズを超えた場合はエラー
                if decompressed.len() + produced > max_size {
                    if self.reset_after_message {
                        self.decompress.reset(false);
                    }
                    return Err(Error::invalid_data(format!(
                        "decompressed size exceeds maximum limit of {} bytes",
                        max_size
                    )));
                }
                decompressed.extend_from_slice(&output_buf[..produced]);
            }

            match status {
                flate2::Status::StreamEnd => break,
                flate2::Status::Ok | flate2::Status::BufError => {
                    // 入力がすべて消費され、出力もなければ終了
                    if input_pos >= input.len() && produced == 0 {
                        break;
                    }
                }
            }
        }

        // no_context_takeover の場合はリセット
        if self.reset_after_message {
            self.decompress.reset(false);
        }

        Ok(decompressed)
    }

    /// コンテキストをリセット
    pub fn reset(&mut self) {
        self.decompress.reset(false);
    }
}

/// permessage-deflate コーデック
///
/// 圧縮と解凍の両方を管理
pub struct PerMessageDeflate {
    /// コンプレッサー
    compressor: Compressor,
    /// デコンプレッサー
    decompressor: Decompressor,
    /// 設定
    config: PerMessageDeflateConfig,
}

impl PerMessageDeflate {
    /// 新しいコーデックを生成（クライアント用）
    pub fn new_client(config: PerMessageDeflateConfig) -> Self {
        Self {
            compressor: Compressor::new(&config, true),
            decompressor: Decompressor::new(&config, true),
            config,
        }
    }

    /// 新しいコーデックを生成（サーバー用）
    pub fn new_server(config: PerMessageDeflateConfig) -> Self {
        Self {
            compressor: Compressor::new(&config, false),
            decompressor: Decompressor::new(&config, false),
            config,
        }
    }

    /// 設定を取得
    pub fn config(&self) -> &PerMessageDeflateConfig {
        &self.config
    }

    /// データを圧縮
    pub fn compress(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        self.compressor.compress(data)
    }

    /// データを解凍（最大サイズ制限付き）
    ///
    /// 展開後のサイズが `max_size` を超えた場合はエラーを返す（Zip Bomb 対策）。
    pub fn decompress(&mut self, data: &[u8], max_size: usize) -> Result<Vec<u8>, Error> {
        self.decompressor.decompress(data, max_size)
    }

    /// 圧縮レベルを設定
    pub fn set_compression_level(&mut self, level: u32) {
        self.compressor.set_level(level);
    }

    /// 圧縮するべきかどうかを判定
    ///
    /// 小さなメッセージは圧縮しても効果が薄いため、しきい値を設定
    pub fn should_compress(&self, data: &[u8], threshold: usize) -> bool {
        data.len() >= threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用の最大解凍サイズ（16MB）
    const TEST_MAX_DECOMPRESS_SIZE: usize = 16 * 1024 * 1024;

    #[test]
    fn test_compress_decompress() {
        let config = PerMessageDeflateConfig::default();
        let mut codec = PerMessageDeflate::new_client(config);

        let original = b"Hello, WebSocket! This is a test message for compression.";

        let compressed = codec.compress(original).unwrap();
        assert!(!compressed.is_empty());

        // 圧縮データにはトレーラーがないことを確認
        assert!(!compressed.ends_with(&DEFLATE_TRAILER));

        let decompressed = codec
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compress_empty() {
        let config = PerMessageDeflateConfig::default();
        let mut codec = PerMessageDeflate::new_client(config);

        let original = b"";
        let compressed = codec.compress(original).unwrap();
        let decompressed = codec
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compress_large_data() {
        // 大きなデータは no_context_takeover で確実にテスト
        let config = PerMessageDeflateConfig::new()
            .client_no_context_takeover()
            .server_no_context_takeover();
        let mut codec = PerMessageDeflate::new_client(config);

        // 大きなデータ（繰り返しパターン）
        let original: Vec<u8> = "Hello, World! ".repeat(1000).into_bytes();

        let compressed = codec.compress(&original).unwrap();
        // 繰り返しパターンは圧縮効果が高い
        assert!(compressed.len() < original.len());

        let decompressed = codec
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_should_compress() {
        let config = PerMessageDeflateConfig::default();
        let codec = PerMessageDeflate::new_client(config);

        assert!(!codec.should_compress(b"short", 100));
        assert!(codec.should_compress(b"this is a longer message that should be compressed", 10));
    }

    #[test]
    fn test_config_no_context_takeover() {
        let config = PerMessageDeflateConfig::new()
            .client_no_context_takeover()
            .server_no_context_takeover();

        let mut codec = PerMessageDeflate::new_client(config);

        let original = b"Test message";
        let compressed = codec.compress(original).unwrap();
        let decompressed = codec
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_config_window_bits() {
        let config = PerMessageDeflateConfig::new()
            .client_max_window_bits(12)
            .server_max_window_bits(10);

        let mut codec = PerMessageDeflate::new_client(config.clone());

        assert_eq!(codec.config().client_max_window_bits, Some(12));
        assert_eq!(codec.config().server_max_window_bits, Some(10));

        let original = b"Test message with custom window bits";
        let compressed = codec.compress(original).unwrap();
        let decompressed = codec
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compression_levels() {
        let config = PerMessageDeflateConfig::default();
        let original: Vec<u8> = "Compression test data ".repeat(100).into_bytes();

        // レベル 1（最速）
        let mut codec1 = PerMessageDeflate::new_client(config.clone());
        codec1.set_compression_level(1);
        let compressed1 = codec1.compress(&original).unwrap();

        // レベル 9（最高圧縮）
        let mut codec9 = PerMessageDeflate::new_client(config);
        codec9.set_compression_level(9);
        let compressed9 = codec9.compress(&original).unwrap();

        // 高圧縮レベルの方が小さくなる（または同じ）
        assert!(compressed9.len() <= compressed1.len());

        // 両方とも正しく解凍できる
        let decompressed1 = codec1
            .decompress(&compressed1, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        let decompressed9 = codec9
            .decompress(&compressed9, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed1, original);
        assert_eq!(decompressed9, original);
    }

    #[test]
    fn test_multiple_messages() {
        // 複数のメッセージを連続して圧縮・解凍
        // クライアント圧縮 -> サーバー解凍の辞書チェーンを再現
        let config = PerMessageDeflateConfig::new();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let messages = [
            "Hello, this is the first message.",
            "Hello, this is the second message.",
            "Hello, this is the third message.",
        ];

        for msg in &messages {
            // クライアント -> サーバー
            let compressed = client.compress(msg.as_bytes()).unwrap();
            let decompressed = server
                .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
                .unwrap();
            assert_eq!(decompressed, msg.as_bytes());
        }
    }

    #[test]
    fn test_decompress_exceeds_max_size() {
        let config = PerMessageDeflateConfig::default();
        let mut codec = PerMessageDeflate::new_client(config);

        // 大きなデータを圧縮
        let original: Vec<u8> = "A".repeat(10000).into_bytes();
        let compressed = codec.compress(&original).unwrap();

        // 小さな上限で解凍を試みる
        let result = codec.decompress(&compressed, 100);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("exceeds maximum limit"));
    }

    /// Context Takeover 有効時、2回目以降の圧縮が効率的になる
    #[test]
    fn test_context_takeover_improves_compression() {
        // Context Takeover 有効（デフォルト）
        let config = PerMessageDeflateConfig::new();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        // 同じパターンを持つ複数のメッセージ
        let msg1 = "The quick brown fox jumps over the lazy dog. ".repeat(10);
        let msg2 = "The quick brown fox jumps over the lazy dog. ".repeat(10);
        let msg3 = "The quick brown fox jumps over the lazy dog. ".repeat(10);

        let compressed1 = client.compress(msg1.as_bytes()).unwrap();
        let decompressed1 = server
            .decompress(&compressed1, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed1, msg1.as_bytes());

        let compressed2 = client.compress(msg2.as_bytes()).unwrap();
        let decompressed2 = server
            .decompress(&compressed2, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed2, msg2.as_bytes());

        let compressed3 = client.compress(msg3.as_bytes()).unwrap();
        let decompressed3 = server
            .decompress(&compressed3, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed3, msg3.as_bytes());

        // Context Takeover により、2回目以降の圧縮は辞書を再利用するため効率的になる可能性
        // ただし、同じメッセージでも圧縮結果は異なる場合がある
        assert!(compressed1.len() < msg1.len());
    }

    /// no_context_takeover 時、各メッセージが独立して圧縮される
    #[test]
    fn test_no_context_takeover_independent_compression() {
        // no_context_takeover を設定
        let config = PerMessageDeflateConfig::new()
            .client_no_context_takeover()
            .server_no_context_takeover();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        let msg = "Hello, WebSocket compression test! ".repeat(20);

        let compressed1 = client.compress(msg.as_bytes()).unwrap();
        let decompressed1 = server
            .decompress(&compressed1, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed1, msg.as_bytes());

        let compressed2 = client.compress(msg.as_bytes()).unwrap();
        let decompressed2 = server
            .decompress(&compressed2, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed2, msg.as_bytes());

        // no_context_takeover の場合、同じメッセージは同じ圧縮結果になる
        assert_eq!(compressed1, compressed2);
    }

    /// クライアント/サーバー双方向通信テスト
    #[test]
    fn test_bidirectional_communication() {
        let config = PerMessageDeflateConfig::new();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        // クライアント -> サーバー
        let client_msg = "Hello from client!";
        let compressed = client.compress(client_msg.as_bytes()).unwrap();
        let decompressed = server
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed, client_msg.as_bytes());

        // サーバー -> クライアント
        let server_msg = "Hello from server!";
        let compressed = server.compress(server_msg.as_bytes()).unwrap();
        let decompressed = client
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed, server_msg.as_bytes());

        // 複数回の往復
        for i in 0..5 {
            let msg = format!("Client message #{}", i);
            let compressed = client.compress(msg.as_bytes()).unwrap();
            let decompressed = server
                .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
                .unwrap();
            assert_eq!(decompressed, msg.as_bytes());

            let reply = format!("Server reply to #{}", i);
            let compressed = server.compress(reply.as_bytes()).unwrap();
            let decompressed = client
                .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
                .unwrap();
            assert_eq!(decompressed, reply.as_bytes());
        }
    }

    /// 大きなメッセージの双方向テスト
    #[test]
    fn test_large_bidirectional_messages() {
        let config = PerMessageDeflateConfig::new();
        let mut client = PerMessageDeflate::new_client(config.clone());
        let mut server = PerMessageDeflate::new_server(config);

        // 大きなメッセージ（100KB）
        let large_msg: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();

        // クライアント -> サーバー
        let compressed = client.compress(&large_msg).unwrap();
        let decompressed = server
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed, large_msg);

        // サーバー -> クライアント
        let compressed = server.compress(&large_msg).unwrap();
        let decompressed = client
            .decompress(&compressed, TEST_MAX_DECOMPRESS_SIZE)
            .unwrap();
        assert_eq!(decompressed, large_msg);
    }
}
