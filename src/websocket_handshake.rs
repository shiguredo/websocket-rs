use std::collections::HashSet;

use crate::error::Error;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha1::{Digest, Sha1};
use shiguredo_http11::{HttpHead, Request, RequestDecoder, ResponseDecoder, ResponseHead};

/// WebSocket ハンドシェイクで使用する固定 GUID (RFC 6455 Section 1.3)
const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

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
            if !is_valid_token(p) {
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

        let key = STANDARD.encode(nonce);

        let mut request = Request::new("GET", &self.path)
            .header("Host", &self.host)
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Key", &key)
            .header("Sec-WebSocket-Version", "13");

        if let Some(origin) = &self.origin {
            request = request.header("Origin", origin);
        }

        if !self.protocols.is_empty() {
            request = request.header("Sec-WebSocket-Protocol", &self.protocols.join(", "));
        }

        if !self.extensions.is_empty() {
            request = request.header("Sec-WebSocket-Extensions", &self.extensions.join(", "));
        }

        for (name, value) in &self.additional_headers {
            request = request.header(name, value);
        }

        Ok(request.encode())
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

/// サーバー側ハンドシェイクレスポンス
#[derive(Debug, Clone, Default)]
pub struct ServerHandshakeResponse {
    /// 選択したサブプロトコル
    pub protocol: Option<String>,
    /// 選択した拡張
    pub extensions: Vec<String>,
    /// 追加ヘッダー
    pub additional_headers: Vec<(String, String)>,
}

impl ServerHandshakeResponse {
    pub fn new() -> Self {
        Self::default()
    }

    /// サブプロトコルを設定する
    pub fn protocol(mut self, protocol: &str) -> Self {
        self.protocol = Some(protocol.to_string());
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
        if request.method != "GET" {
            return Err(Error::handshake_rejected(format!(
                "unexpected method: {}",
                request.method
            )));
        }

        if request.version != "HTTP/1.1" {
            return Err(Error::handshake_rejected(format!(
                "unexpected HTTP version: {}",
                request.version
            )));
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

        // RFC 6455 Section 4.2.1: Sec-WebSocket-Version は単一値ヘッダー
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
                return Err(Error::handshake_rejected(format!(
                    "unsupported WebSocket version: {}",
                    v
                )));
            }
            None => return Err(Error::handshake_rejected("missing Sec-WebSocket-Version")),
        }

        // RFC 6455 Section 4.2.1: Sec-WebSocket-Key は単一値ヘッダー
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

        // RFC 6455 Section 4.2.1 step 10:
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
                if !is_valid_token(p) {
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
            // RFC 6455 Section 9.1: extension-token は token ABNF に準拠しなければならない
            for ext in &extensions {
                let parts: Vec<&str> = ext.split(';').collect();
                let token = parts[0].trim();
                if !is_valid_token(token) {
                    return Err(Error::handshake_rejected(format!(
                        "invalid Sec-WebSocket-Extensions token: {}",
                        token
                    )));
                }
                // RFC 6455 Section 9.1: extension = extension-token *( ";" extension-param )
                // ";" の後は必ず extension-param が必要。trailing ';' は ABNF 違反。
                for part in parts.iter().skip(1) {
                    if part.trim().is_empty() {
                        return Err(Error::handshake_rejected(format!(
                            "trailing ';' in Sec-WebSocket-Extensions: '{}'",
                            ext
                        )));
                    }
                }
            }
            extensions
        };

        let origin = request.get_header("Origin").map(String::from);

        Ok(Some(ServerHandshakeRequest {
            path: request.uri.clone(),
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

/// ハンドシェイクレスポンスの検証結果
#[derive(Debug, Clone)]
pub struct HandshakeResponse {
    /// ネゴシエートされたサブプロトコル
    pub protocol: Option<String>,
    /// ネゴシエートされた拡張
    pub extensions: Vec<String>,
}

/// ハンドシェイクレスポンスバリデーター
pub struct HandshakeValidator {
    decoder: ResponseDecoder,
    expected_accept: String,
    decode_error: Option<String>,
}

impl HandshakeValidator {
    /// 新しいバリデーターを生成する
    pub fn new(nonce: [u8; 16]) -> Self {
        let expected_accept = calculate_accept(&nonce);
        Self {
            decoder: ResponseDecoder::new(),
            expected_accept,
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

    /// レスポンスを検証する
    pub fn validate(&mut self) -> Result<Option<HandshakeResponse>, Error> {
        if let Some(reason) = self.decode_error.as_deref() {
            return Err(Error::invalid_data(reason));
        }

        // WebSocket ハンドシェイクレスポンスにボディは不要なので、
        // ヘッダーが揃った時点で検証する。
        // decode() を使うと、非 101 レスポンスでは BodyKind::CloseDelimited となり
        // mark_eof() が呼ばれるまで None を返し続けてしまう。
        let head = match self
            .decoder
            .decode_headers()
            .map_err(|err| Error::invalid_data(err.to_string()))?
        {
            Some((head, _body_kind)) => head,
            None => return Ok(None),
        };

        self.validate_response(&head)
    }

    fn validate_response(
        &self,
        response: &ResponseHead,
    ) -> Result<Option<HandshakeResponse>, Error> {
        // ステータスコードの検証
        if response.status_code != 101 {
            return Err(Error::handshake_rejected(format!(
                "unexpected status code: {} {}",
                response.status_code, response.reason_phrase
            )));
        }

        // Upgrade ヘッダーの検証（トークンとして検証）
        // RFC 9110 Section 5.3: 同名ヘッダーが複数行の場合は統合して評価する
        {
            let upgrade_values = response.get_headers("Upgrade");
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

        // Connection ヘッダーの検証（トークンとして検証）
        // RFC 9110 Section 5.3: 同名ヘッダーが複数行の場合は統合して評価する
        {
            let connection_values = response.get_headers("Connection");
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

        // RFC 6455 Section 4.2.2: Sec-WebSocket-Accept は単一値ヘッダー
        {
            let accept_values = response.get_headers("Sec-WebSocket-Accept");
            if accept_values.len() > 1 {
                return Err(Error::handshake_rejected(
                    "duplicate Sec-WebSocket-Accept header",
                ));
            }
        }
        // Sec-WebSocket-Accept ヘッダーの検証
        match response.get_header("Sec-WebSocket-Accept") {
            Some(v) if v == self.expected_accept => {}
            Some(v) => {
                return Err(Error::handshake_rejected(format!(
                    "invalid Sec-WebSocket-Accept: expected {}, got {}",
                    self.expected_accept, v
                )));
            }
            None => {
                return Err(Error::handshake_rejected(
                    "missing Sec-WebSocket-Accept header",
                ));
            }
        }

        // RFC 6455 Section 4.2.2: Sec-WebSocket-Protocol は単一値ヘッダー
        {
            let protocol_values = response.get_headers("Sec-WebSocket-Protocol");
            if protocol_values.len() > 1 {
                return Err(Error::handshake_rejected(
                    "duplicate Sec-WebSocket-Protocol header",
                ));
            }
        }
        // サブプロトコルの取得
        let protocol = response
            .get_header("Sec-WebSocket-Protocol")
            .map(String::from);

        // 拡張の取得
        // RFC 6455 Section 11.3.2: HTTP レスポンスでは Sec-WebSocket-Extensions は 1 回のみ許容
        let extension_values = response.get_headers("Sec-WebSocket-Extensions");
        if extension_values.len() > 1 {
            return Err(Error::handshake_rejected(
                "duplicate Sec-WebSocket-Extensions header in HTTP response",
            ));
        }
        let extensions: Vec<String> = extension_values
            .iter()
            .flat_map(|v| v.split(','))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        // RFC 6455 Section 9.1: extension-list = 1#extension
        // ヘッダーが存在するが有効な拡張が 0 個の場合は ABNF 違反
        if !extension_values.is_empty() && extensions.is_empty() {
            return Err(Error::handshake_rejected(
                "malformed Sec-WebSocket-Extensions header: no valid extensions",
            ));
        }
        // RFC 6455 Section 9.1: extension-token は token ABNF に準拠しなければならない
        for ext in &extensions {
            let token = ext.split(';').next().unwrap_or("").trim();
            if !is_valid_token(token) {
                return Err(Error::handshake_rejected(format!(
                    "invalid Sec-WebSocket-Extensions token: {}",
                    token
                )));
            }
        }

        Ok(Some(HandshakeResponse {
            protocol,
            extensions,
        }))
    }
}

/// Sec-WebSocket-Accept の値を計算する
pub fn calculate_accept(nonce: &[u8; 16]) -> String {
    let key = STANDARD.encode(nonce);
    calculate_accept_from_key(&key)
}

pub fn calculate_accept_from_key(key: &str) -> String {
    let combined = format!("{}{}", key, WEBSOCKET_GUID);

    let mut hasher = Sha1::new();
    hasher.update(combined.as_bytes());
    let hash = hasher.finalize();

    STANDARD.encode(hash)
}

/// RFC 7230 の token ABNF に準拠するかチェックする
///
/// token = 1*tchar
/// tchar = "!" / "#" / "$" / "%" / "&" / "'" / "*" / "+" / "-" / "." /
///         "^" / "_" / "`" / "|" / "~" / DIGIT / ALPHA
fn is_valid_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| {
            matches!(b,
                b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' |
                b'^' | b'_' | b'`' | b'|' | b'~' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z'
            )
        })
}

fn validate_key(key: &str) -> Result<(), Error> {
    let decoded = STANDARD
        .decode(key)
        .map_err(|_| Error::handshake_rejected("invalid Sec-WebSocket-Key"))?;
    if decoded.len() != 16 {
        return Err(Error::handshake_rejected("invalid Sec-WebSocket-Key"));
    }
    Ok(())
}
