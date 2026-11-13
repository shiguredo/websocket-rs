//! error.rs のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - Error の生成と表示の整合性
//! - ErrorKind の網羅性

use shiguredo_websocket::{Error, ErrorKind};

/// ErrorKind の全バリアント
const ERROR_KINDS: [ErrorKind; 10] = [
    ErrorKind::InvalidInput,
    ErrorKind::InvalidData,
    ErrorKind::InvalidState,
    ErrorKind::InsufficientBuffer,
    ErrorKind::Unsupported,
    ErrorKind::ProtocolViolation,
    ErrorKind::HandshakeRejected,
    ErrorKind::VersionNotSupported,
    ErrorKind::InvalidUtf8,
    ErrorKind::HttpResponse,
];

const CLOSE_CODE_KINDS: [ErrorKind; 3] = [
    ErrorKind::InvalidInput,
    ErrorKind::InvalidData,
    ErrorKind::ProtocolViolation,
];

const ALNUM_SPACE: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ";
const ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";

// ==== Error 生成のテスト ====

/// Error::new は kind を正しく設定する
#[test]
fn prop_error_new() -> noprop::TestResult {
    pbt::run(|ctx| {
        let kind = noprop::sample_choice(ctx, &ERROR_KINDS);
        let error = Error::new(kind);
        assert_eq!(error.kind, kind);
        assert!(error.reason.is_empty());
        assert!(error.close_code.is_none());
        Ok(())
    })?;
    Ok(())
}

/// Error::with_reason は kind と reason を正しく設定する
#[test]
fn prop_error_with_reason() -> noprop::TestResult {
    pbt::run(|ctx| {
        let kind = noprop::sample_choice(ctx, &ERROR_KINDS);
        let reason = pbt::sample_alphabet(ctx, ALNUM_SPACE, 0, 100);
        let error = Error::with_reason(kind, &reason);
        assert_eq!(error.kind, kind);
        assert_eq!(error.reason, reason);
        assert!(error.close_code.is_none());
        Ok(())
    })?;
    Ok(())
}

/// Error::with_close_code は close_code を設定する
#[test]
fn prop_error_with_close_code() -> noprop::TestResult {
    pbt::run(|ctx| {
        let kind = noprop::sample_choice(ctx, &CLOSE_CODE_KINDS);
        let code = noprop::sample_u16_in(ctx, 1000..=4999);
        let error = Error::new(kind).with_close_code(code);
        assert_eq!(error.close_code, Some(code));
        Ok(())
    })?;
    Ok(())
}

// ==== Error 表示のテスト ====

/// close_code 付きの Error も正しく表示される
#[test]
fn prop_error_display_with_close_code() -> noprop::TestResult {
    pbt::run(|ctx| {
        let code = noprop::sample_u16_in(ctx, 1000..=4999);
        let error = Error::new(ErrorKind::ProtocolViolation).with_close_code(code);
        let display = format!("{error}");
        let code_str = format!("{code}");
        assert!(display.contains(&code_str));
        Ok(())
    })?;
    Ok(())
}

// ==== Display に含まれる情報のテスト ====

/// Display は ErrorKind を含む
#[test]
fn prop_error_display_contains_kind() -> noprop::TestResult {
    pbt::run(|ctx| {
        let kind = noprop::sample_choice(ctx, &ERROR_KINDS);
        let error = Error::new(kind);
        let display = format!("{error}");
        let kind_str = format!("{kind:?}");
        assert!(display.contains(&kind_str));
        Ok(())
    })?;
    Ok(())
}

/// Display は reason を含む
#[test]
fn prop_error_display_contains_reason() -> noprop::TestResult {
    pbt::run(|ctx| {
        let reason = pbt::sample_alphabet(ctx, ALNUM, 5, 20);
        let error = Error::with_reason(ErrorKind::InvalidData, &reason);
        let display = format!("{error}");
        assert!(display.contains(&reason));
        Ok(())
    })?;
    Ok(())
}

// ==== std::error::Error 実装のテスト ====

/// Error は std::error::Error を実装している
#[test]
fn prop_error_is_std_error() -> noprop::TestResult {
    pbt::run(|ctx| {
        fn check_error<T: std::error::Error>(_: &T) {}
        let kind = noprop::sample_choice(ctx, &ERROR_KINDS);
        let error = Error::new(kind);
        check_error(&error);
        Ok(())
    })?;
    Ok(())
}

// ==== reason が様々な型を受け入れるテスト ====

/// reason は String を受け入れる
#[test]
fn prop_error_with_reason_accepts_string() -> noprop::TestResult {
    pbt::run(|ctx| {
        let reason = pbt::sample_alphabet(ctx, LETTERS, 5, 20);
        let owned = reason.clone();
        let error = Error::with_reason(ErrorKind::InvalidData, owned);
        assert_eq!(error.reason, reason);
        Ok(())
    })?;
    Ok(())
}

/// reason は &str を受け入れる
#[test]
fn prop_error_with_reason_accepts_str() -> noprop::TestResult {
    pbt::run(|ctx| {
        let reason = pbt::sample_alphabet(ctx, LETTERS, 5, 20);
        let error = Error::with_reason(ErrorKind::InvalidData, &reason);
        assert_eq!(error.reason, reason);
        Ok(())
    })?;
    Ok(())
}

// ==== 複数の close_code 設定のテスト ====

/// close_code は複数回設定できる（最後の値が残る）
#[test]
fn prop_error_close_code_can_be_set_multiple_times() -> noprop::TestResult {
    pbt::run(|ctx| {
        let code1 = noprop::sample_u16_in(ctx, 1000..=2999);
        let code2 = noprop::sample_u16_in(ctx, 3000..=4999);
        let error = Error::new(ErrorKind::ProtocolViolation)
            .with_close_code(code1)
            .with_close_code(code2);
        assert_eq!(error.close_code, Some(code2));
        Ok(())
    })?;
    Ok(())
}

// ==== 境界値テスト ====

/// 非常に長い reason でもパニックしない
#[test]
fn prop_error_long_reason() -> noprop::TestResult {
    pbt::run(|ctx| {
        let reason = pbt::sample_alphabet(ctx, LOWER, 1000, 2000);
        let error = Error::with_reason(ErrorKind::InvalidData, &reason);
        let _ = format!("{error}");
        Ok(())
    })?;
    Ok(())
}
