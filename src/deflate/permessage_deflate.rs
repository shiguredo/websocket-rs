//! permessage-deflate 圧縮拡張 (RFC 7692)
//!
//! WebSocket メッセージの DEFLATE 圧縮/解凍を提供する。

use noflate::{Decoder, Encoder};

use crate::error::Error;
use crate::websocket_extension::PerMessageDeflateConfig;

/// DEFLATE 圧縮終端バイト列
/// RFC 7692 Section 7.2.1: 0x00 0x00 0xFF 0xFF
const DEFLATE_TRAILER: [u8; 4] = [0x00, 0x00, 0xFF, 0xFF];

/// 解凍時に feed する入力チャンクサイズ
///
/// チャンクごとに出力サイズを検証することで、Zip Bomb 攻撃時に
/// 内部バッファが無制限に拡大する前に制限超過を検出できる。
const DECOMPRESS_FEED_CHUNK: usize = 8192;

/// permessage-deflate コンプレッサー
pub struct Compressor {
    /// DEFLATE エンコーダー
    encoder: Encoder,
    /// メッセージ毎にコンテキストをリセットするか（no_context_takeover）
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
    /// noflate は LZ77 ウィンドウサイズを 32 KiB（window_bits=15）固定で使用する。
    ///
    /// RFC 7692 Section 7.2.1 では、合意した window bits を超える LZ77 ウィンドウを
    /// 使用してはならないと規定されている。この制約はハンドシェイク時に担保する:
    /// - サーバー: negotiate() で server_max_window_bits を含めない (デフォルト 15)
    /// - クライアント: client_max_window_bits < 15 のレスポンスを拒否する
    pub fn new(config: &PerMessageDeflateConfig, is_client: bool) -> Self {
        let reset_after_message = if is_client {
            config.client_no_context_takeover
        } else {
            config.server_no_context_takeover
        };

        Self {
            encoder: Encoder::new(),
            reset_after_message,
        }
    }

    /// データを圧縮
    ///
    /// RFC 7692 Section 7.2.1 に従い、圧縮データから終端の 0x00 0x00 0xFF 0xFF を除去する。
    pub fn compress(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        self.encoder
            .feed(data)
            .map_err(|e| Error::invalid_data(format!("compression failed: {}", e)))?;
        self.encoder
            .sync_flush()
            .map_err(|e| Error::invalid_data(format!("compression flush failed: {}", e)))?;

        let mut out = self.encoder.output().to_vec();
        self.encoder.advance(out.len());

        // RFC 7692: 終端の 0x00 0x00 0xFF 0xFF を除去
        if out.ends_with(&DEFLATE_TRAILER) {
            out.truncate(out.len() - 4);
        }

        // no_context_takeover の場合は LZ77 履歴をリセット
        if self.reset_after_message {
            self.encoder.reset_history();
        }

        Ok(out)
    }
}

/// permessage-deflate デコンプレッサー
pub struct Decompressor {
    /// DEFLATE デコーダー
    decoder: Decoder,
    /// メッセージ毎にコンテキストをリセットするか（no_context_takeover）
    reset_after_message: bool,
}

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
    /// noflate は LZ77 ウィンドウサイズを 32 KiB（window_bits=15）固定で使用する。
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
            decoder: Decoder::new(),
            reset_after_message,
        }
    }

    /// データを解凍（最大サイズ制限付き）
    ///
    /// RFC 7692 Section 7.2.2 に従い、終端の 0x00 0x00 0xFF 0xFF を追加してから解凍する。
    /// 展開後のサイズが `max_size` を超えた場合はエラーを返す（Zip Bomb 対策）。
    pub fn decompress(&mut self, data: &[u8], max_size: usize) -> Result<Vec<u8>, Error> {
        let mut decompressed = Vec::new();

        // RFC 7692: 入力末尾に 0x00 0x00 0xFF 0xFF を追加
        // 入力をチャンク分割して feed し、都度出力サイズを検証することで
        // Zip Bomb 攻撃時の内部バッファ拡大を制限する
        let feed_chain = data
            .chunks(DECOMPRESS_FEED_CHUNK)
            .chain(core::iter::once(DEFLATE_TRAILER.as_slice()));

        for chunk in feed_chain {
            self.decoder
                .feed(chunk)
                .map_err(|e| Error::invalid_data(format!("decompression failed: {}", e)))?;

            let produced = self.decoder.output();
            if decompressed.len().saturating_add(produced.len()) > max_size {
                self.reset_if_needed();
                return Err(Error::invalid_data(format!(
                    "decompressed size exceeds maximum limit of {} bytes",
                    max_size
                )));
            }

            let produced_len = produced.len();
            decompressed.extend_from_slice(produced);
            self.decoder.advance(produced_len);
        }

        // no_context_takeover の場合は Decoder を作り直してコンテキストをリセット
        if self.reset_after_message {
            self.decoder = Decoder::new();
        }

        Ok(decompressed)
    }

    /// エラー時のリセット処理
    fn reset_if_needed(&mut self) {
        if self.reset_after_message {
            self.decoder = Decoder::new();
        }
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

    /// 圧縮するべきかどうかを判定
    ///
    /// 小さなメッセージは圧縮しても効果が薄いため、しきい値を設定
    pub fn should_compress(&self, data: &[u8], threshold: usize) -> bool {
        data.len() >= threshold
    }
}
