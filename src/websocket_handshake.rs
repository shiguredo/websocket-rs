//! WebSocket ハンドシェイクの共有ヘルパ
//!
//! リクエスト側 / レスポンス側で共通利用する次の処理を集約する:
//! - Sec-WebSocket-Accept の計算 (`calculate_accept`, `calculate_accept_from_key`)
//! - Sec-WebSocket-Key の検証 (`validate_key`)
//!
//! Sec-WebSocket-Extensions の ABNF 検証は `crate::websocket_extension::Extension::parse_strict`
//! に集約済み (issue 0050)。リクエスト処理は `crate::websocket_handshake_request`、
//! レスポンス処理は `crate::websocket_handshake_response` に分離されている。

use base64ct::{Base64, Encoding};

use crate::error::Error;

/// WebSocket ハンドシェイクで使用する固定 GUID (RFC 6455 Section 4.2.2 step 5 項目 4)
const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Sec-WebSocket-Accept の値を計算する
pub fn calculate_accept(nonce: &[u8; 16]) -> String {
    let key = Base64::encode_string(nonce.as_slice());
    calculate_accept_from_key(&key)
}

pub fn calculate_accept_from_key(key: &str) -> String {
    let combined = format!("{}{}", key, WEBSOCKET_GUID);
    let hash = sha1_digest(combined.as_bytes());
    Base64::encode_string(hash.as_ref())
}

// RFC 6455 Section 4 の Sec-WebSocket-Accept 計算用 SHA-1 ダイジェスト
// SHA1_FOR_LEGACY_USE_ONLY という命名だが、RFC 6455 でアルゴリズムが固定されているハンドシェイク用途であり
// 他に選択肢はないためそのまま使う
fn sha1_digest(data: &[u8]) -> [u8; 20] {
    let hash = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA1_FOR_LEGACY_USE_ONLY, data);
    let mut out = [0u8; 20];
    out.copy_from_slice(hash.as_ref());
    out
}

pub(crate) fn validate_key(key: &str) -> Result<(), Error> {
    let decoded = Base64::decode_vec(key)
        .map_err(|_| Error::handshake_rejected("invalid Sec-WebSocket-Key"))?;
    if decoded.len() != 16 {
        return Err(Error::handshake_rejected("invalid Sec-WebSocket-Key"));
    }
    Ok(())
}
