//! token.rs のプロパティベーステスト
//!
//! 目的:
//! - RFC 9110 Section 5.6.2 の token ABNF に対する `is_valid_token` の判定が
//!   生成された任意入力でも仕様どおりであることを検証する

use proptest::prelude::*;
use shiguredo_websocket::is_valid_token;

/// tchar 集合（RFC 9110 Section 5.6.2）
const TCHAR: &str = "!#$%&'*+-.^_`|~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

proptest! {
    /// tchar のみから構成される非空文字列は token として有効
    #[test]
    fn prop_nonempty_tchar_only_string_is_valid(
        indices in prop::collection::vec(0usize..TCHAR.len(), 1..=64)
    ) {
        let s: String = indices.into_iter().map(|i| TCHAR.as_bytes()[i] as char).collect();
        prop_assert!(is_valid_token(&s));
    }

    /// 非 tchar を 1 文字以上含む ASCII 文字列は token として無効
    #[test]
    fn prop_string_containing_non_tchar_is_invalid(
        prefix in "[!#$%&'*+\\-.\\^_`|~0-9A-Za-z]{0,32}",
        invalid_byte in proptest::sample::select(vec![
            // ASCII 制御文字 (0x00..=0x1F) と DEL (0x7F)
            0u8, 0x01u8, 0x07u8, b'\t', b'\n', 0x0bu8, 0x0cu8, b'\r', 0x1fu8, 0x7fu8,
            // RFC 9110 Section 5.6.2 で token から除外される delimiter
            b' ', b',', b';', b':', b'/', b'(', b')', b'<', b'>', b'@',
            b'?', b'=', b'{', b'}', b'\\', b'"', b'[', b']',
        ]),
        suffix in "[!#$%&'*+\\-.\\^_`|~0-9A-Za-z]{0,32}",
    ) {
        let mut s = prefix;
        s.push(invalid_byte as char);
        s.push_str(&suffix);
        prop_assert!(!is_valid_token(&s));
    }

    /// 非 ASCII Unicode 文字を含む文字列は token として無効
    /// （UTF-8 エンコード後のバイト列はすべて 0x80..=0xFF の範囲で tchar に該当しないため）
    #[test]
    fn prop_string_containing_non_ascii_unicode_is_invalid(
        prefix in "[!#$%&'*+\\-.\\^_`|~0-9A-Za-z]{0,32}",
        invalid_char in proptest::sample::select(vec![
            '\u{0080}', '\u{00FF}', '\u{0100}', '\u{4E00}', '\u{1F600}',
        ]),
        suffix in "[!#$%&'*+\\-.\\^_`|~0-9A-Za-z]{0,32}",
    ) {
        let mut s = prefix;
        s.push(invalid_char);
        s.push_str(&suffix);
        prop_assert!(!is_valid_token(&s));
    }
}
