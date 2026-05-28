//! WebSocket クライアント接続 (Sans I/O パターン)
//!
//! WebSocket クライアント接続を管理する状態機械。
//! I/O は外部で行い、この構造体はバッファ駆動型で動作する。

use crate::Timestamp;
use crate::deflate::PerMessageDeflate;
use crate::error::Error;
use crate::websocket_close::CloseCode;
use crate::websocket_connection_shared::{
    ClientFramePolicy, DEFAULT_MAX_DECOMPRESSED_SIZE, DEFAULT_MAX_FRAME_SIZE,
    DEFAULT_MAX_MESSAGE_SIZE, SharedConnectionState,
};
use crate::websocket_connection_types::{
    ConnectionEvent, ConnectionOutput, ConnectionState, RandomSource, TimerId,
};
use crate::websocket_extension::{Extension, PerMessageDeflateConfig};
use crate::websocket_handshake::{HandshakeRequest, HandshakeResponse, HandshakeValidator};
use crate::websocket_opcode::Opcode;

/// 接続オプション
#[derive(Debug, Clone)]
pub struct ClientConnectionOptions {
    /// リクエストパス
    pub path: String,
    /// Host ヘッダー
    pub host: String,
    /// Origin ヘッダー（オプション）
    pub origin: Option<String>,
    /// サブプロトコル候補
    pub protocols: Vec<String>,
    /// permessage-deflate 設定（オプション）
    pub deflate_config: Option<PerMessageDeflateConfig>,
    /// 追加ヘッダー
    pub additional_headers: Vec<(String, String)>,
    /// Ping 間隔（ミリ秒、0 で無効）
    pub ping_interval_millis: u64,
    /// Pong タイムアウト（ミリ秒）
    pub pong_timeout_millis: u64,
    /// クローズタイムアウト（ミリ秒）
    pub close_timeout_millis: u64,
    /// 最大フレームサイズ（メモリ DoS 対策）
    pub max_frame_size: usize,
    /// 最大メッセージサイズ（フラグメント累積サイズ制限）
    pub max_message_size: usize,
    /// 最大解凍サイズ（Zip Bomb 対策）
    pub max_decompressed_size: usize,
}

impl Default for ClientConnectionOptions {
    fn default() -> Self {
        Self {
            path: "/".to_string(),
            host: "localhost".to_string(),
            origin: None,
            protocols: Vec::new(),
            deflate_config: None,
            additional_headers: Vec::new(),
            ping_interval_millis: 30_000, // 30秒
            pong_timeout_millis: 10_000,  // 10秒
            close_timeout_millis: 5_000,  // 5秒
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
            max_decompressed_size: DEFAULT_MAX_DECOMPRESSED_SIZE,
        }
    }
}

impl ClientConnectionOptions {
    /// 新しい接続オプションを生成
    pub fn new(host: &str, path: &str) -> Self {
        Self {
            path: path.to_string(),
            host: host.to_string(),
            ..Default::default()
        }
    }

    /// Origin を設定
    pub fn origin(mut self, origin: &str) -> Self {
        self.origin = Some(origin.to_string());
        self
    }

    /// サブプロトコルを追加
    pub fn protocol(mut self, protocol: &str) -> Self {
        self.protocols.push(protocol.to_string());
        self
    }

    /// permessage-deflate を有効化
    pub fn deflate(mut self, config: PerMessageDeflateConfig) -> Self {
        self.deflate_config = Some(config);
        self
    }

    /// 追加ヘッダーを設定
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.additional_headers
            .push((name.to_string(), value.to_string()));
        self
    }

    /// Ping 間隔を設定
    pub fn ping_interval(mut self, millis: u64) -> Self {
        self.ping_interval_millis = millis;
        self
    }

    /// 最大フレームサイズを設定（メモリ DoS 対策）
    pub fn max_frame_size(mut self, size: usize) -> Self {
        self.max_frame_size = size;
        self
    }

    /// 最大メッセージサイズを設定（フラグメント累積サイズ制限）
    pub fn max_message_size(mut self, size: usize) -> Self {
        self.max_message_size = size;
        self
    }

    /// 最大解凍サイズを設定（Zip Bomb 対策）
    pub fn max_decompressed_size(mut self, size: usize) -> Self {
        self.max_decompressed_size = size;
        self
    }
}

/// WebSocket 接続
pub struct WebSocketClientConnection<R: RandomSource> {
    /// 共通接続状態
    shared: SharedConnectionState,
    /// クライアント側フレームポリシー
    policy: ClientFramePolicy<R>,
    /// オプション
    options: ClientConnectionOptions,
    /// ハンドシェイク用 nonce
    nonce: [u8; 16],
    /// ハンドシェイクバリデーター
    handshake_validator: Option<HandshakeValidator>,
    /// ネゴシエートされたサブプロトコル
    negotiated_protocol: Option<String>,
    /// ネゴシエートされた拡張
    negotiated_extensions: Vec<String>,
}

impl<R: RandomSource> WebSocketClientConnection<R> {
    /// 新しい接続を作成
    ///
    /// # Arguments
    /// * `options` - 接続オプション
    /// * `random` - 乱数ソース（`RandomSource` トレイトを実装した型）
    ///
    /// # Example
    /// ```ignore
    /// // 本番環境: 暗号学的に安全な乱数を使用
    /// let ws = WebSocketClientConnection::new(options, SecureRandom);
    ///
    /// // テスト環境: 固定値を使用
    /// let ws = WebSocketClientConnection::new(options, FixedRandom {
    ///     masking_key: [0xAB, 0xCD, 0xEF, 0x12],
    ///     nonce: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
    /// });
    /// ```
    pub fn new(options: ClientConnectionOptions, random: R) -> Self {
        let shared = SharedConnectionState::new(
            options.max_frame_size,
            options.max_message_size,
            options.max_decompressed_size,
            options.ping_interval_millis,
            options.pong_timeout_millis,
            options.close_timeout_millis,
        );
        Self {
            shared,
            policy: ClientFramePolicy::new(random),
            options,
            nonce: [0u8; 16], // connect() で設定
            handshake_validator: None,
            negotiated_protocol: None,
            negotiated_extensions: Vec::new(),
        }
    }

    /// 現在の状態を取得
    pub fn state(&self) -> ConnectionState {
        self.shared.state
    }

    /// ネゴシエートされたサブプロトコルを取得
    pub fn protocol(&self) -> Option<&str> {
        self.negotiated_protocol.as_deref()
    }

    /// ネゴシエートされた拡張を取得
    pub fn extensions(&self) -> &[String] {
        &self.negotiated_extensions
    }

    /// 接続を開始
    pub fn connect(&mut self) -> Result<(), Error> {
        if self.shared.state != ConnectionState::Disconnected {
            return Err(Error::invalid_state("already connected or connecting"));
        }

        // ハンドシェイク用の nonce を生成
        self.nonce = self.policy.nonce();

        // ハンドシェイクリクエストを構築
        let mut request = HandshakeRequest::new(&self.options.path, &self.options.host);

        if let Some(origin) = &self.options.origin {
            request = request.origin(origin);
        }

        for protocol in &self.options.protocols {
            request = request.protocol(protocol);
        }

        // permessage-deflate 拡張
        if let Some(deflate_config) = &self.options.deflate_config {
            let ext = deflate_config.to_extension();
            request = request.extension(&ext.encode());
        }

        for (name, value) in &self.options.additional_headers {
            request = request.header(name, value);
        }

        let encoded = request.build(self.nonce)?;

        // ハンドシェイクバリデーターを作成
        self.handshake_validator = Some(HandshakeValidator::new(self.nonce));

        // 送信キューに追加
        self.shared
            .output_queue
            .push_back(ConnectionOutput::SendData(encoded));

        // 状態遷移
        self.shared.set_state(ConnectionState::Connecting);

        Ok(())
    }

    /// 受信データを処理
    ///
    /// RFC 6455 Section 7.1.7: このメソッドが Err を返した後は
    /// 以降の呼び出しも即座に Err を返す。
    pub fn feed_recv_buf(&mut self, buf: &[u8], now: Timestamp) -> Result<(), Error> {
        if self.shared.failed {
            return Err(Error::invalid_state("connection has failed"));
        }
        let result = match self.shared.state {
            ConnectionState::Connecting => self.process_handshake(buf, now),
            ConnectionState::Connected | ConnectionState::Closing => {
                self.shared.process_frames(buf, &mut self.policy)
            }
            ConnectionState::Disconnected | ConnectionState::Closed => {
                return Err(Error::invalid_state("connection is closed"));
            }
        };
        if result.is_err() {
            self.shared.failed = true;
        }
        result
    }

    /// テキストメッセージを送信
    pub fn send_text(&mut self, text: &str) -> Result<(), Error> {
        self.shared.check_connected()?;
        self.shared
            .send_data_frame(Opcode::Text, text.as_bytes().to_vec(), &mut self.policy)
    }

    /// バイナリメッセージを送信
    pub fn send_binary(&mut self, data: &[u8]) -> Result<(), Error> {
        self.shared.check_connected()?;
        self.shared
            .send_data_frame(Opcode::Binary, data.to_vec(), &mut self.policy)
    }

    /// Ping を送信
    ///
    /// RFC 6455 Section 5.5: data は 125 バイト以下でなければならない
    pub fn send_ping(&mut self, data: &[u8]) -> Result<(), Error> {
        self.shared.check_connected()?;
        self.shared.send_ping_internal(data, &mut self.policy)
    }

    /// 接続をクローズ
    ///
    /// RFC 6455 Section 7.4.1: 送信禁止のクローズコード (1005, 1006, 1015) は拒否される
    /// RFC 6455 Section 5.5: reason は 123 バイト以下でなければならない
    /// RFC 6455 Section 7.1.2: Close フレームは established connection 上でのみ送信可能
    ///
    /// 検証ロジックの実装はクライアント / サーバー間で共通化されている。
    pub fn close(&mut self, code: CloseCode, reason: &str) -> Result<(), Error> {
        self.shared.close(code, reason, &mut self.policy)
    }

    /// タイマーイベントを処理
    pub fn handle_timer(&mut self, timer_id: TimerId) -> Result<(), Error> {
        self.shared.handle_timer(timer_id, &mut self.policy)
    }

    /// イベントを取得
    pub fn poll_event(&mut self) -> Option<ConnectionEvent> {
        self.shared.poll_event()
    }

    /// 出力を取得
    pub fn poll_output(&mut self) -> Option<ConnectionOutput> {
        self.shared.poll_output()
    }

    // === 内部メソッド ===

    fn process_handshake(&mut self, buf: &[u8], now: Timestamp) -> Result<(), Error> {
        let validator = self
            .handshake_validator
            .as_mut()
            .ok_or_else(|| Error::invalid_state("handshake validator not initialized"))?;

        validator.feed(buf);

        let result = validator.validate()?;

        if let Some(response) = result {
            // 残りのデータを先に取得
            let remaining = self
                .handshake_validator
                .as_ref()
                .map(|v| v.remaining().to_vec())
                .unwrap_or_default();

            self.complete_handshake(response, now)?;
            self.handshake_validator = None;

            // ハンドシェイク完了後、残りのデータがあれば即時処理する
            // (101 応答と最初のフレームが同一受信バッファに含まれる場合)
            if !remaining.is_empty() {
                self.shared.process_frames(&remaining, &mut self.policy)?;
            }
        }

        Ok(())
    }

    fn complete_handshake(
        &mut self,
        response: HandshakeResponse,
        _now: Timestamp,
    ) -> Result<(), Error> {
        // RFC 6455 Section 4.2.2: サーバーが返したプロトコルが要求に含まれているか検証
        if let Some(ref protocol) = response.protocol
            && !self.options.protocols.iter().any(|p| p == protocol)
        {
            return Err(Error::handshake_rejected(format!(
                "server returned unsolicited protocol: {}",
                protocol
            )));
        }

        // RFC 6455 Section 4.2.2: サーバーが返した拡張が要求に含まれているか検証
        let requested_extension_names: Vec<&str> = self
            .options
            .deflate_config
            .as_ref()
            .map(|_| vec!["permessage-deflate"])
            .unwrap_or_default();

        // RFC 6455 Section 9.1: ABNF に不適合なら接続を失敗させる (MUST)
        for ext_str in &response.extensions {
            let extensions = Extension::parse_strict(ext_str).map_err(|e| {
                Error::handshake_rejected(format!("invalid Sec-WebSocket-Extensions value: {}", e))
            })?;
            for ext in &extensions {
                if !requested_extension_names.contains(&ext.name.as_str()) {
                    return Err(Error::handshake_rejected(format!(
                        "server returned unsolicited extension: {}",
                        ext.name
                    )));
                }
            }
        }

        self.negotiated_protocol = response.protocol;
        self.negotiated_extensions = response.extensions.clone();

        // permessage-deflate のネゴシエーション結果を解析し、コーデックを作成
        // RFC 7692 Section 7.1.2: クライアントがリクエストした拡張に対して
        // サーバーが不正なレスポンスを返した場合は接続失敗
        // RFC 6455 Section 9.1: 上の検証で ABNF 適合性は確認済み
        // RFC 7692 Section 5 / Section 7.1.3: サーバーは offer の中から 1 つを選んで受諾する。
        // 複数の permessage-deflate 要素を含むレスポンスは不正とみなす。
        let pmce_count = response
            .extensions
            .iter()
            .flat_map(|s| Extension::parse_strict(s).into_iter().flatten())
            .filter(|e| e.name == "permessage-deflate")
            .count();
        if pmce_count > 1 {
            return Err(Error::handshake_rejected(
                "server returned multiple permessage-deflate elements",
            ));
        }
        for ext_str in &response.extensions {
            let extensions = Extension::parse_strict(ext_str).map_err(|e| {
                Error::handshake_rejected(format!("invalid Sec-WebSocket-Extensions value: {}", e))
            })?;
            for ext in extensions {
                if ext.name == "permessage-deflate" {
                    match PerMessageDeflateConfig::from_extension_for_client_response(&ext) {
                        Ok(config) => {
                            if let Some(deflate_config) = &self.options.deflate_config {
                                // RFC 7692 Section 7.1.2.2: クライアントが offer していない
                                // client_max_window_bits をサーバーが含めた場合は拒否
                                let client_offered_cmwb =
                                    deflate_config.client_max_window_bits.is_some();
                                let server_included_cmwb =
                                    ext.get_param("client_max_window_bits").is_some();

                                if server_included_cmwb && !client_offered_cmwb {
                                    return Err(Error::handshake_rejected(
                                        "server included client_max_window_bits without client offer",
                                    ));
                                }

                                // RFC 7692 Section 7.1.2.1: server_max_window_bits は
                                // クライアントの offer 以下でなければならない
                                if let (Some(client_smwb), Some(server_smwb)) = (
                                    deflate_config.server_max_window_bits,
                                    config.server_max_window_bits,
                                ) && server_smwb > client_smwb
                                {
                                    return Err(Error::handshake_rejected(format!(
                                        "server_max_window_bits {} exceeds client offer {}",
                                        server_smwb, client_smwb
                                    )));
                                }

                                // RFC 7692 Section 7.1.2.2: client_max_window_bits は
                                // クライアントの offer 以下でなければならない
                                if let (Some(client_cmwb), Some(server_cmwb)) = (
                                    deflate_config.client_max_window_bits,
                                    config.client_max_window_bits,
                                ) && server_cmwb > client_cmwb
                                {
                                    return Err(Error::handshake_rejected(format!(
                                        "client_max_window_bits {} exceeds client offer {}",
                                        server_cmwb, client_cmwb
                                    )));
                                }

                                // RFC 7692 Section 7.2.1: 合意した window bits で圧縮する必要がある
                                // 現在の実装では window_bits=15 固定 (noflate の制約) のため、
                                // client_max_window_bits < 15 はサポートしない
                                if let Some(cmwb) = config.client_max_window_bits
                                    && cmwb < 15
                                {
                                    return Err(Error::handshake_rejected(format!(
                                        "client_max_window_bits={} is not supported (only 15 is supported)",
                                        cmwb
                                    )));
                                }
                            }
                            self.shared.deflate = Some(PerMessageDeflate::new_client(config));
                        }
                        Err(e) => {
                            // クライアントがリクエストした拡張なら接続失敗
                            if self.options.deflate_config.is_some() {
                                return Err(Error::handshake_rejected(format!(
                                    "invalid permessage-deflate response: {:?}",
                                    e
                                )));
                            }
                            // リクエストしていない拡張は無視
                        }
                    }
                }
            }
        }

        self.shared.set_state(ConnectionState::Connected);

        self.shared
            .event_queue
            .push_back(ConnectionEvent::Connected {
                protocol: self.negotiated_protocol.clone(),
                extensions: self.negotiated_extensions.clone(),
            });

        // Ping タイマー設定
        if self.options.ping_interval_millis > 0 {
            self.shared
                .output_queue
                .push_back(ConnectionOutput::SetTimer {
                    id: TimerId::Ping,
                    duration_millis: self.options.ping_interval_millis,
                });
        }

        Ok(())
    }
}
