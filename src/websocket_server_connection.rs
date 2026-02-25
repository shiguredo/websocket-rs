//! WebSocket サーバー接続 (Sans I/O パターン)
//!
//! WebSocket サーバー接続を管理する状態機械。
//! I/O は外部で行い、この構造体はバッファ駆動型で動作する。

use std::collections::VecDeque;

use crate::deflate::PerMessageDeflate;
use crate::error::Error;
use crate::websocket_close::CloseCode;
use crate::websocket_extension::{Extension, PerMessageDeflateConfig};
use crate::websocket_frame::{DecodedFrame, Frame, FrameDecoder};
use crate::websocket_handshake::{
    HandshakeRequestValidator, ServerHandshakeRequest, ServerHandshakeResponse,
    calculate_accept_from_key,
};
use crate::websocket_opcode::Opcode;
use crate::{ConnectionEvent, ConnectionOutput, ConnectionState, TimerId};
use shiguredo_http11::Response;

/// デフォルトの最大解凍サイズ（16MB）
pub const DEFAULT_MAX_DECOMPRESSED_SIZE: usize = 16 * 1024 * 1024;

/// サーバー接続オプション
#[derive(Debug, Clone)]
pub struct ServerConnectionOptions {
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
    /// 最大解凍サイズ（Zip Bomb 対策）
    pub max_decompressed_size: usize,
}

impl Default for ServerConnectionOptions {
    fn default() -> Self {
        Self {
            protocols: Vec::new(),
            deflate_config: None,
            additional_headers: Vec::new(),
            ping_interval_millis: 30_000, // 30秒
            pong_timeout_millis: 10_000,  // 10秒
            close_timeout_millis: 5_000,  // 5秒
            max_decompressed_size: DEFAULT_MAX_DECOMPRESSED_SIZE,
        }
    }
}

impl ServerConnectionOptions {
    /// 新しい接続オプションを生成
    pub fn new() -> Self {
        Self::default()
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

/// WebSocket サーバー接続
pub struct WebSocketServerConnection {
    /// 状態
    state: ConnectionState,
    /// オプション
    options: ServerConnectionOptions,

    /// ハンドシェイクバリデーター
    handshake_validator: HandshakeRequestValidator,
    /// 受信済みハンドシェイクリクエスト
    pending_request: Option<ServerHandshakeRequest>,
    /// ハンドシェイク後に処理する残りデータ
    pending_frame_data: Vec<u8>,

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

    /// イベントキュー
    event_queue: VecDeque<ConnectionEvent>,
    /// 出力キュー
    output_queue: VecDeque<ConnectionOutput>,
}

impl WebSocketServerConnection {
    /// 新しい接続を作成
    pub fn new(options: ServerConnectionOptions) -> Self {
        Self {
            state: ConnectionState::Disconnected,
            options,
            handshake_validator: HandshakeRequestValidator::new(),
            pending_request: None,
            pending_frame_data: Vec::new(),
            frame_decoder: FrameDecoder::new(),
            fragment_buffer: FragmentBuffer::new(),
            negotiated_protocol: None,
            negotiated_extensions: Vec::new(),
            deflate: None,
            close_sent: false,
            close_received: false,
            awaiting_pong: false,
            event_queue: VecDeque::new(),
            output_queue: VecDeque::new(),
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

    /// ハンドシェイクリクエストを取得
    pub fn handshake_request(&self) -> Option<&ServerHandshakeRequest> {
        self.pending_request.as_ref()
    }

    /// 受信データを処理
    ///
    /// RFC 6455 Section 7.1.7: このメソッドが Err を返した場合、呼び出し側は
    /// 以降の feed_recv_buf() 呼び出しを停止しなければならない。
    /// Err 後も呼び出しを継続すると、Closing 状態でデータフレームが
    /// 処理される可能性がある。
    pub fn feed_recv_buf(&mut self, buf: &[u8]) -> Result<(), Error> {
        match self.state {
            ConnectionState::Disconnected | ConnectionState::Connecting => {
                self.process_handshake(buf)
            }
            ConnectionState::Connected | ConnectionState::Closing => self.process_frames(buf),
            ConnectionState::Closed => Err(Error::invalid_state("connection is closed")),
        }
    }

    /// ハンドシェイクを自動で受諾
    pub fn accept_handshake_auto(&mut self) -> Result<(), Error> {
        let request = self
            .pending_request
            .as_ref()
            .ok_or_else(|| Error::invalid_state("handshake request not available"))?;

        let mut response = ServerHandshakeResponse::new();

        if let Some(protocol) = self.select_protocol(request) {
            response = response.protocol(&protocol);
        }

        if let Some(config) = self.select_deflate(request) {
            response = response.extension(&config.to_extension().encode());
        }

        for (name, value) in &self.options.additional_headers {
            response = response.header(name, value);
        }

        self.accept_handshake(response)
    }

    /// ハンドシェイクを受諾
    pub fn accept_handshake(&mut self, response: ServerHandshakeResponse) -> Result<(), Error> {
        if self.state != ConnectionState::Connecting {
            return Err(Error::invalid_state("handshake is not in progress"));
        }

        let request = self
            .pending_request
            .take()
            .ok_or_else(|| Error::invalid_state("handshake request not available"))?;

        if let Some(protocol) = &response.protocol
            && !request.protocols.iter().any(|p| p == protocol)
        {
            return Err(Error::handshake_rejected(format!(
                "unsupported protocol: {}",
                protocol
            )));
        }

        for extension in &response.extensions {
            // RFC 6455 Section 9.1: ABNF 不適合の拡張文字列は接続を失敗させなければならない (MUST)
            // parse_strict を使い、部分的に不正な拡張を見逃さないようにする
            let parsed = Extension::parse_strict(extension).map_err(|e| {
                Error::handshake_rejected(format!(
                    "invalid extension response '{}': {}",
                    extension, e
                ))
            })?;
            if parsed.is_empty() {
                return Err(Error::handshake_rejected(format!(
                    "invalid extension response: '{}'",
                    extension
                )));
            }
            let mut supported = true;
            for ext in &parsed {
                if request
                    .extensions
                    .iter()
                    .any(|req| Extension::parse(req).iter().any(|e| e.name == ext.name))
                {
                    continue;
                } else {
                    supported = false;
                    break;
                }
            }
            if !supported {
                return Err(Error::handshake_rejected(format!(
                    "unsupported extension: {}",
                    extension
                )));
            }
        }

        // RFC 6455 Section 4.2.2: 予約済みヘッダーとの重複チェック
        // これらのヘッダーは MUST appear かつ MUST NOT appear more than once
        const RESERVED: &[&str] = &[
            "upgrade",
            "connection",
            "sec-websocket-accept",
            "sec-websocket-protocol",
            "sec-websocket-extensions",
        ];
        for (name, _) in &response.additional_headers {
            if RESERVED.contains(&name.to_ascii_lowercase().as_str()) {
                return Err(Error::invalid_input(format!(
                    "additional header '{}' conflicts with a reserved WebSocket header",
                    name
                )));
            }
        }

        let accept = calculate_accept_from_key(&request.key);
        let mut response_builder = Response::new(101, "Switching Protocols")
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Accept", &accept);

        if let Some(protocol) = &response.protocol {
            response_builder = response_builder.header("Sec-WebSocket-Protocol", protocol);
        }

        if !response.extensions.is_empty() {
            response_builder = response_builder
                .header("Sec-WebSocket-Extensions", &response.extensions.join(", "));
        }

        for (name, value) in &response.additional_headers {
            response_builder = response_builder.header(name, value);
        }

        // permessage-deflate のネゴシエーション結果を解析し、コーデックを作成
        // RFC 7692 Section 7: 検証をレスポンス送信前に行い、エラー時は送信しない

        // RFC 7692 Section 7.1.2.1: クライアントが offer した server_max_window_bits を取得する
        let client_offered_smwb: Option<u8> = request
            .extensions
            .iter()
            .flat_map(|s| Extension::parse(s))
            .filter(|e| e.name == "permessage-deflate")
            .find_map(|e| {
                e.get_param("server_max_window_bits")
                    .and_then(|p| p.value.as_deref())
                    .and_then(|v| v.parse::<u8>().ok())
            });

        let mut deflate = None;
        for ext_str in &response.extensions {
            let extensions = Extension::parse(ext_str);
            for ext in extensions {
                if ext.name == "permessage-deflate" {
                    // サーバー自身が送信するレスポンスなので、ClientResponse コンテキストで検証
                    let config = PerMessageDeflateConfig::from_extension_for_client_response(&ext)
                        .map_err(|e| {
                            Error::handshake_rejected(format!(
                                "invalid permessage-deflate parameters: {:?}",
                                e
                            ))
                        })?;

                    // RFC 7692 Section 7.2.1: 合意した server_max_window_bits で
                    // 圧縮する必要がある。現在の実装では window_bits=15 固定
                    // (flate2 の制約) のため、server_max_window_bits < 15 は
                    // サポートしない
                    if let Some(smwb) = config.server_max_window_bits
                        && smwb < 15
                    {
                        return Err(Error::handshake_rejected(format!(
                            "server_max_window_bits={} is not supported (only 15 is supported)",
                            smwb
                        )));
                    }

                    // RFC 7692 Section 7.1.2.1: レスポンスの server_max_window_bits は
                    // クライアントの offer 値以下でなければならない (same or smaller)
                    if let (Some(smwb), Some(offered)) =
                        (config.server_max_window_bits, client_offered_smwb)
                        && smwb > offered
                    {
                        return Err(Error::handshake_rejected(format!(
                            "server_max_window_bits={} exceeds client offer={}",
                            smwb, offered
                        )));
                    }

                    // RFC 7692 Section 7.1.2.2: クライアントが client_max_window_bits を
                    // offer していない場合、レスポンスに含めてはならない (MUST NOT)
                    if ext.get_param("client_max_window_bits").is_some() {
                        let client_offered_cmwb = request.extensions.iter().any(|req_ext_str| {
                            Extension::parse(req_ext_str).iter().any(|req_ext| {
                                req_ext.name == "permessage-deflate"
                                    && req_ext.get_param("client_max_window_bits").is_some()
                            })
                        });
                        if !client_offered_cmwb {
                            return Err(Error::handshake_rejected(
                                "client_max_window_bits included without client offer",
                            ));
                        }
                    }

                    deflate = Some(PerMessageDeflate::new_server(config));
                }
            }
        }

        let encoded = response_builder.encode();
        self.output_queue
            .push_back(ConnectionOutput::SendData(encoded));

        self.deflate = deflate;
        self.negotiated_protocol = response.protocol.clone();
        self.negotiated_extensions = response.extensions.clone();

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

        if !self.pending_frame_data.is_empty() {
            let pending = std::mem::take(&mut self.pending_frame_data);
            self.process_frames(&pending)?;
        }

        self.handshake_validator.reset();
        Ok(())
    }

    /// ハンドシェイクを拒否
    ///
    /// `headers` には追加レスポンスヘッダーを指定できる。
    /// バージョン不一致時は RFC 6455 Section 4.4 の MUST に従い
    /// `("Sec-WebSocket-Version", "13")` を含めること。
    pub fn reject_handshake(
        &mut self,
        status_code: u16,
        reason: &str,
        headers: &[(&str, &str)],
    ) -> Result<(), Error> {
        if self.state != ConnectionState::Connecting {
            return Err(Error::invalid_state("handshake is not in progress"));
        }

        self.pending_request = None;
        self.pending_frame_data.clear();
        self.handshake_validator.reset();

        let mut response = Response::new(status_code, reason).header("Connection", "close");
        for (name, value) in headers {
            response = response.header(name, value);
        }
        self.output_queue
            .push_back(ConnectionOutput::SendData(response.encode()));

        self.set_state(ConnectionState::Closed);
        self.output_queue
            .push_back(ConnectionOutput::CloseConnection);
        Ok(())
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

        self.send_frame(frame);
        Ok(())
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
        self.send_frame(frame);

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
            self.send_frame(frame);
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

    /// 内部エラー処理用のクローズ
    ///
    /// プロトコル違反などで自動的にクローズする際に使用。
    /// 理由が長すぎる場合は切り詰める。
    fn close_internal(&mut self, code: CloseCode, reason: &str) {
        if self.state == ConnectionState::Disconnected || self.state == ConnectionState::Closed {
            return;
        }

        if !self.close_sent {
            // 理由が長すぎる場合は切り詰める
            let truncated_reason = if reason.len() > 123 {
                &reason[..123]
            } else {
                reason
            };
            let frame = Frame::close(Some(code.as_u16()), truncated_reason)
                .unwrap_or_else(|_| Frame::close(Some(code.as_u16()), "").unwrap());
            self.send_frame(frame);
            self.close_sent = true;

            // クローズタイムアウト設定
            self.output_queue.push_back(ConnectionOutput::SetTimer {
                id: TimerId::CloseTimeout,
                duration_millis: self.options.close_timeout_millis,
            });

            self.set_state(ConnectionState::Closing);
        }
    }

    /// タイマーイベントを処理
    pub fn handle_timer(&mut self, timer_id: TimerId) -> Result<(), Error> {
        match timer_id {
            TimerId::Ping => {
                if self.state == ConnectionState::Connected && !self.awaiting_pong {
                    // 空の Ping を送信
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

    fn send_frame(&mut self, frame: Frame) {
        let encoded = frame.encode_unmasked();
        self.output_queue
            .push_back(ConnectionOutput::SendData(encoded));
    }

    fn process_handshake(&mut self, buf: &[u8]) -> Result<(), Error> {
        if self.pending_request.is_some() {
            self.pending_frame_data.extend_from_slice(buf);
            return Ok(());
        }

        if self.state == ConnectionState::Disconnected {
            self.set_state(ConnectionState::Connecting);
        }

        self.handshake_validator.feed(buf);
        let result = self.handshake_validator.validate()?;
        if let Some(request) = result {
            self.pending_request = Some(request);
            self.pending_frame_data
                .extend_from_slice(self.handshake_validator.remaining());
        }

        Ok(())
    }

    fn process_frames(&mut self, buf: &[u8]) -> Result<(), Error> {
        self.frame_decoder.feed(buf);

        loop {
            match self.frame_decoder.decode_with_info() {
                Ok(Some(decoded)) => {
                    self.handle_decoded_frame(decoded)?;
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

    fn handle_decoded_frame(&mut self, decoded: DecodedFrame) -> Result<(), Error> {
        if !decoded.masked {
            self.close_internal(CloseCode::PROTOCOL_ERROR, "unmasked client frame");
            return Err(Error::protocol_violation("unmasked client frame"));
        }
        self.handle_frame(decoded.frame)
    }

    fn handle_frame(&mut self, frame: Frame) -> Result<(), Error> {
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
            Opcode::Continuation => self.handle_continuation(frame)?,
            Opcode::Text | Opcode::Binary => self.handle_data_frame(frame)?,
            Opcode::Close => self.handle_close(frame)?,
            Opcode::Ping => self.handle_ping(frame)?,
            Opcode::Pong => self.handle_pong(frame)?,
        }

        Ok(())
    }

    fn handle_data_frame(&mut self, frame: Frame) -> Result<(), Error> {
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
            self.emit_message(frame.opcode, payload)?;
        } else {
            // フラグメント開始 (RSV1 は最初のフレームにのみ設定される)
            self.fragment_buffer
                .start(frame.opcode, frame.payload, frame.rsv1);
        }
        Ok(())
    }

    fn handle_continuation(&mut self, frame: Frame) -> Result<(), Error> {
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

        if frame.fin {
            let (opcode, payload, compressed) = self.fragment_buffer.take();
            let payload = self.decompress_if_needed(payload, compressed)?;
            self.emit_message(opcode, payload)?;
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

    fn emit_message(&mut self, opcode: Opcode, payload: Vec<u8>) -> Result<(), Error> {
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
                    self.close(CloseCode::INVALID_PAYLOAD, "invalid UTF-8")?;
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

    fn handle_close(&mut self, frame: Frame) -> Result<(), Error> {
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
            self.send_frame(reply_frame);
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
            self.send_frame(pong);
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

    fn select_protocol(&self, request: &ServerHandshakeRequest) -> Option<String> {
        for protocol in &request.protocols {
            if self.options.protocols.iter().any(|p| p == protocol) {
                return Some(protocol.clone());
            }
        }
        None
    }

    fn select_deflate(&self, request: &ServerHandshakeRequest) -> Option<PerMessageDeflateConfig> {
        let server_config = self.options.deflate_config.clone()?;
        for ext_str in &request.extensions {
            for ext in Extension::parse(ext_str) {
                if ext.name == "permessage-deflate" {
                    // クライアント要求をパース
                    match PerMessageDeflateConfig::from_extension_for_server_request(&ext) {
                        Ok(client_request) => {
                            // RFC 7692 Section 7.1.2.1: server_max_window_bits が offer された場合、
                            // サーバーは同値以下を応答に含めることで受け入れる。
                            // flate2 は window_bits=15 固定のため、15 未満の offer はサポートできない。
                            if client_request
                                .server_max_window_bits
                                .is_some_and(|v| v < 15)
                            {
                                // この offer はスキップして次を試す
                                continue;
                            }
                            // クライアント要求とサーバー設定をマージ
                            return Some(PerMessageDeflateConfig::negotiate(
                                &client_request,
                                &server_config,
                            ));
                        }
                        Err(_) => {
                            // パースに失敗した場合（不正なパラメータ）は無視して次を試す
                        }
                    }
                }
            }
        }
        None
    }
}
