//! WebSocket サーバー接続 (Sans I/O パターン)
//!
//! WebSocket サーバー接続を管理する状態機械。
//! I/O は外部で行い、この構造体はバッファ駆動型で動作する。

use std::collections::VecDeque;

use crate::Timestamp;
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
}

/// フラグメント収集バッファ
#[derive(Debug, Default)]
struct FragmentBuffer {
    /// 最初のフレームのオペコード
    opcode: Option<Opcode>,
    /// 収集中のペイロード
    payload: Vec<u8>,
}

impl FragmentBuffer {
    fn new() -> Self {
        Self::default()
    }

    fn is_empty(&self) -> bool {
        self.opcode.is_none()
    }

    fn start(&mut self, opcode: Opcode, payload: Vec<u8>) {
        self.opcode = Some(opcode);
        self.payload = payload;
    }

    fn append(&mut self, payload: &[u8]) {
        self.payload.extend_from_slice(payload);
    }

    fn take(&mut self) -> (Opcode, Vec<u8>) {
        let opcode = self.opcode.take().unwrap_or(Opcode::Binary);
        let payload = std::mem::take(&mut self.payload);
        (opcode, payload)
    }

    fn clear(&mut self) {
        self.opcode = None;
        self.payload.clear();
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
    /// ネゴシエートされた deflate 設定
    negotiated_deflate: Option<PerMessageDeflateConfig>,

    /// クローズフレームを送信したか
    close_sent: bool,
    /// クローズフレームを受信したか
    close_received: bool,

    /// 最後の Ping 送信時刻
    last_ping_time: Option<Timestamp>,
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
            negotiated_deflate: None,
            close_sent: false,
            close_received: false,
            last_ping_time: None,
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
    pub fn feed_recv_buf(&mut self, buf: &[u8], now: Timestamp) -> Result<(), Error> {
        match self.state {
            ConnectionState::Disconnected | ConnectionState::Connecting => {
                self.process_handshake(buf, now)
            }
            ConnectionState::Connected | ConnectionState::Closing => self.process_frames(buf, now),
            ConnectionState::Closed => Err(Error::invalid_state("connection is closed")),
        }
    }

    /// ハンドシェイクを自動で受諾
    pub fn accept_handshake_auto(&mut self, now: Timestamp) -> Result<(), Error> {
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

        self.accept_handshake(response, now)
    }

    /// ハンドシェイクを受諾
    pub fn accept_handshake(
        &mut self,
        response: ServerHandshakeResponse,
        now: Timestamp,
    ) -> Result<(), Error> {
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
            let mut supported = true;
            for ext in Extension::parse(extension) {
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

        let encoded = response_builder.encode();
        self.output_queue
            .push_back(ConnectionOutput::SendData(encoded));

        self.negotiated_protocol = response.protocol.clone();
        self.negotiated_extensions = response.extensions.clone();
        self.negotiated_deflate = None;
        for ext_str in &response.extensions {
            let extensions = Extension::parse(ext_str);
            for ext in extensions {
                if ext.name == "permessage-deflate" {
                    self.negotiated_deflate = PerMessageDeflateConfig::from_extension(&ext);
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

        if !self.pending_frame_data.is_empty() {
            let pending = std::mem::take(&mut self.pending_frame_data);
            self.process_frames(&pending, now)?;
        }

        self.handshake_validator.reset();
        Ok(())
    }

    /// ハンドシェイクを拒否
    pub fn reject_handshake(&mut self, status_code: u16, reason: &str) -> Result<(), Error> {
        if self.state != ConnectionState::Connecting {
            return Err(Error::invalid_state("handshake is not in progress"));
        }

        self.pending_request = None;
        self.pending_frame_data.clear();
        self.handshake_validator.reset();

        let response = Response::new(status_code, reason).header("Connection", "close");
        self.output_queue
            .push_back(ConnectionOutput::SendData(response.encode()));

        self.set_state(ConnectionState::Closed);
        self.output_queue
            .push_back(ConnectionOutput::CloseConnection);
        Ok(())
    }

    /// テキストメッセージを送信
    pub fn send_text(&mut self, text: &str, _now: Timestamp) -> Result<(), Error> {
        self.check_connected()?;

        let frame = Frame::text(text);
        self.send_frame(frame);

        Ok(())
    }

    /// バイナリメッセージを送信
    pub fn send_binary(&mut self, data: &[u8], _now: Timestamp) -> Result<(), Error> {
        self.check_connected()?;

        let frame = Frame::binary(data.to_vec());
        self.send_frame(frame);

        Ok(())
    }

    /// Ping を送信
    pub fn send_ping(&mut self, data: &[u8], now: Timestamp) -> Result<(), Error> {
        self.check_connected()?;

        let frame = Frame::ping(data.to_vec());
        self.send_frame(frame);

        self.last_ping_time = Some(now);
        self.awaiting_pong = true;

        // Pong タイムアウト設定
        self.output_queue.push_back(ConnectionOutput::SetTimer {
            id: TimerId::PongTimeout,
            duration_millis: self.options.pong_timeout_millis,
        });

        Ok(())
    }

    /// 接続をクローズ
    pub fn close(&mut self, code: CloseCode, reason: &str, _now: Timestamp) -> Result<(), Error> {
        if self.state == ConnectionState::Disconnected || self.state == ConnectionState::Closed {
            return Err(Error::invalid_state("connection is already closed"));
        }

        if !self.close_sent {
            let frame = Frame::close(Some(code.as_u16()), reason);
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

    /// タイマーイベントを処理
    pub fn handle_timer(&mut self, timer_id: TimerId, now: Timestamp) -> Result<(), Error> {
        match timer_id {
            TimerId::Ping => {
                if self.state == ConnectionState::Connected && !self.awaiting_pong {
                    // 空の Ping を送信
                    self.send_ping(&[], now)?;
                }
                // 次の Ping タイマー設定
                if self.options.ping_interval_millis > 0 {
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
                    self.close(CloseCode::POLICY_VIOLATION, "pong timeout", now)?;
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

    fn process_handshake(&mut self, buf: &[u8], _now: Timestamp) -> Result<(), Error> {
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

    fn process_frames(&mut self, buf: &[u8], now: Timestamp) -> Result<(), Error> {
        self.frame_decoder.feed(buf);

        while let Some(decoded) = self.frame_decoder.decode_with_info()? {
            self.handle_decoded_frame(decoded, now)?;
        }

        Ok(())
    }

    fn handle_decoded_frame(&mut self, decoded: DecodedFrame, now: Timestamp) -> Result<(), Error> {
        if !decoded.masked {
            return Err(Error::protocol_violation("unmasked client frame"));
        }
        self.handle_frame(decoded.frame, now)
    }

    fn handle_frame(&mut self, frame: Frame, now: Timestamp) -> Result<(), Error> {
        // RSV ビットチェック（permessage-deflate 以外は禁止）
        if frame.rsv2 || frame.rsv3 {
            return Err(Error::protocol_violation("reserved bits set"));
        }
        if frame.rsv1 && self.negotiated_deflate.is_none() {
            return Err(Error::protocol_violation(
                "rsv1 set without permessage-deflate",
            ));
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
            return Err(Error::protocol_violation(
                "new message started before previous completed",
            ));
        }

        if frame.fin {
            // 完全なメッセージ
            self.emit_message(frame.opcode, frame.payload, now)?;
        } else {
            // フラグメント開始
            self.fragment_buffer.start(frame.opcode, frame.payload);
        }
        Ok(())
    }

    fn handle_continuation(&mut self, frame: Frame, now: Timestamp) -> Result<(), Error> {
        if self.fragment_buffer.is_empty() {
            return Err(Error::protocol_violation(
                "continuation frame without initial frame",
            ));
        }

        self.fragment_buffer.append(&frame.payload);

        if frame.fin {
            let (opcode, payload) = self.fragment_buffer.take();
            self.emit_message(opcode, payload, now)?;
        }

        Ok(())
    }

    fn emit_message(
        &mut self,
        opcode: Opcode,
        payload: Vec<u8>,
        now: Timestamp,
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
                    self.close(CloseCode::INVALID_PAYLOAD, "invalid UTF-8", now)?;
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
            return Err(Error::protocol_violation(
                "close frame payload length must be 0 or >= 2",
            ));
        }

        let (code, reason) = if frame.payload.len() >= 2 {
            let code_val = u16::from_be_bytes([frame.payload[0], frame.payload[1]]);
            let close_code = CloseCode::new(code_val);

            // RFC 6455 Section 7.4.1: クローズコードの妥当性検証
            if !close_code.is_valid() {
                return Err(Error::protocol_violation(format!(
                    "invalid close code: {}",
                    code_val
                )));
            }

            // RFC 6455 Section 5.5.1: 理由は有効な UTF-8 でなければならない
            let reason = String::from_utf8(frame.payload[2..].to_vec())
                .map_err(|_| Error::protocol_violation("close frame reason is not valid UTF-8"))?;

            (Some(close_code), reason)
        } else {
            (None, String::new())
        };

        self.event_queue
            .push_back(ConnectionEvent::Close { code, reason });

        if !self.close_sent {
            // クローズフレームを返送
            let reply_code = code.map(|c| c.as_u16()).unwrap_or(1000);
            let reply_frame = Frame::close(Some(reply_code), "");
            self.send_frame(reply_frame);
            self.close_sent = true;
        }

        // 両方向でクローズが完了
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

        // Pong を自動返信
        let pong = Frame::pong(frame.payload);
        self.send_frame(pong);

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
        let config = self.options.deflate_config.clone()?;
        for ext_str in &request.extensions {
            for ext in Extension::parse(ext_str) {
                if ext.name == "permessage-deflate" {
                    return Some(config);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    fn create_handshake_request() -> String {
        let key = STANDARD.encode(b"0123456789ABCDEF");
        format!(
            "GET /chat HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Sec-WebSocket-Protocol: chat\r\n\
             \r\n",
            key
        )
    }

    #[test]
    fn test_server_handshake_auto() {
        let mut conn =
            WebSocketServerConnection::new(ServerConnectionOptions::new().protocol("chat"));
        let now = Timestamp::from_millis(0);

        let request = create_handshake_request();
        conn.feed_recv_buf(request.as_bytes(), now).unwrap();

        assert!(conn.handshake_request().is_some());

        conn.accept_handshake_auto(now).unwrap();
        assert_eq!(conn.state(), ConnectionState::Connected);

        let _event = conn.poll_event().unwrap(); // Connecting
        let _event = conn.poll_event().unwrap(); // Connected (StateChanged)
        let event = conn.poll_event().unwrap();
        match event {
            ConnectionEvent::Connected { protocol, .. } => {
                assert_eq!(protocol, Some("chat".to_string()));
            }
            _ => panic!("expected Connected event"),
        }

        let output = conn.poll_output().unwrap();
        match output {
            ConnectionOutput::SendData(data) => {
                let s = String::from_utf8(data).unwrap();
                assert!(s.starts_with("HTTP/1.1 101 Switching Protocols"));
                assert!(s.contains("Sec-WebSocket-Accept"));
            }
            _ => panic!("expected SendData"),
        }
    }

    #[test]
    fn test_server_requires_masked_frames() {
        let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
        let now = Timestamp::from_millis(0);

        let request = create_handshake_request();
        conn.feed_recv_buf(request.as_bytes(), now).unwrap();
        conn.accept_handshake_auto(now).unwrap();

        // 未マスクのテキストフレームは拒否
        let frame = [0x81, 0x05, b'H', b'e', b'l', b'l', b'o'];
        let result = conn.feed_recv_buf(&frame, now);
        assert!(result.is_err());

        // マスク済みフレームは受理
        let masked_frame = Frame::text("Hello").encode([1, 2, 3, 4]);
        conn.feed_recv_buf(&masked_frame, now).unwrap();

        while let Some(event) = conn.poll_event() {
            if event == ConnectionEvent::TextMessage("Hello".to_string()) {
                return;
            }
        }
        panic!("expected TextMessage");
    }
}
