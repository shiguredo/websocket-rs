//! WebSocket ハンドシェイクのリクエスト側
//!
//! - `HandshakeRequest`: クライアントが送出するリクエストのビルダー
//! - `ServerHandshakeRequest`: サーバーがデコード済みのリクエスト情報
//! - `HandshakeRequestValidator`: サーバー側でリクエストの ABNF / 必須ヘッダーを検証する

use std::collections::HashSet;

use base64ct::{Base64, Encoding};
use shiguredo_http11::{HeaderName, Request, RequestDecoder};

use crate::error::Error;
use crate::websocket_handshake::{validate_extension_entry, validate_key};

/// ハンドシェイクリクエストビルダー
#[derive(Debug, Clone)]
pub struct HandshakeRequest {
    /// リクエスト URI のパス部分
    pub path: String,
    /// Host ヘッダー
    pub host: String,
    /// Origin ヘッダー（オプション）
    pub origin: Option<String>,
    /// サブプロトコル（オプション）
    pub protocols: Vec<String>,
    /// 拡張（オプション）
    pub extensions: Vec<String>,
    /// 追加ヘッダー
    pub additional_headers: Vec<(String, String)>,
}

impl HandshakeRequest {
    /// 新しいハンドシェイクリクエストを生成する
    pub fn new(path: &str, host: &str) -> Self {
        Self {
            path: path.to_string(),
            host: host.to_string(),
            origin: None,
            protocols: Vec::new(),
            extensions: Vec::new(),
            additional_headers: Vec::new(),
        }
    }

    /// Origin ヘッダーを設定する
    pub fn origin(mut self, origin: &str) -> Self {
        self.origin = Some(origin.to_string());
        self
    }

    /// サブプロトコルを追加する
    pub fn protocol(mut self, protocol: &str) -> Self {
        self.protocols.push(protocol.to_string());
        self
    }

    /// 拡張を追加する
    pub fn extension(mut self, extension: &str) -> Self {
        self.extensions.push(extension.to_string());
        self
    }

    /// 追加ヘッダーを設定する
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.additional_headers
            .push((name.to_string(), value.to_string()));
        self
    }

    /// ハンドシェイクリクエストをビルドする
    ///
    /// 戻り値: エンコードされたリクエスト
    pub fn build(&self, nonce: [u8; 16]) -> Result<Vec<u8>, Error> {
        // RFC 6455 Section 4.2.1: Sec-WebSocket-Protocol の各要素は token でなければならない
        for p in &self.protocols {
            if !crate::token::is_valid_token(p) {
                return Err(Error::invalid_input(format!(
                    "invalid Sec-WebSocket-Protocol value: {}",
                    p
                )));
            }
        }
        // RFC 6455 Section 4.2.1: すべての要素が一意でなければならない
        {
            let mut seen = HashSet::new();
            for p in &self.protocols {
                if !seen.insert(p.as_str()) {
                    return Err(Error::invalid_input(format!(
                        "duplicate Sec-WebSocket-Protocol value: {}",
                        p
                    )));
                }
            }
        }

        // RFC 6455 Section 4.1: 予約済みヘッダーとの重複チェック
        // ライブラリが自動的に設定するヘッダーを additional_headers で重複指定させないための実装上の制約。
        // RFC 6455 Section 11.3 では Sec-WebSocket-Protocol (11.3.4) と
        // Sec-WebSocket-Extensions (11.3.2) はリクエストで複数回許容される
        // (> The |Sec-WebSocket-Protocol| header field MAY appear multiple times in an HTTP request)
        // ため、重複禁止は RFC の要件ではなく実装の都合である。
        const RESERVED: &[&str] = &[
            "host",
            "upgrade",
            "connection",
            "sec-websocket-key",
            "sec-websocket-version",
            "sec-websocket-protocol",
            "sec-websocket-extensions",
        ];
        for (name, _) in &self.additional_headers {
            if RESERVED.contains(&name.to_ascii_lowercase().as_str()) {
                return Err(Error::invalid_input(format!(
                    "additional header '{}' conflicts with a reserved WebSocket header",
                    name
                )));
            }
        }

        // RFC 6455 Section 4.1 (line 939-942):
        // Request-URI は origin-form (/ 始まり) または absolute http/https URI でなければならない (MUST)
        {
            let lower = self.path.to_ascii_lowercase();
            if !self.path.starts_with('/')
                && !lower.starts_with("http://")
                && !lower.starts_with("https://")
            {
                return Err(Error::invalid_input(format!(
                    "invalid path: must be origin-form or absolute http/https URI: {}",
                    self.path
                )));
            }
        }

        let key = Base64::encode_string(nonce.as_slice());

        let encoded = (|| {
            let mut request = Request::new("GET", &self.path)?
                .header("Host", &self.host)?
                .header("Upgrade", "websocket")?
                .header("Connection", "Upgrade")?
                .header("Sec-WebSocket-Key", &key)?
                .header("Sec-WebSocket-Version", "13")?;

            if let Some(origin) = &self.origin {
                request = request.header("Origin", origin)?;
            }

            if !self.protocols.is_empty() {
                request = request.header("Sec-WebSocket-Protocol", self.protocols.join(", "))?;
            }

            if !self.extensions.is_empty() {
                request = request.header("Sec-WebSocket-Extensions", self.extensions.join(", "))?;
            }

            for (name, value) in &self.additional_headers {
                request = request.header(HeaderName::new(name)?, value)?;
            }

            request.encode()
        })()
        .map_err(|e| Error::invalid_input(e.to_string()))?;

        Ok(encoded)
    }
}

/// サーバー側ハンドシェイクリクエスト
#[derive(Debug, Clone)]
pub struct ServerHandshakeRequest {
    /// リクエスト URI のパス部分
    pub path: String,
    /// Host ヘッダー
    pub host: String,
    /// Origin ヘッダー（オプション）
    pub origin: Option<String>,
    /// サブプロトコル候補
    pub protocols: Vec<String>,
    /// 拡張候補
    pub extensions: Vec<String>,
    /// Sec-WebSocket-Key の値
    pub key: String,
}

/// サーバー側ハンドシェイクリクエストバリデーター
pub struct HandshakeRequestValidator {
    decoder: RequestDecoder,
    decode_error: Option<String>,
}

impl HandshakeRequestValidator {
    pub fn new() -> Self {
        Self {
            decoder: RequestDecoder::new(),
            decode_error: None,
        }
    }

    /// 受信データをフィードする
    pub fn feed(&mut self, data: &[u8]) {
        if self.decode_error.is_none()
            && let Err(err) = self.decoder.feed(data)
        {
            self.decode_error = Some(err.to_string());
        }
    }

    /// 残りのデータを取得する（フレームデータ用）
    pub fn remaining(&self) -> &[u8] {
        self.decoder.remaining()
    }

    /// デコーダーをリセットする
    pub fn reset(&mut self) {
        self.decoder.reset();
        self.decode_error = None;
    }

    /// リクエストを検証する
    pub fn validate(&mut self) -> Result<Option<ServerHandshakeRequest>, Error> {
        if let Some(reason) = self.decode_error.as_deref() {
            return Err(Error::invalid_data(reason));
        }

        let request = match self
            .decoder
            .decode()
            .map_err(|err| Error::invalid_data(err.to_string()))?
        {
            Some(r) => r,
            None => return Ok(None),
        };

        self.validate_request(&request)
    }

    fn validate_request(&self, request: &Request) -> Result<Option<ServerHandshakeRequest>, Error> {
        if request.method() != "GET" {
            return Err(Error::handshake_rejected(format!(
                "unexpected method: {}",
                request.method()
            )));
        }

        if request.version() != "HTTP/1.1" {
            return Err(Error::handshake_rejected(format!(
                "unexpected HTTP version: {}",
                request.version()
            )));
        }

        // RFC 6455 Section 4.2.1 (line 1143-1146), Section 4.1 (line 939-942):
        // Request-URI は origin-form (/ 始まり) または absolute http/https URI でなければならない。
        // HTTP デコーダーが origin-form / absolute-form の構文検証と
        // GET メソッドへの authority-form / asterisk-form 拒否を担保しているが、
        // absolute-form のスキームが http/https 以外 (ws/wss 等) の場合は WebSocket 層で拒否する。
        if !request.uri().starts_with('/') {
            let lower = request.uri().to_ascii_lowercase();
            if !lower.starts_with("http://") && !lower.starts_with("https://") {
                return Err(Error::handshake_rejected(format!(
                    "invalid Request-URI: must be origin-form or absolute http/https URI: {}",
                    request.uri()
                )));
            }
        }

        let host = request
            .get_header("Host")
            .ok_or_else(|| Error::handshake_rejected("missing Host header"))?
            .to_string();

        // RFC 6455 Section 4.2.1: Upgrade ヘッダーに "websocket" トークンが含まれていること
        // RFC 9110 Section 5.3: 同名ヘッダーが複数行の場合は統合して評価する
        {
            let upgrade_values = request.get_headers("Upgrade");
            if upgrade_values.is_empty() {
                return Err(Error::handshake_rejected("missing Upgrade header"));
            }
            let has_websocket = upgrade_values.iter().any(|v| {
                v.split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("websocket"))
            });
            if !has_websocket {
                return Err(Error::handshake_rejected(format!(
                    "invalid Upgrade header: {}",
                    upgrade_values.join(", ")
                )));
            }
        }

        {
            let connection_values = request.get_headers("Connection");
            if connection_values.is_empty() {
                return Err(Error::handshake_rejected("missing Connection header"));
            }
            let has_upgrade = connection_values.iter().any(|v| {
                v.split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
            });
            if !has_upgrade {
                return Err(Error::handshake_rejected(format!(
                    "invalid Connection header: {}",
                    connection_values.join(", ")
                )));
            }
        }

        // RFC 6455 Section 4.2.1 / Section 11.3.5: Sec-WebSocket-Version は単一値ヘッダー
        {
            let version_values = request.get_headers("Sec-WebSocket-Version");
            if version_values.len() > 1 {
                return Err(Error::handshake_rejected(
                    "duplicate Sec-WebSocket-Version header",
                ));
            }
        }
        match request.get_header("Sec-WebSocket-Version") {
            Some("13") => {}
            Some(v) => {
                return Err(Error::version_not_supported(v));
            }
            None => return Err(Error::handshake_rejected("missing Sec-WebSocket-Version")),
        }

        // RFC 6455 Section 4.1 / Section 11.3.1: Sec-WebSocket-Key は単一値ヘッダー
        {
            let key_values = request.get_headers("Sec-WebSocket-Key");
            if key_values.len() > 1 {
                return Err(Error::handshake_rejected(
                    "duplicate Sec-WebSocket-Key header",
                ));
            }
        }
        let key = request
            .get_header("Sec-WebSocket-Key")
            .ok_or_else(|| Error::handshake_rejected("missing Sec-WebSocket-Key"))?
            .to_string();
        validate_key(&key)?;

        // RFC 6455 Section 4.1 item 10:
        // Sec-WebSocket-Protocol の各要素は token (1*tchar) でなければならない (MUST)
        // すべての要素が一意でなければならない (MUST)
        // RFC 9110 Section 5.3: 同名ヘッダーが複数行の場合はリスト値として統合する
        let protocols = {
            let values = request.get_headers("Sec-WebSocket-Protocol");
            let protocols: Vec<String> = values
                .iter()
                .flat_map(|v| v.split(','))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            // RFC 6455 Section 4.2.1: 1#token
            // ヘッダーが存在するが有効な token が 0 個の場合は ABNF 違反
            if !values.is_empty() && protocols.is_empty() {
                return Err(Error::handshake_rejected(
                    "malformed Sec-WebSocket-Protocol header: no valid protocols",
                ));
            }
            for p in &protocols {
                if !crate::token::is_valid_token(p) {
                    return Err(Error::handshake_rejected(format!(
                        "invalid Sec-WebSocket-Protocol value: {}",
                        p
                    )));
                }
            }
            let mut seen = HashSet::new();
            for p in &protocols {
                if !seen.insert(p.as_str()) {
                    return Err(Error::handshake_rejected(format!(
                        "duplicate Sec-WebSocket-Protocol value: {}",
                        p
                    )));
                }
            }
            protocols
        };

        // RFC 6455 Section 9.1: 複数の Sec-WebSocket-Extensions ヘッダー行は許容される
        // RFC 9110 Section 5.3: 同名ヘッダーが複数行の場合はリスト値として統合する
        let extensions = {
            let values = request.get_headers("Sec-WebSocket-Extensions");
            // 現在は ',' で単純分割しており、extension-param 値が quoted-string の場合に誤分割する。
            // permessage-deflate (RFC 7692) のパラメータはすべて token のため実害はないが、
            // 将来の拡張実装時には RFC 9110 Section 5.6.1 の #rule に準拠した stateful パーサーへの置き換えが必要（未対応）。
            let extensions: Vec<String> = values
                .iter()
                .flat_map(|v| v.split(','))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            // RFC 6455 Section 9.1: extension-list = 1#extension
            // ヘッダーが存在するが有効な拡張が 0 個の場合は ABNF 違反
            if !values.is_empty() && extensions.is_empty() {
                return Err(Error::handshake_rejected(
                    "malformed Sec-WebSocket-Extensions header: no valid extensions",
                ));
            }
            // RFC 6455 Section 9.1: extension-token / extension-param の ABNF を検証
            for ext in &extensions {
                validate_extension_entry(ext)?;
            }
            extensions
        };

        let origin = request.get_header("Origin").map(String::from);

        Ok(Some(ServerHandshakeRequest {
            path: request.uri().to_string(),
            host,
            origin,
            protocols,
            extensions,
            key,
        }))
    }
}

impl Default for HandshakeRequestValidator {
    fn default() -> Self {
        Self::new()
    }
}
