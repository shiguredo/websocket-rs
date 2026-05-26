//! WebSocket クライアント接続 (Sans I/O パターン)
//!
//! WebSocket クライアント接続を管理する状態機械。
//! I/O は外部で行い、この構造体はバッファ駆動型で動作する。

use std::collections::VecDeque;

use crate::Timestamp;
use crate::deflate::PerMessageDeflate;
use crate::error::Error;
use crate::websocket_close::{CloseCode, truncate_reason};
use crate::websocket_extension::{Extension, PerMessageDeflateConfig};
use crate::websocket_frame::{Frame, FrameDecoder};
use crate::websocket_handshake::{HandshakeRequest, HandshakeResponse, HandshakeValidator};
use crate::websocket_opcode::Opcode;

/// 乱数生成のトレイト
///
/// WebSocket クライアントが使用する masking key と nonce を生成するためのトレイト。
/// ライブラリはこのトレイトのみを提供し、実装は利用者側で行う。
///
/// # Example
///
/// ```ignore
/// // 本番環境: 暗号学的に安全な乱数を使用
/// pub struct SecureRandom;
///
/// impl RandomSource for SecureRandom {
///     fn masking_key(&mut self) -> [u8; 4] {
///         let mut key = [0u8; 4];
///         getrandom::fill(&mut key).expect("failed to generate masking key");
///         key
///     }
///
///     fn nonce(&mut self) -> [u8; 16] {
///         let mut nonce = [0u8; 16];
///         getrandom::fill(&mut nonce).expect("failed to generate nonce");
///         nonce
///     }
/// }
///
/// // テスト環境: 固定値を使用
/// pub struct FixedRandom {
///     pub masking_key: [u8; 4],
///     pub nonce: [u8; 16],
/// }
///
/// impl RandomSource for FixedRandom {
///     fn masking_key(&mut self) -> [u8; 4] { self.masking_key }
///     fn nonce(&mut self) -> [u8; 16] { self.nonce }
/// }
/// ```
pub trait RandomSource: Send {
    /// masking key (4 bytes) を生成する
    fn masking_key(&mut self) -> [u8; 4];

    /// nonce (16 bytes) を生成する
    fn nonce(&mut self) -> [u8; 16];
}

/// 接続状態
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionState {
    /// 切断状態
    #[default]
    Disconnected,
    /// ハンドシェイク中
    Connecting,
    /// 接続確立
    Connected,
    /// クローズハンドシェイク中
    Closing,
    /// 切断完了
    Closed,
}

/// タイマー ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimerId {
    /// Ping 送信タイマー
    Ping,
    /// Pong タイムアウト
    PongTimeout,
    /// クローズタイムアウト
    CloseTimeout,
}

/// 接続イベント
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionEvent {
    /// 接続完了
    Connected {
        /// ネゴシエートされたサブプロトコル
        protocol: Option<String>,
        /// ネゴシエートされた拡張
        extensions: Vec<String>,
    },
    /// テキストメッセージ受信
    TextMessage(String),
    /// バイナリメッセージ受信
    BinaryMessage(Vec<u8>),
    /// Ping 受信
    Ping(Vec<u8>),
    /// Pong 受信
    Pong(Vec<u8>),
    /// クローズ受信
    Close {
        code: Option<CloseCode>,
        reason: String,
    },
    /// 状態変化
    StateChanged(ConnectionState),
    /// エラー発生
    Error(String),
}

/// 接続出力アクション
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionOutput {
    /// データ送信
    SendData(Vec<u8>),
    /// タイマー設定
    SetTimer { id: TimerId, duration_millis: u64 },
    /// タイマークリア
    ClearTimer { id: TimerId },
    /// 接続をクローズ
    CloseConnection,
}

/// デフォルトの最大フレームサイズ（64MB）
pub const DEFAULT_MAX_FRAME_SIZE: usize = 64 * 1024 * 1024;

/// デフォルトの最大メッセージサイズ（64MB）
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;

/// デフォルトの最大解凍サイズ（16MB）
pub const DEFAULT_MAX_DECOMPRESSED_SIZE: usize = 16 * 1024 * 1024;

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

/// フラグメント収集バッファ
#[derive(Debug, Default)]
struct FragmentBuffer {
    /// 最初のフレームのオペコード
    opcode: Option<Opcode>,
    /// 収集中のペイロード
    payload: Vec<u8>,
    /// メッセージが圧縮されているか (最初のフレームの RSV1)
    compressed: bool,
}

impl FragmentBuffer {
    fn new() -> Self {
        Self::default()
    }

    fn is_empty(&self) -> bool {
        self.opcode.is_none()
    }

    fn len(&self) -> usize {
        self.payload.len()
    }

    fn start(&mut self, opcode: Opcode, payload: Vec<u8>, compressed: bool) {
        self.opcode = Some(opcode);
        self.payload = payload;
        self.compressed = compressed;
    }

    fn append(&mut self, payload: &[u8]) {
        self.payload.extend_from_slice(payload);
    }

    fn take(&mut self) -> (Opcode, Vec<u8>, bool) {
        let opcode = self.opcode.take().unwrap_or(Opcode::Binary);
        let payload = std::mem::take(&mut self.payload);
        let compressed = self.compressed;
        self.compressed = false;
        (opcode, payload, compressed)
    }

    fn clear(&mut self) {
        self.opcode = None;
        self.payload.clear();
        self.compressed = false;
    }
}

/// WebSocket 接続
pub struct WebSocketClientConnection<R: RandomSource> {
    /// 状態
    state: ConnectionState,
    /// オプション
    options: ClientConnectionOptions,

    /// ハンドシェイク用 nonce
    nonce: [u8; 16],
    /// ハンドシェイクバリデーター
    handshake_validator: Option<HandshakeValidator>,

    /// フレームデコーダー
    frame_decoder: FrameDecoder,
    /// フラグメントバッファ
    fragment_buffer: FragmentBuffer,

    /// ネゴシエートされたサブプロトコル
    negotiated_protocol: Option<String>,
    /// ネゴシエートされた拡張
    negotiated_extensions: Vec<String>,
    /// permessage-deflate コーデック
    deflate: Option<PerMessageDeflate>,

    /// クローズフレームを送信したか
    close_sent: bool,
    /// クローズフレームを受信したか
    close_received: bool,

    /// Pong 待ち
    awaiting_pong: bool,

    /// RFC 6455 Section 7.1.7: 接続失敗フラグ
    ///
    /// feed_recv_buf() が Err を返した後は true になり、
    /// 以降の feed_recv_buf() 呼び出しを即座に Err で弾く。
    failed: bool,

    /// イベントキュー
    event_queue: VecDeque<ConnectionEvent>,
    /// 出力キュー
    output_queue: VecDeque<ConnectionOutput>,

    /// 乱数ソース
    random: R,
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
        Self {
            state: ConnectionState::Disconnected,
            options,
            nonce: [0u8; 16], // connect() で設定
            handshake_validator: None,
            frame_decoder: FrameDecoder::new(),
            fragment_buffer: FragmentBuffer::new(),
            negotiated_protocol: None,
            negotiated_extensions: Vec::new(),
            deflate: None,
            close_sent: false,
            close_received: false,
            awaiting_pong: false,
            failed: false,
            event_queue: VecDeque::new(),
            output_queue: VecDeque::new(),
            random,
        }
    }

    /// 現在の状態を取得
    pub fn state(&self) -> ConnectionState {
        self.state
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
        if self.state != ConnectionState::Disconnected {
            return Err(Error::invalid_state("already connected or connecting"));
        }

        // ハンドシェイク用の nonce を生成
        self.nonce = self.random.nonce();

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
        self.output_queue
            .push_back(ConnectionOutput::SendData(encoded));

        // 状態遷移
        self.set_state(ConnectionState::Connecting);

        Ok(())
    }

    /// 受信データを処理
    ///
    /// RFC 6455 Section 7.1.7: このメソッドが Err を返した後は
    /// 以降の呼び出しも即座に Err を返す。
    pub fn feed_recv_buf(&mut self, buf: &[u8], now: Timestamp) -> Result<(), Error> {
        if self.failed {
            return Err(Error::invalid_state("connection has failed"));
        }
        let result = match self.state {
            ConnectionState::Connecting => self.process_handshake(buf, now),
            ConnectionState::Connected | ConnectionState::Closing => self.process_frames(buf, now),
            ConnectionState::Disconnected | ConnectionState::Closed => {
                return Err(Error::invalid_state("connection is closed"));
            }
        };
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// テキストメッセージを送信
    pub fn send_text(&mut self, text: &str) -> Result<(), Error> {
        self.check_connected()?;
        self.send_data_frame(Opcode::Text, text.as_bytes().to_vec())
    }

    /// バイナリメッセージを送信
    pub fn send_binary(&mut self, data: &[u8]) -> Result<(), Error> {
        self.check_connected()?;
        self.send_data_frame(Opcode::Binary, data.to_vec())
    }

    /// データフレームを送信（圧縮対応）
    fn send_data_frame(&mut self, opcode: Opcode, payload: Vec<u8>) -> Result<(), Error> {
        let (payload, compressed) = self.compress_if_enabled(payload)?;

        let mut frame = Frame::new(opcode, payload);
        frame.rsv1 = compressed;

        self.send_frame(frame)
    }

    /// 圧縮が有効な場合、ペイロードを圧縮する
    fn compress_if_enabled(&mut self, payload: Vec<u8>) -> Result<(Vec<u8>, bool), Error> {
        if let Some(deflate) = &mut self.deflate {
            // 小さなメッセージは圧縮しない（圧縮のオーバーヘッドが大きくなる可能性）
            const COMPRESSION_THRESHOLD: usize = 64;
            if deflate.should_compress(&payload, COMPRESSION_THRESHOLD) {
                let compressed = deflate.compress(&payload)?;
                Ok((compressed, true))
            } else {
                Ok((payload, false))
            }
        } else {
            Ok((payload, false))
        }
    }

    /// Ping を送信
    ///
    /// RFC 6455 Section 5.5: data は 125 バイト以下でなければならない
    pub fn send_ping(&mut self, data: &[u8]) -> Result<(), Error> {
        self.check_connected()?;

        let frame = Frame::ping(data.to_vec())?;
        self.send_frame(frame)?;

        self.awaiting_pong = true;

        // Pong タイムアウト設定
        self.output_queue.push_back(ConnectionOutput::SetTimer {
            id: TimerId::PongTimeout,
            duration_millis: self.options.pong_timeout_millis,
        });

        Ok(())
    }

    /// 接続をクローズ
    ///
    /// RFC 6455 Section 7.4.1: 送信禁止のクローズコード (1005, 1006, 1015) は拒否される
    /// RFC 6455 Section 5.5: reason は 123 バイト以下でなければならない
    /// RFC 6455 Section 7.1.2: Close フレームは established connection 上でのみ送信可能
    pub fn close(&mut self, code: CloseCode, reason: &str) -> Result<(), Error> {
        if !matches!(
            self.state,
            ConnectionState::Connected | ConnectionState::Closing
        ) {
            return Err(Error::invalid_state("connection is not established"));
        }

        // RFC 6455 Section 7.4.1: 送信禁止のクローズコードをチェック
        if !code.is_sendable() {
            return Err(Error::invalid_input(format!(
                "close code {} is not sendable",
                code.as_u16()
            )));
        }

        if !self.close_sent {
            let frame = Frame::close(Some(code.as_u16()), reason)?;
            self.send_frame(frame)?;
            self.close_sent = true;

            // クローズタイムアウト設定
            self.output_queue.push_back(ConnectionOutput::SetTimer {
                id: TimerId::CloseTimeout,
                duration_millis: self.options.close_timeout_millis,
            });

            self.set_state(ConnectionState::Closing);
        }

        Ok(())
    }

    /// タイマーイベントを処理
    pub fn handle_timer(&mut self, timer_id: TimerId) -> Result<(), Error> {
        match timer_id {
            TimerId::Ping => {
                if self.state == ConnectionState::Connected && !self.awaiting_pong {
                    self.send_ping(&[])?;
                }
                // 次の Ping タイマー設定（Connected 状態の場合のみ）
                if self.state == ConnectionState::Connected && self.options.ping_interval_millis > 0
                {
                    self.output_queue.push_back(ConnectionOutput::SetTimer {
                        id: TimerId::Ping,
                        duration_millis: self.options.ping_interval_millis,
                    });
                }
            }
            TimerId::PongTimeout => {
                if self.awaiting_pong {
                    // Pong タイムアウト - 接続を閉じる
                    self.event_queue
                        .push_back(ConnectionEvent::Error("pong timeout".to_string()));
                    self.close(CloseCode::POLICY_VIOLATION, "pong timeout")?;
                }
            }
            TimerId::CloseTimeout => {
                if self.state == ConnectionState::Closing {
                    // クローズタイムアウト - 強制切断
                    self.set_state(ConnectionState::Closed);
                    self.output_queue
                        .push_back(ConnectionOutput::CloseConnection);
                }
            }
        }
        Ok(())
    }

    /// イベントを取得
    pub fn poll_event(&mut self) -> Option<ConnectionEvent> {
        self.event_queue.pop_front()
    }

    /// 出力を取得
    pub fn poll_output(&mut self) -> Option<ConnectionOutput> {
        self.output_queue.pop_front()
    }

    // === 内部メソッド ===

    fn set_state(&mut self, new_state: ConnectionState) {
        if self.state != new_state {
            self.state = new_state;
            self.event_queue
                .push_back(ConnectionEvent::StateChanged(new_state));
        }
    }

    fn check_connected(&self) -> Result<(), Error> {
        if self.state != ConnectionState::Connected {
            return Err(Error::invalid_state("not connected"));
        }
        Ok(())
    }

    /// フレームを送信（masking_key は自動生成）
    fn send_frame(&mut self, frame: Frame) -> Result<(), Error> {
        let masking_key = self.random.masking_key();
        let encoded = frame.encode(masking_key);
        self.output_queue
            .push_back(ConnectionOutput::SendData(encoded));
        Ok(())
    }

    /// 内部エラー処理用のクローズ
    ///
    /// 理由が長すぎる場合は UTF-8 文字境界で切り詰める
    fn close_internal(&mut self, code: CloseCode, reason: &str) {
        if self.state == ConnectionState::Disconnected || self.state == ConnectionState::Closed {
            return;
        }

        if !self.close_sent {
            // RFC 6455 Section 5.5 / 5.5.1: コントロールフレームのペイロードは 125 バイト以下、
            // Close フレームは先頭 2 バイトが status code のため reason は 123 バイト以下
            let truncated = truncate_reason(reason, 123);
            let frame = Frame::close(Some(code.as_u16()), truncated).unwrap_or_else(|_| {
                Frame::close(Some(code.as_u16()), "")
                    .expect("empty reason close frame must always succeed")
            });
            // エラー処理パスでの Close フレーム送信はベストエフォート
            let _ = self.send_frame(frame);
            self.close_sent = true;

            // クローズタイムアウト設定
            self.output_queue.push_back(ConnectionOutput::SetTimer {
                id: TimerId::CloseTimeout,
                duration_millis: self.options.close_timeout_millis,
            });

            self.set_state(ConnectionState::Closing);
        }
    }

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
                self.process_frames(&remaining, now)?;
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
        // RFC 7692 line 427-429, 431-436, 1015: サーバーは offer の中から 1 つを選んで受諾する。
        // 複数の permessage-deflate 要素は不正なレスポンスとして Fail しなければならない (MUST)。
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
                            self.deflate = Some(PerMessageDeflate::new_client(config));
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

        self.set_state(ConnectionState::Connected);

        self.event_queue.push_back(ConnectionEvent::Connected {
            protocol: self.negotiated_protocol.clone(),
            extensions: self.negotiated_extensions.clone(),
        });

        // Ping タイマー設定
        if self.options.ping_interval_millis > 0 {
            self.output_queue.push_back(ConnectionOutput::SetTimer {
                id: TimerId::Ping,
                duration_millis: self.options.ping_interval_millis,
            });
        }

        Ok(())
    }

    fn process_frames(&mut self, buf: &[u8], now: Timestamp) -> Result<(), Error> {
        self.frame_decoder.feed(buf);

        loop {
            match self.frame_decoder.decode_with_info() {
                Ok(Some(decoded)) => {
                    self.handle_decoded_frame(decoded, now)?;
                }
                Ok(None) => break,
                Err(e) => {
                    // RFC 6455 Section 7.1.7: 接続確立後のプロトコル違反では
                    // Close フレームを送信してから接続を終了する
                    self.close_internal(CloseCode::PROTOCOL_ERROR, "frame decode error");
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    fn handle_decoded_frame(
        &mut self,
        decoded: crate::websocket_frame::DecodedFrame,
        now: Timestamp,
    ) -> Result<(), Error> {
        // RFC 6455 Section 5.1: サーバーからのフレームはマスクしてはならない
        if decoded.masked {
            self.close_internal(CloseCode::PROTOCOL_ERROR, "masked server frame");
            return Err(Error::protocol_violation("masked server frame"));
        }
        self.handle_frame(decoded.frame, now)
    }

    fn handle_frame(&mut self, frame: Frame, now: Timestamp) -> Result<(), Error> {
        // フレームサイズチェック（コントロールフレームは RFC 6455 で 125 バイト以下が保証済み）
        if !frame.opcode.is_control() && frame.payload.len() > self.options.max_frame_size {
            self.close_internal(CloseCode::MESSAGE_TOO_BIG, "frame payload too large");
            return Err(Error::protocol_violation("frame payload too large"));
        }
        // RSV ビットチェック（permessage-deflate 以外は禁止）
        if frame.rsv2 || frame.rsv3 {
            self.close_internal(CloseCode::PROTOCOL_ERROR, "reserved bits set");
            return Err(Error::protocol_violation("reserved bits set"));
        }
        // RFC 7692 Section 6: RSV1 検証
        if frame.rsv1 {
            if self.deflate.is_none() {
                self.close_internal(
                    CloseCode::PROTOCOL_ERROR,
                    "rsv1 set without permessage-deflate",
                );
                return Err(Error::protocol_violation(
                    "rsv1 set without permessage-deflate",
                ));
            }
            // コントロールフレームでは RSV1=0 必須
            if frame.opcode.is_control() {
                self.close_internal(
                    CloseCode::PROTOCOL_ERROR,
                    "rsv1 must not be set on control frames",
                );
                return Err(Error::protocol_violation(
                    "rsv1 must not be set on control frames",
                ));
            }
            // 継続フレームでは RSV1=0 必須
            if frame.opcode == Opcode::Continuation {
                self.close_internal(
                    CloseCode::PROTOCOL_ERROR,
                    "rsv1 must not be set on continuation frames",
                );
                return Err(Error::protocol_violation(
                    "rsv1 must not be set on continuation frames",
                ));
            }
        }

        match frame.opcode {
            Opcode::Continuation => self.handle_continuation(frame, now)?,
            Opcode::Text | Opcode::Binary => self.handle_data_frame(frame, now)?,
            Opcode::Close => self.handle_close(frame, now)?,
            Opcode::Ping => self.handle_ping(frame)?,
            Opcode::Pong => self.handle_pong(frame)?,
        }

        Ok(())
    }

    fn handle_data_frame(&mut self, frame: Frame, now: Timestamp) -> Result<(), Error> {
        // RFC 6455 Section 5.4: フラグメント中に新しいデータフレームは禁止
        if !self.fragment_buffer.is_empty() {
            self.close_internal(
                CloseCode::PROTOCOL_ERROR,
                "new message started before previous completed",
            );
            return Err(Error::protocol_violation(
                "new message started before previous completed",
            ));
        }

        if frame.fin {
            // 完全なメッセージ
            let payload = self.decompress_if_needed(frame.payload, frame.rsv1)?;
            self.emit_message(frame.opcode, payload, now)?;
        } else {
            // フラグメント開始 (RSV1 は最初のフレームにのみ設定される)
            if frame.payload.len() > self.options.max_message_size {
                self.close_internal(CloseCode::MESSAGE_TOO_BIG, "message too large");
                return Err(Error::protocol_violation("message too large"));
            }
            self.fragment_buffer
                .start(frame.opcode, frame.payload, frame.rsv1);
        }
        Ok(())
    }

    fn handle_continuation(&mut self, frame: Frame, now: Timestamp) -> Result<(), Error> {
        if self.fragment_buffer.is_empty() {
            self.close_internal(
                CloseCode::PROTOCOL_ERROR,
                "continuation frame without initial frame",
            );
            return Err(Error::protocol_violation(
                "continuation frame without initial frame",
            ));
        }

        self.fragment_buffer.append(&frame.payload);

        // フラグメント累積サイズチェック
        if self.fragment_buffer.len() > self.options.max_message_size {
            self.close_internal(CloseCode::MESSAGE_TOO_BIG, "message too large");
            return Err(Error::protocol_violation("message too large"));
        }

        if frame.fin {
            let (opcode, payload, compressed) = self.fragment_buffer.take();
            let payload = self.decompress_if_needed(payload, compressed)?;
            self.emit_message(opcode, payload, now)?;
        }

        Ok(())
    }

    /// 必要に応じてペイロードを解凍する
    fn decompress_if_needed(
        &mut self,
        payload: Vec<u8>,
        compressed: bool,
    ) -> Result<Vec<u8>, Error> {
        if compressed {
            if let Some(deflate) = &mut self.deflate {
                deflate.decompress(&payload, self.options.max_decompressed_size)
            } else {
                self.close_internal(
                    CloseCode::PROTOCOL_ERROR,
                    "received compressed frame without permessage-deflate",
                );
                Err(Error::protocol_violation(
                    "received compressed frame without permessage-deflate",
                ))
            }
        } else {
            Ok(payload)
        }
    }

    fn emit_message(
        &mut self,
        opcode: Opcode,
        payload: Vec<u8>,
        _now: Timestamp,
    ) -> Result<(), Error> {
        match opcode {
            Opcode::Text => match String::from_utf8(payload) {
                Ok(text) => {
                    self.event_queue
                        .push_back(ConnectionEvent::TextMessage(text));
                }
                Err(e) => {
                    // RFC 6455 Section 8.1: UTF-8 検証失敗時は接続を失敗させる
                    self.event_queue.push_back(ConnectionEvent::Error(format!(
                        "invalid UTF-8 in text message: {}",
                        e
                    )));
                    self.close_internal(CloseCode::INVALID_PAYLOAD, "invalid UTF-8");
                    return Err(Error::protocol_violation("invalid UTF-8 in text message"));
                }
            },
            Opcode::Binary => {
                self.event_queue
                    .push_back(ConnectionEvent::BinaryMessage(payload));
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_close(&mut self, frame: Frame, _now: Timestamp) -> Result<(), Error> {
        self.close_received = true;

        // RFC 6455 Section 5.5.1: ペイロード長は 0 または 2 以上でなければならない
        if frame.payload.len() == 1 {
            self.close_internal(
                CloseCode::PROTOCOL_ERROR,
                "close frame payload length must be 0 or >= 2",
            );
            return Err(Error::protocol_violation(
                "close frame payload length must be 0 or >= 2",
            ));
        }

        let (code, reason) = if frame.payload.len() >= 2 {
            let code_val = u16::from_be_bytes([frame.payload[0], frame.payload[1]]);
            let close_code = CloseCode::new(code_val);

            // RFC 6455 Section 7.4.1: クローズコードの妥当性検証
            if !close_code.is_valid() {
                self.close_internal(
                    CloseCode::PROTOCOL_ERROR,
                    &format!("invalid close code: {}", code_val),
                );
                return Err(Error::protocol_violation(format!(
                    "invalid close code: {}",
                    code_val
                )));
            }

            // RFC 6455 Section 5.5.1: 理由は有効な UTF-8 でなければならない
            let reason = match String::from_utf8(frame.payload[2..].to_vec()) {
                Ok(r) => r,
                Err(_) => {
                    self.close_internal(
                        CloseCode::PROTOCOL_ERROR,
                        "close frame reason is not valid UTF-8",
                    );
                    return Err(Error::protocol_violation(
                        "close frame reason is not valid UTF-8",
                    ));
                }
            };

            (Some(close_code), reason)
        } else {
            (None, String::new())
        };

        self.event_queue
            .push_back(ConnectionEvent::Close { code, reason });

        if !self.close_sent {
            // クローズフレームを返送
            // 送信禁止コードの場合は 1000 (Normal Closure) を使用
            let reply_code = code
                .filter(|c| c.is_sendable())
                .map(|c| c.as_u16())
                .unwrap_or(1000);
            let reply_frame = Frame::close(Some(reply_code), "")?;
            self.send_frame(reply_frame)?;
            self.close_sent = true;
        }

        // 両方向でクローズが完了
        // Ping/Pong 関連の状態とタイマーをクリア
        self.awaiting_pong = false;
        self.output_queue.push_back(ConnectionOutput::ClearTimer {
            id: TimerId::PongTimeout,
        });
        self.output_queue
            .push_back(ConnectionOutput::ClearTimer { id: TimerId::Ping });
        self.output_queue.push_back(ConnectionOutput::ClearTimer {
            id: TimerId::CloseTimeout,
        });
        self.set_state(ConnectionState::Closed);
        self.output_queue
            .push_back(ConnectionOutput::CloseConnection);

        // クリーンアップ
        self.frame_decoder.clear();
        self.fragment_buffer.clear();

        Ok(())
    }

    fn handle_ping(&mut self, frame: Frame) -> Result<(), Error> {
        // Ping イベントを発行
        self.event_queue
            .push_back(ConnectionEvent::Ping(frame.payload.clone()));

        // RFC 6455 Section 5.5.2: Close を受信済みなら Pong を送らない
        if !self.close_received {
            // Pong を自動返信（受信した Ping のペイロードをそのまま返すので 125 バイト以下は保証される）
            let pong = Frame::pong(frame.payload)?;
            self.send_frame(pong)?;
        }

        Ok(())
    }

    fn handle_pong(&mut self, frame: Frame) -> Result<(), Error> {
        self.awaiting_pong = false;

        // Pong タイムアウトをクリア
        self.output_queue.push_back(ConnectionOutput::ClearTimer {
            id: TimerId::PongTimeout,
        });

        // Pong イベントを発行
        self.event_queue
            .push_back(ConnectionEvent::Pong(frame.payload));

        Ok(())
    }
}
