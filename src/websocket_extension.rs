/// WebSocket 拡張パラメータ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionParam {
    pub name: String,
    pub value: Option<String>,
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
    pub fn parse(s: &str) -> Vec<Extension> {
        s.split(',')
            .filter_map(|ext| {
                let ext = ext.trim();
                if ext.is_empty() {
                    return None;
                }

                let mut parts = ext.split(';');
                let name = parts.next()?.trim().to_string();

                let params: Vec<ExtensionParam> = parts
                    .filter_map(|p| {
                        let p = p.trim();
                        if p.is_empty() {
                            return None;
                        }

                        if let Some((name, value)) = p.split_once('=') {
                            Some(ExtensionParam {
                                name: name.trim().to_string(),
                                value: Some(value.trim().to_string()),
                            })
                        } else {
                            Some(ExtensionParam {
                                name: p.to_string(),
                                value: None,
                            })
                        }
                    })
                    .collect();

                Some(Extension { name, params })
            })
            .collect()
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

    /// クライアント要求とサーバー設定をマージして交渉結果を生成
    ///
    /// RFC 7692 に従い、両者の制約を満たす設定を返す
    pub fn negotiate(client_request: &Self, server_config: &Self) -> Self {
        Self {
            // server_max_window_bits: クライアント要求があればそれを尊重（サーバー設定以下に制限）
            server_max_window_bits: match (
                client_request.server_max_window_bits,
                server_config.server_max_window_bits,
            ) {
                (Some(client), Some(server)) => Some(client.min(server)),
                (Some(client), None) => Some(client),
                (None, Some(server)) => Some(server),
                (None, None) => None,
            },
            // client_max_window_bits: サーバー設定があればそれを優先
            client_max_window_bits: match (
                client_request.client_max_window_bits,
                server_config.client_max_window_bits,
            ) {
                (Some(client), Some(server)) => Some(client.min(server)),
                (Some(client), None) => Some(client),
                (None, Some(server)) => Some(server),
                (None, None) => None,
            },
            // no_context_takeover: どちらかが要求すれば有効
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
    fn test_permessage_deflate_from_extension() {
        let ext = Extension::new("permessage-deflate")
            .param("server_no_context_takeover", None)
            .param("client_max_window_bits", Some("12"));

        let config = PerMessageDeflateConfig::from_extension(&ext).unwrap();
        assert!(config.server_no_context_takeover);
        assert!(!config.client_no_context_takeover);
        assert_eq!(config.client_max_window_bits, Some(12));
    }
}
