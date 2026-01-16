//! error.rs のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - Error の生成と表示の整合性
//! - ErrorKind の網羅性

use proptest::prelude::*;
use shiguredo_websocket::{Error, ErrorKind};

// ==== ErrorKind のテスト ====

#[test]
fn test_error_kind_variants() {
    // すべての ErrorKind バリアントがある
    let _ = ErrorKind::InvalidInput;
    let _ = ErrorKind::InvalidData;
    let _ = ErrorKind::InvalidState;
    let _ = ErrorKind::InsufficientBuffer;
    let _ = ErrorKind::Unsupported;
    let _ = ErrorKind::ProtocolViolation;
    let _ = ErrorKind::HandshakeRejected;
    let _ = ErrorKind::InvalidUtf8;
}

proptest! {
    /// ErrorKind は Clone 可能
    #[test]
    fn test_error_kind_clone(
        kind in prop::sample::select(vec![
            ErrorKind::InvalidInput,
            ErrorKind::InvalidData,
            ErrorKind::InvalidState,
            ErrorKind::InsufficientBuffer,
            ErrorKind::Unsupported,
            ErrorKind::ProtocolViolation,
            ErrorKind::HandshakeRejected,
            ErrorKind::InvalidUtf8,
        ])
    ) {
        let cloned = kind.clone();
        prop_assert_eq!(kind, cloned);
    }

    /// ErrorKind は Copy 可能
    #[test]
    fn test_error_kind_copy(
        kind in prop::sample::select(vec![
            ErrorKind::InvalidInput,
            ErrorKind::InvalidData,
            ErrorKind::InvalidState,
            ErrorKind::InsufficientBuffer,
            ErrorKind::Unsupported,
            ErrorKind::ProtocolViolation,
            ErrorKind::HandshakeRejected,
            ErrorKind::InvalidUtf8,
        ])
    ) {
        let copied = kind;
        prop_assert_eq!(kind, copied);
    }

    /// ErrorKind は Debug 表示可能
    #[test]
    fn test_error_kind_debug(
        kind in prop::sample::select(vec![
            ErrorKind::InvalidInput,
            ErrorKind::InvalidData,
            ErrorKind::InvalidState,
            ErrorKind::InsufficientBuffer,
            ErrorKind::Unsupported,
            ErrorKind::ProtocolViolation,
            ErrorKind::HandshakeRejected,
            ErrorKind::InvalidUtf8,
        ])
    ) {
        let debug_str = format!("{:?}", kind);
        prop_assert!(!debug_str.is_empty());
    }

    /// ErrorKind は Hash 可能
    #[test]
    fn test_error_kind_hash(
        kind in prop::sample::select(vec![
            ErrorKind::InvalidInput,
            ErrorKind::InvalidData,
            ErrorKind::InvalidState,
            ErrorKind::InsufficientBuffer,
            ErrorKind::Unsupported,
            ErrorKind::ProtocolViolation,
            ErrorKind::HandshakeRejected,
            ErrorKind::InvalidUtf8,
        ])
    ) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        kind.hash(&mut hasher1);
        kind.hash(&mut hasher2);

        prop_assert_eq!(hasher1.finish(), hasher2.finish());
    }
}

// ==== Error 生成のテスト ====

proptest! {
    /// Error::new は kind を正しく設定する
    #[test]
    fn test_error_new(
        kind in prop::sample::select(vec![
            ErrorKind::InvalidInput,
            ErrorKind::InvalidData,
            ErrorKind::InvalidState,
            ErrorKind::InsufficientBuffer,
            ErrorKind::Unsupported,
            ErrorKind::ProtocolViolation,
            ErrorKind::HandshakeRejected,
            ErrorKind::InvalidUtf8,
        ])
    ) {
        let error = Error::new(kind);
        prop_assert_eq!(error.kind, kind);
        prop_assert!(error.reason.is_empty());
        prop_assert!(error.close_code.is_none());
    }

    /// Error::with_reason は kind と reason を正しく設定する
    #[test]
    fn test_error_with_reason(
        kind in prop::sample::select(vec![
            ErrorKind::InvalidInput,
            ErrorKind::InvalidData,
            ErrorKind::InvalidState,
            ErrorKind::InsufficientBuffer,
            ErrorKind::Unsupported,
            ErrorKind::ProtocolViolation,
            ErrorKind::HandshakeRejected,
            ErrorKind::InvalidUtf8,
        ]),
        reason in "[a-zA-Z0-9 ]{0,100}"
    ) {
        let error = Error::with_reason(kind, &reason);
        prop_assert_eq!(error.kind, kind);
        prop_assert_eq!(error.reason, reason);
        prop_assert!(error.close_code.is_none());
    }

    /// Error::with_close_code は close_code を設定する
    #[test]
    fn test_error_with_close_code(
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
}

// ==== Error 表示のテスト ====

proptest! {
    /// Error の Display はパニックしない
    #[test]
    fn test_error_display_no_panic(
        kind in prop::sample::select(vec![
            ErrorKind::InvalidInput,
            ErrorKind::InvalidData,
            ErrorKind::InvalidState,
            ErrorKind::InsufficientBuffer,
            ErrorKind::Unsupported,
            ErrorKind::ProtocolViolation,
            ErrorKind::HandshakeRejected,
            ErrorKind::InvalidUtf8,
        ]),
        reason in "[a-zA-Z0-9 ]{0,50}"
    ) {
        let error = Error::with_reason(kind, &reason);
        let display = format!("{}", error);
        prop_assert!(!display.is_empty());
    }

    /// Error の Debug はパニックしない
    #[test]
    fn test_error_debug_no_panic(
        kind in prop::sample::select(vec![
            ErrorKind::InvalidInput,
            ErrorKind::InvalidData,
            ErrorKind::InvalidState,
            ErrorKind::InsufficientBuffer,
            ErrorKind::Unsupported,
            ErrorKind::ProtocolViolation,
            ErrorKind::HandshakeRejected,
            ErrorKind::InvalidUtf8,
        ]),
        reason in "[a-zA-Z0-9 ]{0,50}"
    ) {
        let error = Error::with_reason(kind, &reason);
        let debug = format!("{:?}", error);
        prop_assert!(!debug.is_empty());
    }

    /// close_code 付きの Error も正しく表示される
    #[test]
    fn test_error_display_with_close_code(
        kind in prop::sample::select(vec![
            ErrorKind::ProtocolViolation,
        ]),
        code in 1000u16..=4999
    ) {
        let error = Error::new(kind).with_close_code(code);
        let display = format!("{}", error);
        let code_str = format!("{}", code);
        prop_assert!(display.contains(&code_str));
    }
}

// ==== Display に含まれる情報のテスト ====

#[test]
fn test_error_display_contains_kind() {
    let error = Error::new(ErrorKind::InvalidInput);
    let display = format!("{}", error);
    assert!(display.contains("InvalidInput"));
}

#[test]
fn test_error_display_contains_reason() {
    let error = Error::with_reason(ErrorKind::InvalidData, "test reason");
    let display = format!("{}", error);
    assert!(display.contains("test reason"));
}

#[test]
fn test_error_display_contains_location() {
    let error = Error::new(ErrorKind::InvalidState);
    let display = format!("{}", error);
    // ファイル名が含まれる
    assert!(display.contains(".rs"));
}

#[test]
fn test_error_display_contains_close_code() {
    let error = Error::new(ErrorKind::ProtocolViolation).with_close_code(1002);
    let display = format!("{}", error);
    assert!(display.contains("1002"));
}

// ==== location のテスト ====

#[test]
fn test_error_location_is_caller() {
    let error = Error::new(ErrorKind::InvalidInput);
    // location はこのファイルを指す
    assert!(error.location.file().contains("prop_error.rs"));
}

// ==== std::error::Error 実装のテスト ====

#[test]
fn test_error_is_std_error() {
    fn check_error<T: std::error::Error>(_: &T) {}

    let error = Error::new(ErrorKind::InvalidInput);
    check_error(&error);
}

// ==== ErrorKind 網羅性テスト ====

#[test]
fn test_all_error_kinds_distinguishable() {
    use std::collections::HashSet;

    let kinds = [
        ErrorKind::InvalidInput,
        ErrorKind::InvalidData,
        ErrorKind::InvalidState,
        ErrorKind::InsufficientBuffer,
        ErrorKind::Unsupported,
        ErrorKind::ProtocolViolation,
        ErrorKind::HandshakeRejected,
        ErrorKind::InvalidUtf8,
    ];

    let mut set = HashSet::new();
    for kind in &kinds {
        assert!(set.insert(*kind), "Duplicate ErrorKind: {:?}", kind);
    }

    // すべて異なる
    assert_eq!(set.len(), kinds.len());
}

// ==== reason が Into<String> を受け入れるテスト ====

#[test]
fn test_error_with_reason_accepts_string() {
    let reason = String::from("owned string");
    let error = Error::with_reason(ErrorKind::InvalidData, reason);
    assert_eq!(error.reason, "owned string");
}

#[test]
fn test_error_with_reason_accepts_str() {
    let error = Error::with_reason(ErrorKind::InvalidData, "string slice");
    assert_eq!(error.reason, "string slice");
}

#[test]
fn test_error_with_reason_accepts_empty() {
    let error = Error::with_reason(ErrorKind::InvalidData, "");
    assert!(error.reason.is_empty());
}

// ==== 複数の close_code 設定のテスト ====

#[test]
fn test_error_close_code_can_be_set_multiple_times() {
    let error = Error::new(ErrorKind::ProtocolViolation)
        .with_close_code(1000)
        .with_close_code(1002);

    // 最後に設定した値が残る
    assert_eq!(error.close_code, Some(1002));
}

// ==== 境界値テスト ====

proptest! {
    /// 非常に長い reason でもパニックしない
    #[test]
    fn test_error_long_reason(
        reason in "[a-z]{1000,2000}"
    ) {
        let error = Error::with_reason(ErrorKind::InvalidData, &reason);
        let _ = format!("{}", error);
    }
}
