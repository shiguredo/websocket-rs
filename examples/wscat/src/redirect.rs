//! HTTP リダイレクトの追従 (3xx + Location)
//!
//! WebSocket ハンドシェイクで 101 以外を受信した場合、`shiguredo_websocket` は
//! `Error::http_response` でステータスコードとヘッダを返す。本モジュールはそこから
//! 3xx を判定し、Location ヘッダを次の URL として解決する。

use shiguredo_websocket::HttpResponseInfo;

use crate::common::{AnyError, WsUrl, parse_ws_url};

/// `info` が 3xx でない場合は `Ok(None)`、3xx の場合は次に接続すべき URL を返す
pub fn next_url(current: &WsUrl, info: &HttpResponseInfo) -> Result<Option<WsUrl>, AnyError> {
    if !(300..400).contains(&info.status_code) {
        return Ok(None);
    }
    let location = info
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("Location"))
        .map(|(_, v)| v.trim().to_string())
        .ok_or_else(|| {
            format!(
                "redirect {} {} returned without Location header",
                info.status_code, info.reason_phrase
            )
        })?;

    if location.is_empty() {
        return Err("redirect Location header is empty".into());
    }

    // 絶対 URL のみサポート (相対 URL は本実装ではサポート外)
    if has_absolute_scheme(&location) {
        let normalized = normalize_redirect_target(&location);
        let next = parse_ws_url(&normalized)?;
        return Ok(Some(next));
    }
    if location.starts_with('/') {
        let mut next = current.clone();
        next.path = location;
        return Ok(Some(next));
    }

    Err(format!("relative redirect not supported: {}", location).into())
}

fn has_absolute_scheme(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.starts_with("ws://")
        || lower.starts_with("wss://")
        || lower.starts_with("http://")
        || lower.starts_with("https://")
}

/// `https://` を `wss://`、`http://` を `ws://` に正規化する
fn normalize_redirect_target(location: &str) -> String {
    if let Some(rest) = location
        .strip_prefix("https://")
        .or_else(|| location.strip_prefix("HTTPS://"))
    {
        format!("wss://{}", rest)
    } else if let Some(rest) = location
        .strip_prefix("http://")
        .or_else(|| location.strip_prefix("HTTP://"))
    {
        format!("ws://{}", rest)
    } else {
        location.to_string()
    }
}
