//! permessage-deflate 圧縮拡張 (RFC 7692)
//!
//! WebSocket メッセージの DEFLATE 圧縮/解凍を提供する。

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

use crate::error::Error;
use crate::websocket_extension::PerMessageDeflateConfig;

/// DEFLATE 圧縮終端バイト列
/// RFC 7692 Section 7.2.1: 0x00 0x00 0xFF 0xFF
const DEFLATE_TRAILER: [u8; 4] = [0x00, 0x00, 0xFF, 0xFF];

/// permessage-deflate コンプレッサー
pub struct Compressor {
    /// 圧縮レベル (0-9)
    level: u32,
    /// コンテキスト引き継ぎなし
    no_context_takeover: bool,
    /// 圧縮状態（コンテキスト引き継ぎ用）
    state: Option<Vec<u8>>,
}

impl Compressor {
    /// 新しいコンプレッサーを生成
    pub fn new(config: &PerMessageDeflateConfig, is_client: bool) -> Self {
        let no_context_takeover = if is_client {
            config.client_no_context_takeover
        } else {
            config.server_no_context_takeover
        };

        Self {
            level: 6, // デフォルト圧縮レベル
            no_context_takeover,
            state: None,
        }
    }

    /// 圧縮レベルを設定
    pub fn set_level(&mut self, level: u32) {
        self.level = level.min(9);
    }

    /// データを圧縮
    ///
    /// RFC 7692 Section 7.2.1 に従い、圧縮データから終端の 0x00 0x00 0xFF 0xFF を除去
    pub fn compress(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        let compression = Compression::new(self.level);
        let mut encoder = DeflateEncoder::new(Vec::new(), compression);

        encoder
            .write_all(data)
            .map_err(|e| Error::invalid_data(format!("compression failed: {}", e)))?;

        let mut compressed = encoder
            .finish()
            .map_err(|e| Error::invalid_data(format!("compression failed: {}", e)))?;

        // RFC 7692: 終端の 0x00 0x00 0xFF 0xFF を除去
        if compressed.ends_with(&DEFLATE_TRAILER) {
            compressed.truncate(compressed.len() - 4);
        }

        // コンテキスト引き継ぎ
        if !self.no_context_takeover {
            // 現在の実装では毎回新しいエンコーダーを使用するため、
            // コンテキスト引き継ぎは限定的
            self.state = Some(compressed.clone());
        }

        Ok(compressed)
    }
}

/// permessage-deflate デコンプレッサー
pub struct Decompressor {
    /// コンテキスト引き継ぎなし
    no_context_takeover: bool,
    /// 解凍状態（コンテキスト引き継ぎ用）
    state: Option<Vec<u8>>,
}

impl Decompressor {
    /// 新しいデコンプレッサーを生成
    pub fn new(config: &PerMessageDeflateConfig, is_client: bool) -> Self {
        // クライアントは受信時にサーバーの設定を使用
        let no_context_takeover = if is_client {
            config.server_no_context_takeover
        } else {
            config.client_no_context_takeover
        };

        Self {
            no_context_takeover,
            state: None,
        }
    }

    /// データを解凍
    ///
    /// RFC 7692 Section 7.2.2 に従い、終端の 0x00 0x00 0xFF 0xFF を追加してから解凍
    pub fn decompress(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        // RFC 7692: 終端の 0x00 0x00 0xFF 0xFF を追加
        let mut input = data.to_vec();
        input.extend_from_slice(&DEFLATE_TRAILER);

        let mut decoder = DeflateDecoder::new(&input[..]);
        let mut decompressed = Vec::new();

        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| Error::invalid_data(format!("decompression failed: {}", e)))?;

        // コンテキスト引き継ぎ
        if !self.no_context_takeover {
            self.state = Some(decompressed.clone());
        }

        Ok(decompressed)
    }

    /// コンテキストをリセット
    pub fn reset(&mut self) {
        self.state = None;
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

    /// データを解凍
    pub fn decompress(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        self.decompressor.decompress(data)
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

    #[test]
    fn test_compress_decompress() {
        let config = PerMessageDeflateConfig::default();
        let mut codec = PerMessageDeflate::new_client(config);

        let original = b"Hello, WebSocket! This is a test message for compression.";

        let compressed = codec.compress(original).unwrap();
        assert!(!compressed.is_empty());

        // 圧縮データにはトレーラーがないことを確認
        assert!(!compressed.ends_with(&DEFLATE_TRAILER));

        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compress_empty() {
        let config = PerMessageDeflateConfig::default();
        let mut codec = PerMessageDeflate::new_client(config);

        let original = b"";
        let compressed = codec.compress(original).unwrap();
        let decompressed = codec.decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compress_large_data() {
        let config = PerMessageDeflateConfig::default();
        let mut codec = PerMessageDeflate::new_client(config);

        // 大きなデータ（繰り返しパターン）
        let original: Vec<u8> = "Hello, World! ".repeat(1000).into_bytes();

        let compressed = codec.compress(&original).unwrap();
        // 繰り返しパターンは圧縮効果が高い
        assert!(compressed.len() < original.len());

        let decompressed = codec.decompress(&compressed).unwrap();
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
        let decompressed = codec.decompress(&compressed).unwrap();
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
        let decompressed = codec.decompress(&compressed).unwrap();
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
        let decompressed1 = codec1.decompress(&compressed1).unwrap();
        let decompressed9 = codec9.decompress(&compressed9).unwrap();
        assert_eq!(decompressed1, original);
        assert_eq!(decompressed9, original);
    }
}
