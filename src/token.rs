/// RFC 9110 Section 5.6.2 の token ABNF に準拠するかチェックする
///
/// token = 1*tchar
/// tchar = "!" / "#" / "$" / "%" / "&" / "'" / "*" / "+" / "-" / "." /
///         "^" / "_" / "`" / "|" / "~" / DIGIT / ALPHA
///
/// 参照: RFC 9110 (HTTP Semantics). 将来の RFC 更新により tchar の定義が変わる可能性がある。
///
/// 内部向け API。`lib.rs` で `#[doc(hidden)] pub use` 経由で PBT に公開する。
/// 通常の `pub use` で公開しないこと。
pub fn is_valid_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| {
            matches!(b,
                b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' |
                b'^' | b'_' | b'`' | b'|' | b'~' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z'
            )
        })
}
