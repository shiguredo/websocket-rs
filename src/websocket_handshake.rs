//! WebSocket ハンドシェイクの共有ヘルパ
//!
//! リクエスト側 / レスポンス側で共通利用する次の処理を集約する:
//! - Sec-WebSocket-Accept の計算 (`calculate_accept`, `calculate_accept_from_key`)
//! - Sec-WebSocket-Key の検証 (`validate_key`)
//! - Sec-WebSocket-Extensions の ABNF 検証 (`validate_extension_entry`)
//!
//! リクエスト処理は `crate::websocket_handshake_request`、レスポンス処理は
//! `crate::websocket_handshake_response` に分離されている。

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

/// RFC 6455 Section 9.1 の extension ABNF を検証する
///
/// extension       = extension-token *( ";" extension-param )
/// extension-param = token [ "=" ( token / ( DQUOTE *QDTEXT DQUOTE ) ) ]
pub(crate) fn validate_extension_entry(ext: &str) -> Result<(), Error> {
    // ';' による単純分割は quoted-string 内の ';' を誤分割する。
    // Sec-WebSocket-Extensions の ',' 分割と同様に stateful パーサーへの置き換えが必要（未対応）。
    let parts: Vec<&str> = ext.split(';').collect();
    let token = parts[0].trim();
    if !crate::token::is_valid_token(token) {
        return Err(Error::handshake_rejected(format!(
            "invalid Sec-WebSocket-Extensions token: {}",
            token
        )));
    }
    for part in parts.iter().skip(1) {
        let param = part.trim();
        if param.is_empty() {
            return Err(Error::handshake_rejected(format!(
                "trailing ';' in Sec-WebSocket-Extensions: '{}'",
                ext
            )));
        }
        match param.split_once('=') {
            Some((name, value)) => {
                let name = name.trim();
                let value = value.trim();
                if !crate::token::is_valid_token(name) {
                    return Err(Error::handshake_rejected(format!(
                        "invalid extension-param name: '{}'",
                        name
                    )));
                }
                if value.starts_with('"') {
                    // RFC 6455 Section 9.1 (ABNF は RFC 2616 由来、qdtext は RFC 9110 Section 5.6.4 準拠ただし obs-text 除外):
                    // quoted-string = DQUOTE *( qdtext / quoted-pair ) DQUOTE
                    // qdtext = HTAB / SP / %x21 / %x23-5B / %x5D-7E
                    // quoted-pair = "\" CHAR
                    // unescape 後の値は token ABNF に準拠する必要がある (MUST)
                    let bytes = value.as_bytes();
                    let mut i = 1; // 開き DQUOTE をスキップ
                    let mut unescaped = String::new();
                    let mut closed = false;
                    while i < bytes.len() {
                        let b = bytes[i];
                        if b == b'\\' {
                            i += 1;
                            if i >= bytes.len() {
                                return Err(Error::handshake_rejected(format!(
                                    "incomplete escape sequence in quoted-string: '{}'",
                                    param
                                )));
                            }
                            unescaped.push(bytes[i] as char);
                        } else if b == b'"' {
                            if i + 1 != bytes.len() {
                                return Err(Error::handshake_rejected(format!(
                                    "trailing characters after closing DQUOTE: '{}'",
                                    param
                                )));
                            }
                            closed = true;
                            break;
                        } else {
                            // QDTEXT = HTAB / SP / %x21 / %x23-5B / %x5D-7E
                            if !matches!(b, b'\t' | b' ' | 0x21 | 0x23..=0x5B | 0x5D..=0x7E) {
                                return Err(Error::handshake_rejected(format!(
                                    "invalid character in quoted-string: '{}'",
                                    param
                                )));
                            }
                            unescaped.push(b as char);
                        }
                        i += 1;
                    }
                    if !closed {
                        return Err(Error::handshake_rejected(format!(
                            "unclosed quoted-string in extension-param: '{}'",
                            param
                        )));
                    }
                    // RFC 6455 Section 9.1: unescape 後の値は token ABNF に準拠する必要がある
                    if !crate::token::is_valid_token(&unescaped) {
                        return Err(Error::handshake_rejected(format!(
                            "quoted-string value after unescaping is not a valid token: '{}'",
                            param
                        )));
                    }
                } else if !crate::token::is_valid_token(value) {
                    return Err(Error::handshake_rejected(format!(
                        "invalid extension-param value: '{}'",
                        param
                    )));
                }
            }
            None => {
                if !crate::token::is_valid_token(param) {
                    return Err(Error::handshake_rejected(format!(
                        "invalid extension-param name: '{}'",
                        param
                    )));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_key(key: &str) -> Result<(), Error> {
    let decoded = Base64::decode_vec(key)
        .map_err(|_| Error::handshake_rejected("invalid Sec-WebSocket-Key"))?;
    if decoded.len() != 16 {
        return Err(Error::handshake_rejected("invalid Sec-WebSocket-Key"));
    }
    Ok(())
}
