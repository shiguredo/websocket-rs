//! token.rs のプロパティベーステスト
//!
//! 目的:
//! - RFC 9110 Section 5.6.2 の token ABNF に対する `is_valid_token` の判定が
//!   生成された任意入力でも仕様どおりであることを検証する

use shiguredo_websocket::is_valid_token;

/// tchar 集合（RFC 9110 Section 5.6.2）
const TCHAR: &[u8] =
    b"!#$%&'*+-.^_`|~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// token から除外する ASCII。制御文字、DEL、delimiter。
const NON_TCHAR: &[u8] = &[
    0x00, 0x01, 0x07, b'\t', b'\n', 0x0b, 0x0c, b'\r', 0x1f, 0x7f, b' ', b',', b';', b':', b'/',
    b'(', b')', b'<', b'>', b'@', b'?', b'=', b'{', b'}', b'\\', b'"', b'[', b']',
];

const NON_ASCII: &[char] = &['\u{0080}', '\u{00FF}', '\u{0100}', '\u{4E00}', '\u{1F600}'];

/// tchar のみから構成される非空文字列は token として有効
#[test]
fn prop_nonempty_tchar_only_string_is_valid() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 1..=64 文字。空は token にしない。1 文字と 64 文字を境界として厚く引く。
        let len = pbt::sample_len_inclusive(ctx, 1, 64);
        let mut text = String::with_capacity(len);
        for _ in 0..len {
            text.push(noprop::sample_choice(ctx, TCHAR) as char);
        }
        assert!(is_valid_token(&text));
        Ok(())
    })?;
    Ok(())
}

/// 非 tchar を 1 文字以上含む ASCII 文字列は token として無効
#[test]
fn prop_string_containing_non_tchar_is_invalid() -> noprop::TestResult {
    pbt::run(|ctx| {
        let prefix = pbt::sample_alphabet(ctx, TCHAR, 0, 32);
        let invalid_byte = noprop::sample_choice(ctx, NON_TCHAR);
        let suffix = pbt::sample_alphabet(ctx, TCHAR, 0, 32);
        let mut text = prefix;
        text.push(invalid_byte as char);
        text.push_str(&suffix);
        assert!(!is_valid_token(&text));
        Ok(())
    })?;
    Ok(())
}

/// 非 ASCII Unicode 文字を含む文字列は token として無効
/// （UTF-8 エンコード後のバイト列はすべて 0x80..=0xFF の範囲で tchar に該当しないため）
#[test]
fn prop_string_containing_non_ascii_unicode_is_invalid() -> noprop::TestResult {
    pbt::run(|ctx| {
        let prefix = pbt::sample_alphabet(ctx, TCHAR, 0, 32);
        let invalid_char = noprop::sample_choice(ctx, NON_ASCII);
        let suffix = pbt::sample_alphabet(ctx, TCHAR, 0, 32);
        let mut text = prefix;
        text.push(invalid_char);
        text.push_str(&suffix);
        assert!(!is_valid_token(&text));
        Ok(())
    })?;
    Ok(())
}
