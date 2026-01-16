//! WebSocket クライアント接続 (Sans I/O パターン)
//!
//! WebSocket クライアント接続を管理する状態機械。
//! I/O は外部で行い、この構造体はバッファ駆動型で動作する。

use std::collections::VecDeque;

use crate::Timestamp;
use crate::error::Error;
use crate::websocket_close::CloseCode;
use crate::websocket_extension::{Extension, PerMessageDeflateConfig};
use crate::websocket_frame::{Frame, FrameDecoder};
use crate::websocket_handshake::{HandshakeRequest, HandshakeResponse, HandshakeValidator};
use crate::websocket_opcode::Opcode;

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

/// 暗号学的に安全な乱数を生成
fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).expect("getrandom failed");
    buf
}

/// WebSocket 接続
pub struct WebSocketClientConnection {
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

impl WebSocketClientConnection {
    /// 新しい接続を作成
    pub fn new(options: ClientConnectionOptions) -> Self {
        Self {
            state: ConnectionState::Disconnected,
            options,
            nonce: random_bytes(),
            handshake_validator: None,
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

    /// 接続を開始
    pub fn connect(&mut self, _now: Timestamp) -> Result<(), Error> {
        if self.state != ConnectionState::Disconnected {
            return Err(Error::invalid_state("already connected or connecting"));
        }

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

        let encoded = request.build(self.nonce);

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
    pub fn feed_recv_buf(&mut self, buf: &[u8], now: Timestamp) -> Result<(), Error> {
        match self.state {
            ConnectionState::Connecting => self.process_handshake(buf, now),
            ConnectionState::Connected | ConnectionState::Closing => self.process_frames(buf, now),
            ConnectionState::Disconnected | ConnectionState::Closed => {
                Err(Error::invalid_state("connection is closed"))
            }
        }
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
        let masking_key: [u8; 4] = random_bytes();
        let encoded = frame.encode(masking_key);
        self.output_queue
            .push_back(ConnectionOutput::SendData(encoded));
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

            self.complete_handshake(response, now);

            if !remaining.is_empty() {
                self.frame_decoder.feed(&remaining);
            }
            self.handshake_validator = None;
        }

        Ok(())
    }

    fn complete_handshake(&mut self, response: HandshakeResponse, _now: Timestamp) {
        self.negotiated_protocol = response.protocol;
        self.negotiated_extensions = response.extensions.clone();

        // permessage-deflate のネゴシエーション結果を解析
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
    }

    fn process_frames(&mut self, buf: &[u8], now: Timestamp) -> Result<(), Error> {
        self.frame_decoder.feed(buf);

        while let Some(frame) = self.frame_decoder.decode()? {
            self.handle_frame(frame, now)?;
        }

        Ok(())
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
            Opcode::Continuation => self.handle_continuation(frame)?,
            Opcode::Text | Opcode::Binary => self.handle_data_frame(frame)?,
            Opcode::Close => self.handle_close(frame, now)?,
            Opcode::Ping => self.handle_ping(frame)?,
            Opcode::Pong => self.handle_pong(frame)?,
        }

        Ok(())
    }

    fn handle_data_frame(&mut self, frame: Frame) -> Result<(), Error> {
        // RFC 6455 Section 5.4: フラグメント中に新しいデータフレームは禁止
        if !self.fragment_buffer.is_empty() {
            return Err(Error::protocol_violation(
                "new message started before previous completed",
            ));
        }

        if frame.fin {
            // 完全なメッセージ
            self.emit_message(frame.opcode, frame.payload);
        } else {
            // フラグメント開始
            self.fragment_buffer.start(frame.opcode, frame.payload);
        }
        Ok(())
    }

    fn handle_continuation(&mut self, frame: Frame) -> Result<(), Error> {
        if self.fragment_buffer.is_empty() {
            return Err(Error::protocol_violation(
                "continuation frame without initial frame",
            ));
        }

        self.fragment_buffer.append(&frame.payload);

        if frame.fin {
            let (opcode, payload) = self.fragment_buffer.take();
            self.emit_message(opcode, payload);
        }

        Ok(())
    }

    fn emit_message(&mut self, opcode: Opcode, payload: Vec<u8>) {
        match opcode {
            Opcode::Text => match String::from_utf8(payload) {
                Ok(text) => {
                    self.event_queue
                        .push_back(ConnectionEvent::TextMessage(text));
                }
                Err(e) => {
                    self.event_queue.push_back(ConnectionEvent::Error(format!(
                        "invalid UTF-8 in text message: {}",
                        e
                    )));
                }
            },
            Opcode::Binary => {
                self.event_queue
                    .push_back(ConnectionEvent::BinaryMessage(payload));
            }
            _ => {}
        }
    }

    fn handle_close(&mut self, frame: Frame, _now: Timestamp) -> Result<(), Error> {
        self.close_received = true;

        let (code, reason) = if frame.payload.len() >= 2 {
            let code_val = u16::from_be_bytes([frame.payload[0], frame.payload[1]]);
            let reason = String::from_utf8_lossy(&frame.payload[2..]).to_string();
            (Some(CloseCode::new(code_val)), reason)
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

    /// テスト用: nonce から期待される Sec-WebSocket-Accept を計算
    #[cfg(test)]
    fn compute_expected_accept(&self) -> String {
        use base64::Engine;
        use base64::engine::general_purpose::STANDARD;
        use sha1::{Digest, Sha1};

        let key = STANDARD.encode(self.nonce);
        let mut hasher = Sha1::new();
        hasher.update(key.as_bytes());
        hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        let hash = hasher.finalize();
        STANDARD.encode(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // テスト用ヘルパー: 接続を作成してハンドシェイク完了まで進める
    fn create_connected_client_connection() -> WebSocketClientConnection {
        let options = ClientConnectionOptions::new("example.com", "/chat");
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        conn.connect(now).unwrap();

        // nonce から期待される Accept を計算
        let expected_accept = conn.compute_expected_accept();

        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             \r\n",
            expected_accept
        );
        conn.feed_recv_buf(response.as_bytes(), now).unwrap();

        // イベントをクリア
        while conn.poll_event().is_some() {}
        while conn.poll_output().is_some() {}

        conn
    }

    #[test]
    fn test_connection_new() {
        let options = ClientConnectionOptions::new("example.com", "/chat");
        let conn = WebSocketClientConnection::new(options);
        assert_eq!(conn.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_connection_connect() {
        let options = ClientConnectionOptions::new("example.com", "/chat");
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        conn.connect(now).unwrap();

        assert_eq!(conn.state(), ConnectionState::Connecting);

        // 出力を確認
        let output = conn.poll_output().unwrap();
        match output {
            ConnectionOutput::SendData(data) => {
                let s = String::from_utf8(data).unwrap();
                assert!(s.contains("GET /chat HTTP/1.1"));
                assert!(s.contains("Host: example.com"));
                assert!(s.contains("Upgrade: websocket"));
            }
            _ => panic!("expected SendData"),
        }

        // StateChanged イベントを確認
        let event = conn.poll_event().unwrap();
        assert_eq!(
            event,
            ConnectionEvent::StateChanged(ConnectionState::Connecting)
        );
    }

    #[test]
    fn test_connection_handshake() {
        let options = ClientConnectionOptions::new("example.com", "/chat");
        let mut conn = WebSocketClientConnection::new(options);
        let now = Timestamp::from_millis(0);

        conn.connect(now).unwrap();

        // nonce から期待される Accept を計算
        let expected_accept = conn.compute_expected_accept();

        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             \r\n",
            expected_accept
        );

        conn.feed_recv_buf(response.as_bytes(), now).unwrap();

        assert_eq!(conn.state(), ConnectionState::Connected);

        // イベントを確認
        let _state_event = conn.poll_event().unwrap(); // Connecting
        let _state_event = conn.poll_event().unwrap(); // Connected (StateChanged)
        let event = conn.poll_event().unwrap();
        match event {
            ConnectionEvent::Connected {
                protocol,
                extensions,
            } => {
                assert!(protocol.is_none());
                assert!(extensions.is_empty());
            }
            _ => panic!("expected Connected event"),
        }
    }

    #[test]
    fn test_send_text() {
        let mut conn = create_connected_client_connection();
        let now = Timestamp::from_millis(0);

        // テキスト送信
        conn.send_text("Hello", now).unwrap();

        let output = conn.poll_output().unwrap();
        match output {
            ConnectionOutput::SendData(data) => {
                // フレームがエンコードされていることを確認
                assert!(!data.is_empty());
                // 最初のバイト: FIN + Text opcode (0x81)
                assert_eq!(data[0], 0x81);
            }
            _ => panic!("expected SendData"),
        }
    }

    #[test]
    fn test_receive_text() {
        let mut conn = create_connected_client_connection();
        let now = Timestamp::from_millis(0);

        // サーバーからのテキストフレーム（マスクなし）
        // FIN=1, opcode=text(1), length=5, "Hello"
        let frame = [0x81, 0x05, b'H', b'e', b'l', b'l', b'o'];
        conn.feed_recv_buf(&frame, now).unwrap();

        let event = conn.poll_event().unwrap();
        assert_eq!(event, ConnectionEvent::TextMessage("Hello".to_string()));
    }

    #[test]
    fn test_close_handshake() {
        let mut conn = create_connected_client_connection();
        let now = Timestamp::from_millis(0);

        // クローズ送信
        conn.close(CloseCode::NORMAL, "goodbye", now).unwrap();

        assert_eq!(conn.state(), ConnectionState::Closing);

        // サーバーからクローズ応答
        // FIN=1, opcode=close(8), length=2, code=1000
        let close_frame = [0x88, 0x02, 0x03, 0xE8];
        conn.feed_recv_buf(&close_frame, now).unwrap();

        assert_eq!(conn.state(), ConnectionState::Closed);
    }

    #[test]
    fn test_ping_pong() {
        let mut conn = create_connected_client_connection();
        let now = Timestamp::from_millis(0);

        // サーバーから Ping
        // FIN=1, opcode=ping(9), length=4, "ping"
        let ping_frame = [0x89, 0x04, b'p', b'i', b'n', b'g'];
        conn.feed_recv_buf(&ping_frame, now).unwrap();

        // Ping イベント
        let event = conn.poll_event().unwrap();
        assert_eq!(event, ConnectionEvent::Ping(b"ping".to_vec()));

        // Pong 自動返信
        let output = conn.poll_output().unwrap();
        match output {
            ConnectionOutput::SendData(data) => {
                // Pong opcode = 0x8A
                assert_eq!(data[0], 0x8A);
            }
            _ => panic!("expected SendData for Pong"),
        }
    }
}
