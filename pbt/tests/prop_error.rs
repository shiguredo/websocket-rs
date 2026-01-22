//! error.rs のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - Error の生成と表示の整合性
//! - ErrorKind の網羅性

use proptest::prelude::*;
use shiguredo_websocket::{Error, ErrorKind};

/// ErrorKind の全バリアント
const ERROR_KINDS: [ErrorKind; 8] = [
    ErrorKind::InvalidInput,
    ErrorKind::InvalidData,
    ErrorKind::InvalidState,
    ErrorKind::InsufficientBuffer,
    ErrorKind::Unsupported,
    ErrorKind::ProtocolViolation,
    ErrorKind::HandshakeRejected,
    ErrorKind::InvalidUtf8,
];

proptest! {
    // ==== ErrorKind のテスト ====

    /// すべての ErrorKind バリアントがある
    #[test]
    fn prop_error_kind_variants(idx in 0usize..8) {
        let kind = ERROR_KINDS[idx];
        let _ = kind;
    }

    /// ErrorKind は Clone 可能
    #[test]
    fn prop_error_kind_clone(idx in 0usize..8) {
        let kind = ERROR_KINDS[idx];
        let cloned = kind.clone();
        prop_assert_eq!(kind, cloned);
    }

    /// ErrorKind は Copy 可能
    #[test]
    fn prop_error_kind_copy(idx in 0usize..8) {
        let kind = ERROR_KINDS[idx];
        let copied = kind;
        prop_assert_eq!(kind, copied);
    }

    /// ErrorKind は Debug 表示可能
    #[test]
    fn prop_error_kind_debug(idx in 0usize..8) {
        let kind = ERROR_KINDS[idx];
        let debug_str = format!("{:?}", kind);
        prop_assert!(!debug_str.is_empty());
    }

    /// ErrorKind は Hash 可能
    #[test]
    fn prop_error_kind_hash(idx in 0usize..8) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let kind = ERROR_KINDS[idx];
        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        kind.hash(&mut hasher1);
        kind.hash(&mut hasher2);

        prop_assert_eq!(hasher1.finish(), hasher2.finish());
    }

    // ==== Error 生成のテスト ====

    /// Error::new は kind を正しく設定する
    #[test]
    fn prop_error_new(idx in 0usize..8) {
        let kind = ERROR_KINDS[idx];
        let error = Error::new(kind);
        prop_assert_eq!(error.kind, kind);
        prop_assert!(error.reason.is_empty());
        prop_assert!(error.close_code.is_none());
    }

    /// Error::with_reason は kind と reason を正しく設定する
    #[test]
    fn prop_error_with_reason(
        idx in 0usize..8,
        reason in "[a-zA-Z0-9 ]{0,100}"
    ) {
        let kind = ERROR_KINDS[idx];
        let error = Error::with_reason(kind, &reason);
        prop_assert_eq!(error.kind, kind);
        prop_assert_eq!(error.reason, reason);
        prop_assert!(error.close_code.is_none());
    }

    /// Error::with_close_code は close_code を設定する
    #[test]
    fn prop_error_with_close_code(
        idx in 0usize..3,
        code in 1000u16..=4999
    ) {
        let kinds = [ErrorKind::InvalidInput, ErrorKind::InvalidData, ErrorKind::ProtocolViolation];
        let kind = kinds[idx];
        let error = Error::new(kind).with_close_code(code);
        prop_assert_eq!(error.close_code, Some(code));
    }

    // ==== Error 表示のテスト ====

    /// Error の Display はパニックしない
    #[test]
    fn prop_error_display_no_panic(
        idx in 0usize..8,
        reason in "[a-zA-Z0-9 ]{0,50}"
    ) {
        let kind = ERROR_KINDS[idx];
        let error = Error::with_reason(kind, &reason);
        let display = format!("{}", error);
        prop_assert!(!display.is_empty());
    }

    /// Error の Debug はパニックしない
    #[test]
    fn prop_error_debug_no_panic(
        idx in 0usize..8,
        reason in "[a-zA-Z0-9 ]{0,50}"
    ) {
        let kind = ERROR_KINDS[idx];
        let error = Error::with_reason(kind, &reason);
        let debug = format!("{:?}", error);
        prop_assert!(!debug.is_empty());
    }

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
    fn prop_error_display_contains_kind(idx in 0usize..8) {
        let kind = ERROR_KINDS[idx];
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

    /// Display はファイル名を含む
    #[test]
    fn prop_error_display_contains_location(_dummy in 0u8..1) {
        let error = Error::new(ErrorKind::InvalidState);
        let display = format!("{}", error);
        prop_assert!(display.contains(".rs"));
    }

    /// Display は close_code を含む
    #[test]
    fn prop_error_display_close_code(code in 1000u16..=4999) {
        let error = Error::new(ErrorKind::ProtocolViolation).with_close_code(code);
        let display = format!("{}", error);
        let code_str = format!("{}", code);
        prop_assert!(display.contains(&code_str));
    }

    // ==== location のテスト ====

    /// location はこのファイルを指す
    #[test]
    fn prop_error_location_is_caller(_dummy in 0u8..1) {
        let error = Error::new(ErrorKind::InvalidInput);
        prop_assert!(error.location.file().contains("prop_error.rs"));
    }

    // ==== std::error::Error 実装のテスト ====

    /// Error は std::error::Error を実装している
    #[test]
    fn prop_error_is_std_error(idx in 0usize..8) {
        fn check_error<T: std::error::Error>(_: &T) {}
        let kind = ERROR_KINDS[idx];
        let error = Error::new(kind);
        check_error(&error);
    }

    // ==== ErrorKind 網羅性テスト ====

    /// すべての ErrorKind は区別可能
    #[test]
    fn prop_all_error_kinds_distinguishable(_dummy in 0u8..1) {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        for kind in &ERROR_KINDS {
            prop_assert!(set.insert(*kind), "Duplicate ErrorKind: {:?}", kind);
        }
        prop_assert_eq!(set.len(), ERROR_KINDS.len());
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

    /// reason は空文字列を受け入れる
    #[test]
    fn prop_error_with_reason_accepts_empty(_dummy in 0u8..1) {
        let error = Error::with_reason(ErrorKind::InvalidData, "");
        prop_assert!(error.reason.is_empty());
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
