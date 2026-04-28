//! 共通ユーティリティ

use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use base64ct::{Base64, Encoding};
use shiguredo_websocket::{RandomSource, Timestamp};

/// 暗号学的に安全な乱数ソース
pub struct SecureRandom;

impl RandomSource for SecureRandom {
    fn masking_key(&mut self) -> [u8; 4] {
        let mut key = [0u8; 4];
        aws_lc_rs::rand::fill(&mut key).expect("failed to generate masking key");
        key
    }

    fn nonce(&mut self) -> [u8; 16] {
        let mut nonce = [0u8; 16];
        aws_lc_rs::rand::fill(&mut nonce).expect("failed to generate nonce");
        nonce
    }
}

/// 現在のタイムスタンプを取得
pub fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    Timestamp::from_millis(millis)
}

/// `Authorization: Basic` ヘッダ用の値を生成
pub fn basic_auth_value(user_pass: &str) -> String {
    format!("Basic {}", Base64::encode_string(user_pass.as_bytes()))
}

pub type AnyError = Box<dyn Error + Send + Sync>;

/// IPv6 アドレスを `[` `]` で囲む
pub fn format_bracketed_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{}]", host)
    } else {
        host.to_string()
    }
}

/// Host ヘッダ用の文字列を生成 (デフォルトポートのときはポート省略)
pub fn format_host_header(host: &str, port: u16, tls: bool) -> String {
    let host = format_bracketed_host(host);
    let default = default_port(tls);
    if port == default {
        host
    } else {
        format!("{}:{}", host, port)
    }
}

pub fn default_port(tls: bool) -> u16 {
    if tls { 443 } else { 80 }
}

/// 解析済みの WebSocket URL
#[derive(Debug, Clone)]
pub struct WsUrl {
    pub host: String,
    pub port: u16,
    pub path: String,
    pub tls: bool,
}

/// `ws://` `wss://` URL をパース。スキーム省略時は `ws://` を補完する (wscat 互換)
pub fn parse_ws_url(input: &str) -> Result<WsUrl, AnyError> {
    let normalized = if has_scheme(input) {
        input.to_string()
    } else {
        format!("ws://{}", input)
    };

    let uri = shiguredo_http11::uri::Uri::parse(&normalized)
        .map_err(|e| format!("invalid URL: {}", e))?;
    let scheme = uri.scheme().ok_or("URL has no scheme")?;
    let tls = match scheme.to_ascii_lowercase().as_str() {
        "wss" | "https" => true,
        "ws" | "http" => false,
        other => return Err(format!("unsupported scheme: {}", other).into()),
    };
    if let Some(authority) = uri.authority()
        && authority.contains('@')
    {
        return Err("URL userinfo is not supported".into());
    }
    let host = uri.host().ok_or("URL has no host")?;
    let host = normalize_host(host)?;
    let port = uri.port().unwrap_or_else(|| default_port(tls));
    let path = uri.origin_form();
    Ok(WsUrl {
        host,
        port,
        path,
        tls,
    })
}

/// プロキシ URL を解析する。ユーザー情報があれば抽出する
#[derive(Debug, Clone)]
pub struct ProxyUrl {
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub auth: Option<String>,
}

pub fn parse_proxy_url(input: &str) -> Result<ProxyUrl, AnyError> {
    let normalized = if has_scheme(input) {
        input.to_string()
    } else {
        format!("http://{}", input)
    };
    let uri = shiguredo_http11::uri::Uri::parse(&normalized)
        .map_err(|e| format!("invalid proxy URL: {}", e))?;
    let scheme = uri.scheme().ok_or("proxy URL has no scheme")?;
    let tls = match scheme.to_ascii_lowercase().as_str() {
        "https" => true,
        "http" => false,
        other => return Err(format!("unsupported proxy scheme: {}", other).into()),
    };
    // userinfo の抽出は Uri::authority から手動で行う
    let auth = uri.authority().and_then(|authority| {
        let (userinfo, _) = authority.split_once('@')?;
        Some(basic_auth_value(userinfo))
    });
    let host = uri.host().ok_or("proxy URL has no host")?;
    let host = normalize_host(host)?;
    let port = uri.port().unwrap_or_else(|| default_port(tls));
    Ok(ProxyUrl {
        host,
        port,
        tls,
        auth,
    })
}

fn has_scheme(input: &str) -> bool {
    // RFC 3986: scheme は ALPHA で始まり ALPHA / DIGIT / "+" / "-" / "." が続く
    let mut chars = input.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    for c in chars.by_ref() {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return false;
        }
    }
    false
}

fn normalize_host(host: &str) -> Result<String, AnyError> {
    let host = host.trim();
    if host.is_empty() {
        return Err("host is empty".into());
    }
    if let Some(stripped) = host.strip_prefix('[') {
        let stripped = stripped.strip_suffix(']').ok_or("invalid bracketed host")?;
        if stripped.is_empty() {
            return Err("host is empty".into());
        }
        return Ok(stripped.to_string());
    }
    Ok(host.to_string())
}
