//! WebSocket サーバー接続 (Sans I/O パターン)
//!
//! WebSocket サーバー接続を管理する状態機械。
//! I/O は外部で行い、この構造体はバッファ駆動型で動作する。

use crate::deflate::PerMessageDeflate;
use crate::error::{Error, ErrorKind};
use crate::frame_policy::ServerFramePolicy;
use crate::websocket_close::CloseCode;
use crate::websocket_connection_shared::{
    DEFAULT_MAX_DECOMPRESSED_SIZE, DEFAULT_MAX_FRAME_SIZE, DEFAULT_MAX_MESSAGE_SIZE,
    SharedConnectionState,
};
use crate::websocket_connection_types::{
    ConnectionEvent, ConnectionOutput, ConnectionState, TimerId,
};
use crate::websocket_extension::{Extension, PerMessageDeflateConfig};
use crate::websocket_handshake::calculate_accept_from_key;
use crate::websocket_handshake_request::{HandshakeRequestValidator, ServerHandshakeRequest};
use crate::websocket_handshake_response::ServerHandshakeResponse;
use shiguredo_http11::{HeaderName, Response};

/// ハンドシェイク受理待ち中の最大バッファサイズ（1MB）
const MAX_PENDING_FRAME_DATA: usize = 1024 * 1024;

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
    /// 最大フレームサイズ（メモリ DoS 対策）
    pub max_frame_size: usize,
    /// 最大メッセージサイズ（フラグメント累積サイズ制限）
    pub max_message_size: usize,
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
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
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

/// WebSocket サーバー接続
pub struct WebSocketServerConnection {
    /// 共通接続状態
    shared: SharedConnectionState,
    /// サーバー側フレームポリシー
    policy: ServerFramePolicy,
    /// オプション
    options: ServerConnectionOptions,
    /// ハンドシェイクバリデーター
    handshake_validator: HandshakeRequestValidator,
    /// 受信済みハンドシェイクリクエスト
    pending_request: Option<ServerHandshakeRequest>,
    /// ハンドシェイク後に処理する残りデータ
    pending_frame_data: Vec<u8>,
    /// ネゴシエートされたサブプロトコル
    negotiated_protocol: Option<String>,
    /// ネゴシエートされた拡張
    negotiated_extensions: Vec<String>,
}

impl WebSocketServerConnection {
    /// 新しい接続を作成
    pub fn new(options: ServerConnectionOptions) -> Self {
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
            policy: ServerFramePolicy,
            options,
            handshake_validator: HandshakeRequestValidator::new(),
            pending_request: None,
            pending_frame_data: Vec::new(),
            negotiated_protocol: None,
            negotiated_extensions: Vec::new(),
        }
    }

    /// 現在の状態を取得
    pub fn state(&self) -> ConnectionState {
        self.shared.state()
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
    /// RFC 6455 Section 7.1.7: このメソッドが Err を返した後は
    /// 以降の呼び出しも即座に Err を返す。
    pub fn feed_recv_buf(&mut self, buf: &[u8]) -> Result<(), Error> {
        if self.shared.is_failed() {
            return Err(Error::invalid_state("connection has failed"));
        }
        let result = match self.shared.state() {
            ConnectionState::Disconnected | ConnectionState::Connecting => {
                self.process_handshake(buf)
            }
            ConnectionState::Connected | ConnectionState::Closing => {
                self.shared.process_frames(buf, &mut self.policy)
            }
            ConnectionState::Closed => {
                return Err(Error::invalid_state("connection is closed"));
            }
        };
        if result.is_err() {
            self.shared.mark_failed();
        }
        result
    }

    /// ハンドシェイクを自動で受諾する
    ///
    /// # セキュリティ上の注意
    ///
    /// この関数は `ServerHandshakeRequest` が保持する `origin` および `path` を検証しない。
    /// ブラウザ + Cookie 認証の環境でこの関数を使用すると、
    /// CSWSH (Cross-Site WebSocket Hijacking) の踏み台になる可能性がある。
    ///
    /// 信頼できないクライアントからの接続を受け付ける場合は、
    /// `pending_request()` で `ServerHandshakeRequest` を取得し、
    /// `origin` / `path` を自前で検証したうえで `accept_handshake()` を呼ぶこと。
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
        if self.shared.state() != ConnectionState::Connecting {
            return Err(Error::invalid_state("handshake is not in progress"));
        }

        let request = self
            .pending_request
            .take()
            .ok_or_else(|| Error::invalid_state("handshake request not available"))?;

        Self::validate_handshake_response(&request, &response)?;
        // 旧来の検証順序を維持するため、PMCE ネゴシエーションより先にレスポンスを
        // 組み立てて additional_headers の ABNF token 検証を済ませる
        let encoded = Self::build_handshake_response(&request, &response)?;
        let deflate = Self::negotiate_deflate(&request, &response)?;

        self.shared
            .enqueue_output(ConnectionOutput::SendData(encoded));

        if let Some(deflate) = deflate {
            self.shared.enable_deflate(deflate);
        }
        self.negotiated_protocol = response.protocol.clone();
        self.negotiated_extensions = response.extensions.clone();

        // accept_handshake は冒頭で Connecting であることをガード済み。
        // 通常は Ok を返すが、前提崩れに備えて `?` で伝播する
        self.shared.set_state(ConnectionState::Connected)?;
        self.shared.emit_event(ConnectionEvent::Connected {
            protocol: self.negotiated_protocol.clone(),
            extensions: self.negotiated_extensions.clone(),
        });

        // Ping タイマー設定
        if self.options.ping_interval_millis > 0 {
            self.shared.enqueue_output(ConnectionOutput::SetTimer {
                id: TimerId::Ping,
                duration_millis: self.options.ping_interval_millis,
            });
        }

        if !self.pending_frame_data.is_empty() {
            let pending = std::mem::take(&mut self.pending_frame_data);
            self.shared.process_frames(&pending, &mut self.policy)?;
        }

        self.handshake_validator.reset();
        Ok(())
    }

    /// レスポンス本体の事前検証 (protocol / extensions / 予約済みヘッダー)
    fn validate_handshake_response(
        request: &ServerHandshakeRequest,
        response: &ServerHandshakeResponse,
    ) -> Result<(), Error> {
        if let Some(protocol) = &response.protocol
            && !request.protocols.iter().any(|p| p == protocol)
        {
            return Err(Error::handshake_rejected(format!(
                "unsupported protocol: {}",
                protocol
            )));
        }

        for extension in &response.extensions {
            // RFC 6455 Section 9.1: ABNF 不適合の拡張文字列は接続を失敗させなければならない (MUST)。
            // ここはサーバー自身が組み立てたレスポンスの拡張文字列を送信前に自己検証する経路で、
            // client が Section 9.1 により Fail the WebSocket Connection を発動する事態を未然に防ぐ。
            // parse_strict を使い、部分的に不正な拡張を見逃さないようにする
            let parsed = Extension::parse_strict(extension).map_err(|e| {
                Error::handshake_rejected(format!("invalid extension response '{extension}': {e}"))
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

        // RFC 7692 Section 7 / 7.1.3: サーバーは offer から 1 つの permessage-deflate を選んで返す。
        // 複数の permessage-deflate 要素を含むレスポンスは不正とみなす。
        {
            let pmce_count: usize = response
                .extensions
                .iter()
                .flat_map(|s| Extension::parse(s))
                .filter(|e| e.name == "permessage-deflate")
                .count();
            if pmce_count > 1 {
                return Err(Error::handshake_rejected(
                    "response contains multiple permessage-deflate elements",
                ));
            }
        }

        // RFC 6455 Section 4.2.2: 予約済みヘッダーとの重複チェック
        // ライブラリが自動的に設定するヘッダーを additional_headers で重複指定させないための実装上の制約。
        // HTTP response に対しては以下のヘッダーが重複禁止:
        // > the |Sec-WebSocket-Extensions| header field MUST NOT appear more than once in an HTTP response
        //   (Section 11.3.2)
        // > The |Sec-WebSocket-Accept| header MUST NOT appear more than once in an HTTP response.
        //   (Section 11.3.3)
        // > the |Sec-WebSocket-Protocol| header field MUST NOT appear more than once in an HTTP response
        //   (Section 11.3.4)
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

        Ok(())
    }

    /// permessage-deflate (RFC 7692) のネゴシエーション結果を解析し、コーデックを返す
    fn negotiate_deflate(
        request: &ServerHandshakeRequest,
        response: &ServerHandshakeResponse,
    ) -> Result<Option<PerMessageDeflate>, Error> {
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
                    // (noflate の制約) のため、server_max_window_bits < 15 は
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
        Ok(deflate)
    }

    /// 101 Switching Protocols レスポンスをエンコードする
    fn build_handshake_response(
        request: &ServerHandshakeRequest,
        response: &ServerHandshakeResponse,
    ) -> Result<Vec<u8>, Error> {
        let accept = calculate_accept_from_key(&request.key);
        let mut response_builder = Response::new(101, "Switching Protocols")
            .map_err(|e| Error::invalid_input(e.to_string()))?
            .header("Upgrade", "websocket")
            .map_err(|e| Error::invalid_input(e.to_string()))?
            .header("Connection", "Upgrade")
            .map_err(|e| Error::invalid_input(e.to_string()))?
            .header("Sec-WebSocket-Accept", &accept)
            .map_err(|e| Error::invalid_input(e.to_string()))?;

        if let Some(protocol) = &response.protocol {
            response_builder = response_builder
                .header("Sec-WebSocket-Protocol", protocol)
                .map_err(|e| Error::invalid_input(e.to_string()))?;
        }

        if !response.extensions.is_empty() {
            response_builder = response_builder
                .header("Sec-WebSocket-Extensions", response.extensions.join(", "))
                .map_err(|e| Error::invalid_input(e.to_string()))?;
        }

        for (name, value) in &response.additional_headers {
            let header_name =
                HeaderName::new(name).map_err(|e| Error::invalid_input(e.to_string()))?;
            response_builder = response_builder
                .header(header_name, value)
                .map_err(|e| Error::invalid_input(e.to_string()))?;
        }

        response_builder
            .encode()
            .map_err(|e| Error::invalid_input(e.to_string()))
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
        if self.shared.state() != ConnectionState::Connecting {
            return Err(Error::invalid_state("handshake is not in progress"));
        }

        self.pending_request = None;
        self.pending_frame_data.clear();
        self.handshake_validator.reset();

        let mut response = Response::new(status_code, reason)
            .map_err(|e| Error::invalid_input(e.to_string()))?
            .header("Connection", "close")
            .map_err(|e| Error::invalid_input(e.to_string()))?;
        for (name, value) in headers {
            let header_name =
                HeaderName::new(*name).map_err(|e| Error::invalid_input(e.to_string()))?;
            response = response
                .header(header_name, *value)
                .map_err(|e| Error::invalid_input(e.to_string()))?;
        }
        let encoded = response
            .encode()
            .map_err(|e| Error::invalid_input(e.to_string()))?;
        self.shared
            .enqueue_output(ConnectionOutput::SendData(encoded));

        // reject_handshake は冒頭で Connecting であることをガード済み。
        // 通常は Ok を返すが、前提崩れに備えて `?` で伝播する
        self.shared.set_state(ConnectionState::Closed)?;
        self.shared
            .enqueue_output(ConnectionOutput::CloseConnection);
        Ok(())
    }

    /// テキストメッセージを送信
    pub fn send_text(&mut self, text: &str) -> Result<(), Error> {
        self.shared.send_text(text, &mut self.policy)
    }

    /// バイナリメッセージを送信
    pub fn send_binary(&mut self, data: &[u8]) -> Result<(), Error> {
        self.shared.send_binary(data, &mut self.policy)
    }

    /// Ping を送信
    ///
    /// RFC 6455 Section 5.5: data は 125 バイト以下でなければならない
    pub fn send_ping(&mut self, data: &[u8]) -> Result<(), Error> {
        self.shared.send_ping(data, &mut self.policy)
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

    fn process_handshake(&mut self, buf: &[u8]) -> Result<(), Error> {
        if self.pending_request.is_some() {
            if self.pending_frame_data.len() + buf.len() > MAX_PENDING_FRAME_DATA {
                return Err(Error::protocol_violation(
                    "pending frame data exceeds limit while awaiting handshake acceptance",
                ));
            }
            self.pending_frame_data.extend_from_slice(buf);
            return Ok(());
        }

        if self.shared.state() == ConnectionState::Disconnected {
            // 直前の if で Disconnected を確認済み。前提崩れに備えて `?` で伝播
            self.shared.set_state(ConnectionState::Connecting)?;
        }

        self.handshake_validator.feed(buf);
        match self.handshake_validator.validate() {
            Ok(Some(request)) => {
                self.pending_request = Some(request);
                self.pending_frame_data
                    .extend_from_slice(self.handshake_validator.remaining());
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(e) if e.kind == ErrorKind::VersionNotSupported => {
                // RFC 6455 Section 4.2.2 / 4.4: バージョン不一致時は 426 + Sec-WebSocket-Version: 13 を送信
                self.reject_handshake(426, "Upgrade Required", &[("Sec-WebSocket-Version", "13")])?;
                Err(e)
            }
            Err(e) => {
                // RFC 6455 Section 4.2.1: ハンドシェイク違反時は HTTP エラーレスポンスを返す (MUST)
                self.reject_handshake(400, "Bad Request", &[])?;
                Err(e)
            }
        }
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
                            // noflate は window_bits=15 固定のため、15 未満の offer はサポートできない。
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
