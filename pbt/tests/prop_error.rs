//! error.rs のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - Error の生成と表示の整合性
//! - ErrorKind の網羅性

use proptest::prelude::*;
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

proptest! {
    // ==== Error 生成のテスト ====

    /// Error::new は kind を正しく設定する
    #[test]
    fn prop_error_new(kind in prop::sample::select(ERROR_KINDS.to_vec())) {
        let error = Error::new(kind);
        prop_assert_eq!(error.kind, kind);
        prop_assert!(error.reason.is_empty());
        prop_assert!(error.close_code.is_none());
    }

    /// Error::with_reason は kind と reason を正しく設定する
    #[test]
    fn prop_error_with_reason(
        kind in prop::sample::select(ERROR_KINDS.to_vec()),
        reason in "[a-zA-Z0-9 ]{0,100}"
    ) {
        let error = Error::with_reason(kind, &reason);
        prop_assert_eq!(error.kind, kind);
        prop_assert_eq!(error.reason, reason);
        prop_assert!(error.close_code.is_none());
    }

    /// Error::with_close_code は close_code を設定する
    #[test]
    fn prop_error_with_close_code(
        kind in prop::sample::select(vec![
            ErrorKind::InvalidInput,
            ErrorKind::InvalidData,
            ErrorKind::ProtocolViolation,
        ]),
        code in 1000u16..=4999
    ) {
        let error = Error::new(kind).with_close_code(code);
        prop_assert_eq!(error.close_code, Some(code));
    }

    // ==== Error 表示のテスト ====

    /// close_code 付きの Error も正しく表示される
    #[test]
    fn prop_error_display_with_close_code(code in 1000u16..=4999) {
        let error = Error::new(ErrorKind::ProtocolViolation).with_close_code(code);
        let display = format!("{}", error);
        let code_str = format!("{}", code);
        prop_assert!(display.contains(&code_str));
    }

    // ==== Display に含まれる情報のテスト ====

    /// Display は ErrorKind を含む
    #[test]
    fn prop_error_display_contains_kind(kind in prop::sample::select(ERROR_KINDS.to_vec())) {
        let error = Error::new(kind);
        let display = format!("{}", error);
        let kind_str = format!("{:?}", kind);
        prop_assert!(display.contains(&kind_str));
    }

    /// Display は reason を含む
    #[test]
    fn prop_error_display_contains_reason(reason in "[a-zA-Z0-9]{5,20}") {
        let error = Error::with_reason(ErrorKind::InvalidData, &reason);
        let display = format!("{}", error);
        prop_assert!(display.contains(&reason));
    }

    // ==== std::error::Error 実装のテスト ====

    /// Error は std::error::Error を実装している
    #[test]
    fn prop_error_is_std_error(kind in prop::sample::select(ERROR_KINDS.to_vec())) {
        fn check_error<T: std::error::Error>(_: &T) {}
        let error = Error::new(kind);
        check_error(&error);
    }

    // ==== reason が様々な型を受け入れるテスト ====

    /// reason は String を受け入れる
    #[test]
    fn prop_error_with_reason_accepts_string(reason in "[a-zA-Z]{5,20}") {
        let owned = reason.clone();
        let error = Error::with_reason(ErrorKind::InvalidData, owned);
        prop_assert_eq!(error.reason, reason);
    }

    /// reason は &str を受け入れる
    #[test]
    fn prop_error_with_reason_accepts_str(reason in "[a-zA-Z]{5,20}") {
        let error = Error::with_reason(ErrorKind::InvalidData, &reason);
        prop_assert_eq!(error.reason, reason);
    }

    // ==== 複数の close_code 設定のテスト ====

    /// close_code は複数回設定できる（最後の値が残る）
    #[test]
    fn prop_error_close_code_can_be_set_multiple_times(
        code1 in 1000u16..=2999,
        code2 in 3000u16..=4999
    ) {
        let error = Error::new(ErrorKind::ProtocolViolation)
            .with_close_code(code1)
            .with_close_code(code2);
        prop_assert_eq!(error.close_code, Some(code2));
    }

    // ==== 境界値テスト ====

    /// 非常に長い reason でもパニックしない
    #[test]
    fn prop_error_long_reason(reason in "[a-z]{1000,2000}") {
        let error = Error::with_reason(ErrorKind::InvalidData, &reason);
        let _ = format!("{}", error);
    }
}
