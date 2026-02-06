use std::collections::HashSet;

/// WebSocket 拡張パラメータ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionParam {
    pub name: String,
    pub value: Option<String>,
}

/// 拡張パースのコンテキスト
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionParseContext {
    /// クライアントがサーバーレスポンスをパースする
    ClientResponse,
    /// サーバーがクライアントリクエストをパースする
    ServerRequest,
}

/// 拡張パースエラー
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionParseError {
    /// 拡張名が permessage-deflate ではない
    NotDeflate,
    /// 未定義のパラメータ
    UnknownParameter(String),
    /// 重複したパラメータ
    DuplicateParameter(String),
    /// 値が必要なパラメータに値がない
    MissingValue(String),
    /// 値が不要なパラメータに値がある
    UnexpectedValue(String),
    /// 値が不正（範囲外など）
    InvalidValue(String),
}

/// WebSocket 拡張ネゴシエーション結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extension {
    pub name: String,
    pub params: Vec<ExtensionParam>,
}

impl Extension {
    /// 新しい拡張を生成する
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            params: Vec::new(),
        }
    }

    /// パラメータを追加する
    pub fn param(mut self, name: &str, value: Option<&str>) -> Self {
        self.params.push(ExtensionParam {
            name: name.to_string(),
            value: value.map(String::from),
        });
        self
    }

    /// パラメータを取得する
    pub fn get_param(&self, name: &str) -> Option<&ExtensionParam> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Sec-WebSocket-Extensions ヘッダー値としてエンコードする
    pub fn encode(&self) -> String {
        let mut s = self.name.clone();
        for param in &self.params {
            s.push_str("; ");
            s.push_str(&param.name);
            if let Some(value) = &param.value {
                s.push('=');
                s.push_str(value);
            }
        }
        s
    }

    /// Sec-WebSocket-Extensions ヘッダー値をパースする
    ///
    /// RFC 6455 Section 9.1 の ABNF に従い、quoted-string をサポートする。
    /// quoted-string 内の `,` / `;` は区切り文字として扱わない。
    /// 復号後の値が token ABNF に準拠しない場合、その拡張は除外する。
    pub fn parse(s: &str) -> Vec<Extension> {
        Self::split_respecting_quotes(s, b',')
            .into_iter()
            .filter_map(|ext| {
                let ext = ext.trim();
                if ext.is_empty() {
                    return None;
                }

                let parts = Self::split_respecting_quotes(ext, b';');
                let mut parts_iter = parts.into_iter();
                let name = parts_iter.next()?.trim().to_string();
                if name.is_empty() {
                    return None;
                }

                let mut params = Vec::new();
                for p in parts_iter {
                    let p = p.trim();
                    if p.is_empty() {
                        continue;
                    }

                    if let Some((param_name, value)) = p.split_once('=') {
                        let value = value.trim();
                        // RFC 6455 Section 9.1: 値が token に準拠しない場合は拡張全体を除外
                        let parsed_value = Self::parse_param_value(value)?;
                        params.push(ExtensionParam {
                            name: param_name.trim().to_string(),
                            value: Some(parsed_value),
                        });
                    } else {
                        params.push(ExtensionParam {
                            name: p.to_string(),
                            value: None,
                        });
                    }
                }

                Some(Extension { name, params })
            })
            .collect()
    }

    /// quoted-string を考慮して区切り文字で分割する
    fn split_respecting_quotes(s: &str, delimiter: u8) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut start = 0;
        let mut in_quotes = false;
        let bytes = s.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            let b = bytes[i];
            if in_quotes {
                if b == b'\\' {
                    // エスケープシーケンス: 次の文字をスキップ
                    i += 2;
                    continue;
                }
                if b == b'"' {
                    in_quotes = false;
                }
                i += 1;
                continue;
            }
            if b == b'"' {
                in_quotes = true;
                i += 1;
                continue;
            }
            if b == delimiter {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            i += 1;
        }
        parts.push(&s[start..]);
        parts
    }

    /// RFC 7230 の token ABNF に準拠するかチェック
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

    /// パラメータ値をパースする (quoted-string 対応)
    ///
    /// RFC 6455 Section 9.1:
    /// - quoted-string = DQUOTE *( qdtext / quoted-pair ) DQUOTE
    /// - quoted-pair = "\" ( HTAB / SP / VCHAR / obs-text )
    /// - 復号後の値は token ABNF に準拠する必要がある (MUST)
    ///
    /// 不正な値 (token 制約に違反) の場合は None を返す
    fn parse_param_value(value: &str) -> Option<String> {
        // quoted-string の場合
        if value.starts_with('"') && value.len() >= 2 {
            let inner = &value[1..];
            if let Some(end_quote) = Self::find_unescaped_quote(inner) {
                let quoted_content = &inner[..end_quote];
                let unescaped = Self::unescape_quoted_string(quoted_content);
                // RFC 6455 Section 9.1: 復号後の値は token ABNF に準拠する必要がある
                if Self::is_valid_token(&unescaped) {
                    return Some(unescaped);
                }
                return None;
            }
        }
        // token の場合: token として有効か検証
        if Self::is_valid_token(value) {
            Some(value.to_string())
        } else {
            None
        }
    }

    /// エスケープされていないダブルクォートの位置を探す
    fn find_unescaped_quote(s: &str) -> Option<usize> {
        let mut chars = s.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '\\' {
                // エスケープシーケンス: 次の文字をスキップ
                chars.next();
            } else if c == '"' {
                return Some(i);
            }
        }
        None
    }

    /// quoted-string のエスケープを解除する
    fn unescape_quoted_string(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                // エスケープシーケンス: 次の文字をそのまま追加
                if let Some(escaped) = chars.next() {
                    result.push(escaped);
                }
            } else {
                result.push(c);
            }
        }
        result
    }
}

/// permessage-deflate 拡張の設定 (RFC 7692)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PerMessageDeflateConfig {
    /// サーバーの最大ウィンドウビット (8-15)
    pub server_max_window_bits: Option<u8>,
    /// クライアントの最大ウィンドウビット (8-15)
    pub client_max_window_bits: Option<u8>,
    /// サーバーがコンテキストを引き継がない
    pub server_no_context_takeover: bool,
    /// クライアントがコンテキストを引き継がない
    pub client_no_context_takeover: bool,
}

impl PerMessageDeflateConfig {
    /// 新しい設定を生成する
    pub fn new() -> Self {
        Self::default()
    }

    /// サーバーの最大ウィンドウビットを設定する
    pub fn server_max_window_bits(mut self, bits: u8) -> Self {
        self.server_max_window_bits = Some(bits.clamp(8, 15));
        self
    }

    /// クライアントの最大ウィンドウビットを設定する
    pub fn client_max_window_bits(mut self, bits: u8) -> Self {
        self.client_max_window_bits = Some(bits.clamp(8, 15));
        self
    }

    /// サーバーがコンテキストを引き継がないように設定する
    pub fn server_no_context_takeover(mut self) -> Self {
        self.server_no_context_takeover = true;
        self
    }

    /// クライアントがコンテキストを引き継がないように設定する
    pub fn client_no_context_takeover(mut self) -> Self {
        self.client_no_context_takeover = true;
        self
    }

    /// Sec-WebSocket-Extensions ヘッダー値としてエンコードする
    ///
    /// 設定に応じて no_context_takeover パラメータを含める
    pub fn to_extension(&self) -> Extension {
        let mut ext = Extension::new("permessage-deflate");

        if self.server_no_context_takeover {
            ext = ext.param("server_no_context_takeover", None);
        }

        if self.client_no_context_takeover {
            ext = ext.param("client_no_context_takeover", None);
        }

        if let Some(bits) = self.server_max_window_bits {
            ext = ext.param("server_max_window_bits", Some(&bits.to_string()));
        }

        if let Some(bits) = self.client_max_window_bits {
            ext = ext.param("client_max_window_bits", Some(&bits.to_string()));
        }

        ext
    }

    /// Extension からパースする
    ///
    /// RFC 7692 に従い、8-15 範囲外の window_bits 値は拒否する
    ///
    /// # Deprecated
    /// この関数は検証が不十分なため、`from_extension_validated` を使用してください。
    #[deprecated(
        since = "0.3.0",
        note = "use from_extension_for_client_response or from_extension_for_server_request instead"
    )]
    pub fn from_extension(ext: &Extension) -> Option<Self> {
        if ext.name != "permessage-deflate" {
            return None;
        }

        let mut config = Self::default();

        for param in &ext.params {
            match param.name.as_str() {
                "server_no_context_takeover" => {
                    config.server_no_context_takeover = true;
                }
                "client_no_context_takeover" => {
                    config.client_no_context_takeover = true;
                }
                "server_max_window_bits" => {
                    if let Some(value) = &param.value {
                        if let Ok(bits) = value.parse::<u8>() {
                            // RFC 7692: 8-15 の範囲外は拒否
                            if !(8..=15).contains(&bits) {
                                return None;
                            }
                            config.server_max_window_bits = Some(bits);
                        } else {
                            return None;
                        }
                    }
                }
                "client_max_window_bits" => {
                    if let Some(value) = &param.value {
                        if let Ok(bits) = value.parse::<u8>() {
                            // RFC 7692: 8-15 の範囲外は拒否
                            if !(8..=15).contains(&bits) {
                                return None;
                            }
                            config.client_max_window_bits = Some(bits);
                        } else {
                            return None;
                        }
                    } else {
                        // 値なしの場合はデフォルト (15) を使用
                        config.client_max_window_bits = Some(15);
                    }
                }
                _ => {}
            }
        }

        Some(config)
    }

    /// Extension からパースする（検証付き）
    ///
    /// RFC 7692 に従い、パラメータの妥当性を検証する。
    /// コンテキストに応じて異なる検証ルールを適用する。
    pub fn from_extension_validated(
        ext: &Extension,
        context: ExtensionParseContext,
    ) -> Result<Self, ExtensionParseError> {
        if ext.name != "permessage-deflate" {
            return Err(ExtensionParseError::NotDeflate);
        }

        let mut config = Self::default();
        let mut seen_params = HashSet::new();

        // 有効なパラメータ名
        const VALID_PARAMS: &[&str] = &[
            "server_no_context_takeover",
            "client_no_context_takeover",
            "server_max_window_bits",
            "client_max_window_bits",
        ];

        for param in &ext.params {
            // 未定義パラメータをチェック
            if !VALID_PARAMS.contains(&param.name.as_str()) {
                return Err(ExtensionParseError::UnknownParameter(param.name.clone()));
            }

            // 重複パラメータをチェック
            if !seen_params.insert(param.name.clone()) {
                return Err(ExtensionParseError::DuplicateParameter(param.name.clone()));
            }

            match param.name.as_str() {
                "server_no_context_takeover" => {
                    // RFC 7692: このパラメータは値を持ってはならない
                    if param.value.is_some() {
                        return Err(ExtensionParseError::UnexpectedValue(
                            "server_no_context_takeover".to_string(),
                        ));
                    }
                    config.server_no_context_takeover = true;
                }
                "client_no_context_takeover" => {
                    // RFC 7692: このパラメータは値を持ってはならない
                    if param.value.is_some() {
                        return Err(ExtensionParseError::UnexpectedValue(
                            "client_no_context_takeover".to_string(),
                        ));
                    }
                    config.client_no_context_takeover = true;
                }
                "server_max_window_bits" => {
                    match &param.value {
                        Some(value) => {
                            let bits = value.parse::<u8>().map_err(|_| {
                                ExtensionParseError::InvalidValue(format!(
                                    "server_max_window_bits: invalid value '{}'",
                                    value
                                ))
                            })?;
                            // RFC 7692: 8-15 の範囲外は拒否
                            if !(8..=15).contains(&bits) {
                                return Err(ExtensionParseError::InvalidValue(format!(
                                    "server_max_window_bits: {} is out of range (8-15)",
                                    bits
                                )));
                            }
                            config.server_max_window_bits = Some(bits);
                        }
                        None => {
                            // RFC 7692 Section 7.1.2.1: server_max_window_bits は
                            // オファーでもレスポンスでも値が必須
                            // (client_max_window_bits とは異なり、値なしは許容されない)
                            return Err(ExtensionParseError::MissingValue(
                                "server_max_window_bits".to_string(),
                            ));
                        }
                    }
                }
                "client_max_window_bits" => {
                    if let Some(value) = &param.value {
                        let bits = value.parse::<u8>().map_err(|_| {
                            ExtensionParseError::InvalidValue(format!(
                                "client_max_window_bits: invalid value '{}'",
                                value
                            ))
                        })?;
                        // RFC 7692: 8-15 の範囲外は拒否
                        if !(8..=15).contains(&bits) {
                            return Err(ExtensionParseError::InvalidValue(format!(
                                "client_max_window_bits: {} is out of range (8-15)",
                                bits
                            )));
                        }
                        config.client_max_window_bits = Some(bits);
                    } else {
                        // RFC 7692 Section 7.1.2.2: クライアントレスポンスでは値が必須
                        if context == ExtensionParseContext::ClientResponse {
                            return Err(ExtensionParseError::MissingValue(
                                "client_max_window_bits".to_string(),
                            ));
                        }
                        // サーバーリクエスト (クライアントのオファー) では
                        // 値なしは「サーバーに値を選択させる」意味
                        config.client_max_window_bits = Some(15);
                    }
                }
                _ => unreachable!(),
            }
        }

        Ok(config)
    }

    /// クライアントがサーバーレスポンスをパースする
    ///
    /// サーバーからのレスポンスをパースする際に使用する。
    /// より厳格な検証を行い、RFC 7692 に準拠していないレスポンスを拒否する。
    pub fn from_extension_for_client_response(
        ext: &Extension,
    ) -> Result<Self, ExtensionParseError> {
        Self::from_extension_validated(ext, ExtensionParseContext::ClientResponse)
    }

    /// サーバーがクライアントリクエストをパースする
    ///
    /// クライアントからのリクエストをパースする際に使用する。
    /// クライアントリクエストでは一部のパラメータで値なしが許容される。
    pub fn from_extension_for_server_request(ext: &Extension) -> Result<Self, ExtensionParseError> {
        Self::from_extension_validated(ext, ExtensionParseContext::ServerRequest)
    }

    /// クライアント要求とサーバー設定をマージして交渉結果を生成
    ///
    /// RFC 7692 に従い、両者の制約を満たす設定を返す。
    /// no_context_takeover はどちらかが要求すれば有効になる。
    pub fn negotiate(client_request: &Self, server_config: &Self) -> Self {
        Self {
            // 現在の実装では window_bits=15 固定 (flate2 の制約) のため、
            // server_max_window_bits は含めない (デフォルト 15 を使用)
            // RFC 7692 Section 7.1.2.1: クライアントが server_max_window_bits を
            // offer しても、サーバーはパラメータを含めないことで拒否できる
            server_max_window_bits: None,
            // client_max_window_bits: クライアントが offer した場合のみ含める
            // RFC 7692: クライアントが offer していなければサーバーは含めてはならない
            client_max_window_bits: match (
                client_request.client_max_window_bits,
                server_config.client_max_window_bits,
            ) {
                (Some(client), Some(server)) => Some(client.min(server)),
                (Some(client), None) => Some(client),
                (None, _) => None, // クライアントが offer していなければ含めない
            },
            // RFC 7692: どちらかが要求すれば no_context_takeover が有効
            server_no_context_takeover: client_request.server_no_context_takeover
                || server_config.server_no_context_takeover,
            client_no_context_takeover: client_request.client_no_context_takeover
                || server_config.client_no_context_takeover,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_parse() {
        let extensions = Extension::parse("permessage-deflate; client_max_window_bits");
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].name, "permessage-deflate");
        assert_eq!(extensions[0].params.len(), 1);
        assert_eq!(extensions[0].params[0].name, "client_max_window_bits");
        assert!(extensions[0].params[0].value.is_none());
    }

    #[test]
    fn test_extension_parse_with_value() {
        let extensions = Extension::parse("permessage-deflate; server_max_window_bits=10");
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].name, "permessage-deflate");
        assert_eq!(extensions[0].params.len(), 1);
        assert_eq!(extensions[0].params[0].name, "server_max_window_bits");
        assert_eq!(extensions[0].params[0].value, Some("10".to_string()));
    }

    #[test]
    fn test_extension_parse_multiple() {
        let extensions = Extension::parse("foo, bar; param=value");
        assert_eq!(extensions.len(), 2);
        assert_eq!(extensions[0].name, "foo");
        assert_eq!(extensions[1].name, "bar");
    }

    #[test]
    fn test_extension_encode() {
        let ext = Extension::new("permessage-deflate")
            .param("server_no_context_takeover", None)
            .param("client_max_window_bits", Some("15"));

        let encoded = ext.encode();
        assert!(encoded.contains("permessage-deflate"));
        assert!(encoded.contains("server_no_context_takeover"));
        assert!(encoded.contains("client_max_window_bits=15"));
    }

    #[test]
    fn test_permessage_deflate_config() {
        let config = PerMessageDeflateConfig::new()
            .server_no_context_takeover()
            .client_max_window_bits(12);

        let ext = config.to_extension();
        assert_eq!(ext.name, "permessage-deflate");
        assert!(ext.get_param("server_no_context_takeover").is_some());
        assert_eq!(
            ext.get_param("client_max_window_bits")
                .and_then(|p| p.value.as_ref()),
            Some(&"12".to_string())
        );
    }

    #[test]
    #[allow(deprecated)]
    fn test_permessage_deflate_from_extension() {
        let ext = Extension::new("permessage-deflate")
            .param("server_no_context_takeover", None)
            .param("client_max_window_bits", Some("12"));

        let config = PerMessageDeflateConfig::from_extension(&ext).unwrap();
        assert!(config.server_no_context_takeover);
        assert!(!config.client_no_context_takeover);
        assert_eq!(config.client_max_window_bits, Some(12));
    }

    // === RFC 7692 パラメータ検証テスト ===

    #[test]
    fn test_reject_unknown_parameter() {
        let ext = Extension::new("permessage-deflate").param("unknown_param", None);

        let result = PerMessageDeflateConfig::from_extension_for_client_response(&ext);
        assert!(matches!(
            result,
            Err(ExtensionParseError::UnknownParameter(_))
        ));
    }

    #[test]
    fn test_reject_duplicate_parameter() {
        let ext = Extension::new("permessage-deflate")
            .param("server_no_context_takeover", None)
            .param("server_no_context_takeover", None);

        let result = PerMessageDeflateConfig::from_extension_for_client_response(&ext);
        assert!(matches!(
            result,
            Err(ExtensionParseError::DuplicateParameter(_))
        ));
    }

    #[test]
    fn test_reject_no_context_takeover_with_value() {
        let ext =
            Extension::new("permessage-deflate").param("server_no_context_takeover", Some("true"));

        let result = PerMessageDeflateConfig::from_extension_for_client_response(&ext);
        assert!(matches!(
            result,
            Err(ExtensionParseError::UnexpectedValue(_))
        ));
    }

    #[test]
    fn test_client_response_requires_server_max_window_bits_value() {
        // ClientResponse コンテキストでは server_max_window_bits に値が必要
        let ext = Extension::new("permessage-deflate").param("server_max_window_bits", None);

        let result = PerMessageDeflateConfig::from_extension_for_client_response(&ext);
        assert!(matches!(result, Err(ExtensionParseError::MissingValue(_))));
    }

    #[test]
    fn test_server_request_requires_server_max_window_bits_value() {
        // RFC 7692 Section 7.1.2.1: server_max_window_bits はオファーでも値が必須
        let ext = Extension::new("permessage-deflate").param("server_max_window_bits", None);

        let result = PerMessageDeflateConfig::from_extension_for_server_request(&ext);
        assert!(matches!(result, Err(ExtensionParseError::MissingValue(_))));
    }

    #[test]
    fn test_client_response_requires_client_max_window_bits_value() {
        // RFC 7692 Section 7.1.2.2: ClientResponse では client_max_window_bits に値が必要
        let ext = Extension::new("permessage-deflate").param("client_max_window_bits", None);

        let result = PerMessageDeflateConfig::from_extension_for_client_response(&ext);
        assert!(matches!(result, Err(ExtensionParseError::MissingValue(_))));
    }

    #[test]
    fn test_server_request_allows_client_max_window_bits_without_value() {
        // ServerRequest (クライアントのオファー) では client_max_window_bits の値なしは許容
        let ext = Extension::new("permessage-deflate").param("client_max_window_bits", None);

        let result = PerMessageDeflateConfig::from_extension_for_server_request(&ext);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().client_max_window_bits, Some(15)); // デフォルト値
    }

    #[test]
    fn test_negotiate_does_not_include_cmwb_when_not_offered() {
        // RFC 7692: クライアントが client_max_window_bits を offer していない場合、
        // サーバーはレスポンスに含めてはならない
        let client = PerMessageDeflateConfig::new().server_max_window_bits(12);
        let server = PerMessageDeflateConfig::new().client_max_window_bits(10);
        let result = PerMessageDeflateConfig::negotiate(&client, &server);
        assert!(result.client_max_window_bits.is_none());
    }

    #[test]
    fn test_negotiate_includes_cmwb_when_offered() {
        // クライアントが client_max_window_bits を offer した場合は含める
        let client = PerMessageDeflateConfig::new().client_max_window_bits(12);
        let server = PerMessageDeflateConfig::new().client_max_window_bits(10);
        let result = PerMessageDeflateConfig::negotiate(&client, &server);
        assert_eq!(result.client_max_window_bits, Some(10)); // サーバー設定の方が小さい
    }

    #[test]
    fn test_negotiate_cmwb_only_from_client() {
        // クライアントのみが offer した場合
        let client = PerMessageDeflateConfig::new().client_max_window_bits(12);
        let server = PerMessageDeflateConfig::new();
        let result = PerMessageDeflateConfig::negotiate(&client, &server);
        assert_eq!(result.client_max_window_bits, Some(12));
    }

    #[test]
    fn test_negotiate_forces_server_max_window_bits_to_none() {
        // flate2 の制約により server_max_window_bits は常に None (デフォルト 15)
        let client = PerMessageDeflateConfig::new().server_max_window_bits(10);
        let server = PerMessageDeflateConfig::new().server_max_window_bits(12);
        let result = PerMessageDeflateConfig::negotiate(&client, &server);
        assert!(result.server_max_window_bits.is_none());
    }

    #[test]
    fn test_reject_invalid_window_bits_value() {
        // 7 は範囲外
        let ext = Extension::new("permessage-deflate").param("server_max_window_bits", Some("7"));

        let result = PerMessageDeflateConfig::from_extension_for_client_response(&ext);
        assert!(matches!(result, Err(ExtensionParseError::InvalidValue(_))));

        // 16 も範囲外
        let ext = Extension::new("permessage-deflate").param("server_max_window_bits", Some("16"));

        let result = PerMessageDeflateConfig::from_extension_for_client_response(&ext);
        assert!(matches!(result, Err(ExtensionParseError::InvalidValue(_))));
    }

    #[test]
    fn test_accept_valid_window_bits_range() {
        // 8-15 の範囲は全て OK
        for bits in 8..=15 {
            let ext = Extension::new("permessage-deflate")
                .param("server_max_window_bits", Some(&bits.to_string()));

            let result = PerMessageDeflateConfig::from_extension_for_client_response(&ext);
            assert!(result.is_ok());
            assert_eq!(result.unwrap().server_max_window_bits, Some(bits));
        }
    }

    #[test]
    fn test_not_deflate_extension() {
        let ext = Extension::new("other-extension");

        let result = PerMessageDeflateConfig::from_extension_for_client_response(&ext);
        assert!(matches!(result, Err(ExtensionParseError::NotDeflate)));
    }

    // === RFC 6455 Section 9.1 quoted-string テスト ===

    #[test]
    fn test_parse_quoted_string_valid_token() {
        // RFC 6455 Section 9.1: quoted-string は token と等価
        let extensions = Extension::parse(r#"ext; param="simplevalue""#);
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].params[0].name, "param");
        assert_eq!(
            extensions[0].params[0].value,
            Some("simplevalue".to_string())
        );
    }

    #[test]
    fn test_parse_quoted_string_numeric() {
        // quoted-string で数値を囲んだ場合も正しくパースされる
        let extensions = Extension::parse(r#"permessage-deflate; server_max_window_bits="10""#);
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].name, "permessage-deflate");
        assert_eq!(extensions[0].params[0].value, Some("10".to_string()));
    }

    #[test]
    fn test_parse_quoted_string_rejects_non_token_space() {
        // RFC 6455 Section 9.1: 復号後の値が token に準拠しない場合は拡張を除外
        // スペースは tchar に含まれない
        let extensions = Extension::parse(r#"ext; param="quoted value""#);
        assert!(extensions.is_empty());
    }

    #[test]
    fn test_parse_quoted_string_rejects_non_token_escape() {
        // quoted-pair: \" → " (tchar に含まれない)
        // RFC 6455 Section 9.1: 復号後の値が token に準拠しないため拡張を除外
        let extensions = Extension::parse(r#"ext; param="value\"quote""#);
        assert!(extensions.is_empty());
    }

    #[test]
    fn test_parse_quoted_string_rejects_non_token_backslash() {
        // quoted-pair: \\ → \ (tchar に含まれない)
        // RFC 6455 Section 9.1: 復号後の値が token に準拠しないため拡張を除外
        let extensions = Extension::parse(r#"ext; param="path\\file""#);
        assert!(extensions.is_empty());
    }

    #[test]
    fn test_parse_unquoted_token_unchanged() {
        // token はそのまま
        let extensions = Extension::parse("ext; param=simplevalue");
        assert_eq!(extensions.len(), 1);
        assert_eq!(
            extensions[0].params[0].value,
            Some("simplevalue".to_string())
        );
    }

    #[test]
    fn test_parse_respects_quotes_with_comma() {
        // quoted-string 内のカンマは拡張の区切りとして扱わない
        // "a,b" は token に準拠しないため ext は除外、other は残る
        let extensions = Extension::parse(r#"ext; param="a,b", other"#);
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].name, "other");
    }

    #[test]
    fn test_parse_respects_quotes_with_semicolon() {
        // quoted-string 内のセミコロンはパラメータの区切りとして扱わない
        // "a;b" は token に準拠しないため拡張を除外
        let extensions = Extension::parse(r#"ext; param="a;b""#);
        assert!(extensions.is_empty());
    }

    #[test]
    fn test_parse_rejects_invalid_unquoted_value() {
        // token に準拠しない非 quoted 値は拡張を除外
        let extensions = Extension::parse("ext; param=invalid value");
        assert!(extensions.is_empty());
    }
}
