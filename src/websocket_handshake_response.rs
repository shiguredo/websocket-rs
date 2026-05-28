//! WebSocket ハンドシェイクのレスポンス側
//!
//! - `ServerHandshakeResponse`: サーバーが返すレスポンスのビルダー
//! - `HandshakeResponse`: クライアントがデコード済みのレスポンス情報
//! - `HandshakeValidator`: クライアント側でレスポンスの ABNF / Sec-WebSocket-Accept を検証する

use shiguredo_http11::{HttpHead, ResponseDecoder, ResponseHead};

use crate::error::Error;
use crate::websocket_extension::Extension;
use crate::websocket_handshake::calculate_accept;

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
        // RFC 6455 Section 4.1: 101 以外の HTTP レスポンスは HTTP procedures に従って処理する
        if response.status_code() != 101 {
            return Err(Error::http_response(crate::error::HttpResponseInfo {
                status_code: response.status_code(),
                reason_phrase: response.reason_phrase().to_string(),
                headers: response
                    .headers()
                    .iter()
                    .map(|(name, value)| (name.as_str().to_string(), value.clone()))
                    .collect(),
            }));
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

        // RFC 6455 Section 4.2.2 / Section 11.3.3: Sec-WebSocket-Accept は単一値ヘッダー
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

        // RFC 6455 Section 4.2.2 / Section 11.3.4: Sec-WebSocket-Protocol は単一値ヘッダー
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

        // RFC 6455 Section 4.2.2 項目 6: 複数行の Sec-WebSocket-Extensions を許容し統合する
        let extension_values = response.get_headers("Sec-WebSocket-Extensions");
        // 現在は ',' で単純分割しており、extension-param 値が quoted-string の場合に誤分割する。
        // permessage-deflate (RFC 7692) のパラメータはすべて token のため実害はないが、
        // 将来の拡張実装時には RFC 9110 Section 5.6.1 の #rule に準拠した stateful パーサーへの置き換えが必要（未対応）。
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
        // RFC 6455 Section 9.1: extension-token / extension-param の ABNF を検証
        for ext in &extensions {
            Extension::parse_strict(ext).map_err(|e| {
                Error::handshake_rejected(format!("invalid Sec-WebSocket-Extensions value: {e}"))
            })?;
        }

        Ok(Some(HandshakeResponse {
            protocol,
            extensions,
        }))
    }
}
