use proptest::prelude::*;
use shiguredo_websocket::CloseCode;

proptest! {
    #[test]
    fn test_close_code_from_u16(code in any::<u16>()) {
        let close_code = CloseCode::from(code);
        prop_assert_eq!(close_code.as_u16(), code);
    }

    #[test]
    fn test_close_code_to_u16(code in any::<u16>()) {
        let close_code = CloseCode::new(code);
        let back: u16 = close_code.into();
        prop_assert_eq!(back, code);
    }

    #[test]
    fn test_close_code_display(code in any::<u16>()) {
        let close_code = CloseCode::new(code);
        let display = format!("{}", close_code);
        prop_assert!(!display.is_empty());
    }

    #[test]
    fn test_invalid_range_0_999(code in 0u16..1000) {
        let close_code = CloseCode::new(code);
        prop_assert!(!close_code.is_valid());
        prop_assert!(!close_code.is_sendable());
    }

    #[test]
    fn test_valid_range_1000_1003(code in 1000u16..=1003) {
        let close_code = CloseCode::new(code);
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    #[test]
    fn test_reserved_1004(_dummy in 0u8..1) {
        let close_code = CloseCode::new(1004);
        prop_assert!(!close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    #[test]
    fn test_unsendable_codes(code in prop::sample::select(vec![1005u16, 1006, 1015])) {
        let close_code = CloseCode::new(code);
        prop_assert!(!close_code.is_sendable());
    }

    #[test]
    fn test_valid_range_1007_1011(code in 1007u16..=1011) {
        let close_code = CloseCode::new(code);
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    #[test]
    fn test_unused_range_1012_2999(code in 1012u16..3000) {
        prop_assume!(code != 1015);
        let close_code = CloseCode::new(code);
        prop_assert!(!close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    #[test]
    fn test_library_range_3000_3999(code in 3000u16..4000) {
        let close_code = CloseCode::new(code);
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    #[test]
    fn test_application_range_4000_4999(code in 4000u16..5000) {
        let close_code = CloseCode::new(code);
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    #[test]
    fn test_over_5000(code in 5000u16..=u16::MAX) {
        let close_code = CloseCode::new(code);
        prop_assert!(!close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    #[test]
    fn test_constant_equivalence(_dummy in 0u8..1) {
        prop_assert_eq!(CloseCode::new(1000), CloseCode::NORMAL);
        prop_assert_eq!(CloseCode::new(1001), CloseCode::GOING_AWAY);
        prop_assert_eq!(CloseCode::new(1002), CloseCode::PROTOCOL_ERROR);
        prop_assert_eq!(CloseCode::new(1003), CloseCode::UNSUPPORTED_DATA);
        prop_assert_eq!(CloseCode::new(1007), CloseCode::INVALID_PAYLOAD);
        prop_assert_eq!(CloseCode::new(1008), CloseCode::POLICY_VIOLATION);
        prop_assert_eq!(CloseCode::new(1009), CloseCode::MESSAGE_TOO_BIG);
        prop_assert_eq!(CloseCode::new(1010), CloseCode::MANDATORY_EXTENSION);
        prop_assert_eq!(CloseCode::new(1011), CloseCode::INTERNAL_ERROR);
    }

    /// Clone と Copy は同じ結果
    #[test]
    fn test_clone_copy(code in any::<u16>()) {
        let close_code = CloseCode::new(code);
        let cloned = close_code.clone();
        let copied = close_code;

        prop_assert_eq!(close_code, cloned);
        prop_assert_eq!(close_code, copied);
    }

    /// 同じ値は同じハッシュを持つ
    #[test]
    fn test_hash_consistency(code in any::<u16>()) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let cc1 = CloseCode::new(code);
        let cc2 = CloseCode::new(code);

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        cc1.hash(&mut hasher1);
        cc2.hash(&mut hasher2);

        prop_assert_eq!(hasher1.finish(), hasher2.finish());
    }

    /// Debug はパニックしない
    #[test]
    fn test_debug_no_panic(code in any::<u16>()) {
        let close_code = CloseCode::new(code);
        let _ = format!("{:?}", close_code);
    }
}

// ==== Display の説明文テスト ====

#[test]
fn test_display_descriptions() {
    assert!(format!("{}", CloseCode::NORMAL).contains("Normal Closure"));
    assert!(format!("{}", CloseCode::GOING_AWAY).contains("Going Away"));
    assert!(format!("{}", CloseCode::PROTOCOL_ERROR).contains("Protocol Error"));
    assert!(format!("{}", CloseCode::UNSUPPORTED_DATA).contains("Unsupported Data"));
    assert!(format!("{}", CloseCode::RESERVED).contains("Reserved"));
    assert!(format!("{}", CloseCode::NO_STATUS_RECEIVED).contains("No Status Received"));
    assert!(format!("{}", CloseCode::ABNORMAL_CLOSURE).contains("Abnormal Closure"));
    assert!(format!("{}", CloseCode::INVALID_PAYLOAD).contains("Invalid Payload"));
    assert!(format!("{}", CloseCode::POLICY_VIOLATION).contains("Policy Violation"));
    assert!(format!("{}", CloseCode::MESSAGE_TOO_BIG).contains("Message Too Big"));
    assert!(format!("{}", CloseCode::MANDATORY_EXTENSION).contains("Mandatory Extension"));
    assert!(format!("{}", CloseCode::INTERNAL_ERROR).contains("Internal Error"));
    assert!(format!("{}", CloseCode::TLS_HANDSHAKE).contains("TLS Handshake"));
}

#[test]
fn test_display_library_range() {
    let code = CloseCode::new(3500);
    assert!(format!("{}", code).contains("Library/Framework"));
}

#[test]
fn test_display_application_range() {
    let code = CloseCode::new(4500);
    assert!(format!("{}", code).contains("Application"));
}

#[test]
fn test_display_unknown_range() {
    let code = CloseCode::new(999);
    assert!(format!("{}", code).contains("Unknown"));
}
