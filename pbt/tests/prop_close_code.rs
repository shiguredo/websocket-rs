use proptest::prelude::*;
use shiguredo_websocket::{CloseCode, truncate_reason};

proptest! {
    #[test]
    fn prop_close_code_from_u16(code in any::<u16>()) {
        let close_code = CloseCode::from(code);
        prop_assert_eq!(close_code.as_u16(), code);
    }

    #[test]
    fn prop_close_code_to_u16(code in any::<u16>()) {
        let close_code = CloseCode::new(code);
        let back: u16 = close_code.into();
        prop_assert_eq!(back, code);
    }

    #[test]
    fn prop_close_code_display(code in any::<u16>()) {
        let close_code = CloseCode::new(code);
        let display = format!("{}", close_code);
        prop_assert!(!display.is_empty());
    }

    #[test]
    fn prop_invalid_range_0_999(code in 0u16..1000) {
        let close_code = CloseCode::new(code);
        prop_assert!(!close_code.is_valid());
        prop_assert!(!close_code.is_sendable());
    }

    #[test]
    fn prop_valid_range_1000_1003(code in 1000u16..=1003) {
        let close_code = CloseCode::new(code);
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    /// 1004 は予約済みで、受信時は有効だが送信禁止
    #[test]
    fn prop_reserved_1004(_dummy in 0u8..1) {
        let close_code = CloseCode::new(1004);
        prop_assert!(close_code.is_valid());
        prop_assert!(!close_code.is_sendable());
    }

    /// 1005, 1006, 1015 は送信禁止であり、受信時も無効として扱う
    /// (RFC 6455 Section 7.4.1: MUST NOT be set as a status code)
    #[test]
    fn prop_unsendable_codes(code in prop::sample::select(vec![1005u16, 1006, 1015])) {
        let close_code = CloseCode::new(code);
        prop_assert!(!close_code.is_sendable());
        // 受信時も無効として扱う
        prop_assert!(!close_code.is_valid());
    }

    #[test]
    fn prop_valid_range_1007_1011(code in 1007u16..=1011) {
        let close_code = CloseCode::new(code);
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    /// 1012-1014 は IANA 登録済みで送信可能
    #[test]
    fn prop_iana_range_1012_1014(code in 1012u16..=1014) {
        let close_code = CloseCode::new(code);
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    /// 1016-1999 は予約済みだが、受信時は有効として扱い送信も許容
    #[test]
    fn prop_reserved_range_1016_1999(code in 1016u16..2000) {
        let close_code = CloseCode::new(code);
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    /// 2000-2999 は予約済み（RFC 6455 Section 7.4.2: RFC 6455 および将来の拡張用）のため送信しない
    #[test]
    fn prop_reserved_range_2000_2999(code in 2000u16..3000) {
        let close_code = CloseCode::new(code);
        // 受信時は有効として扱う
        prop_assert!(close_code.is_valid());
        // 送信は禁止
        prop_assert!(!close_code.is_sendable());
    }

    #[test]
    fn prop_library_range_3000_3999(code in 3000u16..4000) {
        let close_code = CloseCode::new(code);
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    #[test]
    fn prop_application_range_4000_4999(code in 4000u16..5000) {
        let close_code = CloseCode::new(code);
        prop_assert!(close_code.is_valid());
        prop_assert!(close_code.is_sendable());
    }

    /// 5000 以上は RFC で定義されていない範囲のため送信禁止
    #[test]
    fn prop_over_5000(code in 5000u16..=u16::MAX) {
        let close_code = CloseCode::new(code);
        prop_assert!(!close_code.is_valid());
        prop_assert!(!close_code.is_sendable());
    }

}

/// 定義済み CloseCode 定数の値
const DEFINED_CLOSE_CODES: [(u16, &str); 13] = [
    (1000, "Normal Closure"),
    (1001, "Going Away"),
    (1002, "Protocol Error"),
    (1003, "Unsupported Data"),
    (1004, "Reserved"),
    (1005, "No Status Received"),
    (1006, "Abnormal Closure"),
    (1007, "Invalid Payload"),
    (1008, "Policy Violation"),
    (1009, "Message Too Big"),
    (1010, "Mandatory Extension"),
    (1011, "Internal Error"),
    (1015, "TLS Handshake"),
];

proptest! {
    // ==== Display の説明文テスト ====

    /// 定義済み CloseCode は正しい説明文を含む
    #[test]
    fn prop_display_descriptions(
        idx in 0usize..13
    ) {
        let (code, expected_text) = DEFINED_CLOSE_CODES[idx];
        let close_code = CloseCode::new(code);
        let display = format!("{}", close_code);
        prop_assert!(display.contains(expected_text), "code {} should contain '{}'", code, expected_text);
    }

    /// Library/Framework 範囲 (3000-3999) の Display
    #[test]
    fn prop_display_library_range(code in 3000u16..4000) {
        let close_code = CloseCode::new(code);
        let display = format!("{}", close_code);
        prop_assert!(display.contains("Library/Framework"));
    }

    /// Application 範囲 (4000-4999) の Display
    #[test]
    fn prop_display_application_range(code in 4000u16..5000) {
        let close_code = CloseCode::new(code);
        let display = format!("{}", close_code);
        prop_assert!(display.contains("Application"));
    }

    /// Unknown 範囲 (0-999) の Display
    #[test]
    fn prop_display_unknown_range(code in 0u16..1000) {
        let close_code = CloseCode::new(code);
        let display = format!("{}", close_code);
        prop_assert!(display.contains("Unknown"));
    }
}

// ==== truncate_reason の PBT ====

proptest! {
    /// truncation 結果のバイト長が max_bytes 以下である
    #[test]
    fn prop_truncate_reason_length(
        reason in "\\PC{0,300}",
        max_bytes in 0usize..=200
    ) {
        let result = truncate_reason(&reason, max_bytes);
        prop_assert!(
            result.len() <= max_bytes,
            "結果 {} バイトが max_bytes {} を超えている",
            result.len(), max_bytes
        );
    }

    /// truncation 結果が元の文字列の prefix である
    #[test]
    fn prop_truncate_reason_is_prefix(
        reason in "\\PC{0,300}",
        max_bytes in 0usize..=200
    ) {
        let result = truncate_reason(&reason, max_bytes);
        prop_assert!(
            reason.starts_with(result),
            "結果 {:?} が元の文字列 {:?} の prefix ではない",
            result, reason
        );
    }

    /// reason.len() <= max_bytes の場合は reason がそのまま返る
    #[test]
    fn prop_truncate_reason_identity_when_short(
        reason in "\\PC{0,200}"
    ) {
        let max_bytes = reason.len() + 10;
        let result = truncate_reason(&reason, max_bytes);
        prop_assert_eq!(result, reason.as_str());
    }
}
