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
    /// no_context_takeover が有効な場合、次のメッセージから新しいレベルが適用される。
    /// no_context_takeover が無効な場合、圧縮コンテキストの維持が必要なため
    /// レベル変更は適用されない。
    pub fn set_level(&mut self, level: u32) {
        self.level = level.min(9);
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

        // no_context_takeover の場合はリセット（set_level() の変更もここで反映される）
        if self.reset_after_message {
            self.compress = Compress::new(Compression::new(self.level), false);
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
