//! websocket_opcode.rs のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - Opcode の変換の整合性
//! - RFC 6455 準拠の検証

use proptest::prelude::*;
use shiguredo_websocket::Opcode;

/// 有効な Opcode 値
const VALID_OPCODES: [u8; 6] = [0x0, 0x1, 0x2, 0x8, 0x9, 0xA];

/// 無効な Opcode 値（RFC 6455 で予約または未使用）
const INVALID_OPCODES: [u8; 10] = [0x3, 0x4, 0x5, 0x6, 0x7, 0xB, 0xC, 0xD, 0xE, 0xF];

// ==== from_u8 のテスト ====

#[test]
fn test_from_u8_valid_opcodes() {
    assert_eq!(Opcode::from_u8(0x0), Some(Opcode::Continuation));
    assert_eq!(Opcode::from_u8(0x1), Some(Opcode::Text));
    assert_eq!(Opcode::from_u8(0x2), Some(Opcode::Binary));
    assert_eq!(Opcode::from_u8(0x8), Some(Opcode::Close));
    assert_eq!(Opcode::from_u8(0x9), Some(Opcode::Ping));
    assert_eq!(Opcode::from_u8(0xA), Some(Opcode::Pong));
}

proptest! {
    /// 有効な Opcode は Some を返す
    #[test]
    fn test_from_u8_valid(
        opcode in prop::sample::select(VALID_OPCODES.to_vec())
    ) {
        let result = Opcode::from_u8(opcode);
        prop_assert!(result.is_some());
    }

    /// 無効な Opcode は None を返す
    #[test]
    fn test_from_u8_invalid(
        opcode in prop::sample::select(INVALID_OPCODES.to_vec())
    ) {
        let result = Opcode::from_u8(opcode);
        prop_assert!(result.is_none());
    }

    /// 16 以上の値は無効
    #[test]
    fn test_from_u8_out_of_range(value in 16u8..=u8::MAX) {
        let result = Opcode::from_u8(value);
        prop_assert!(result.is_none());
    }

    /// from_u8 と as_u8 はラウンドトリップ
    #[test]
    fn test_roundtrip(
        opcode in prop::sample::select(VALID_OPCODES.to_vec())
    ) {
        let op = Opcode::from_u8(opcode).unwrap();
        prop_assert_eq!(op.as_u8(), opcode);
    }
}

// ==== as_u8 のテスト ====

#[test]
fn test_as_u8_all_variants() {
    assert_eq!(Opcode::Continuation.as_u8(), 0x0);
    assert_eq!(Opcode::Text.as_u8(), 0x1);
    assert_eq!(Opcode::Binary.as_u8(), 0x2);
    assert_eq!(Opcode::Close.as_u8(), 0x8);
    assert_eq!(Opcode::Ping.as_u8(), 0x9);
    assert_eq!(Opcode::Pong.as_u8(), 0xA);
}

// ==== is_control のテスト ====

#[test]
fn test_is_control() {
    // データフレームはコントロールではない
    assert!(!Opcode::Continuation.is_control());
    assert!(!Opcode::Text.is_control());
    assert!(!Opcode::Binary.is_control());

    // コントロールフレームはコントロール
    assert!(Opcode::Close.is_control());
    assert!(Opcode::Ping.is_control());
    assert!(Opcode::Pong.is_control());
}

proptest! {
    /// is_control と is_data は排他的
    #[test]
    fn test_control_data_exclusive(
        opcode in prop::sample::select(VALID_OPCODES.to_vec())
    ) {
        let op = Opcode::from_u8(opcode).unwrap();
        // コントロールとデータは排他的
        prop_assert_ne!(op.is_control(), op.is_data());
    }
}

// ==== is_data のテスト ====

#[test]
fn test_is_data() {
    // データフレームはデータ
    assert!(Opcode::Continuation.is_data());
    assert!(Opcode::Text.is_data());
    assert!(Opcode::Binary.is_data());

    // コントロールフレームはデータではない
    assert!(!Opcode::Close.is_data());
    assert!(!Opcode::Ping.is_data());
    assert!(!Opcode::Pong.is_data());
}

// ==== RFC 6455 準拠テスト ====

/// RFC 6455 Section 5.2: コントロールフレームのオペコードは 0x8-0xF
#[test]
fn test_rfc6455_control_opcodes() {
    // 0x8, 0x9, 0xA はコントロールフレームとして定義
    for opcode in [0x8, 0x9, 0xA] {
        if let Some(op) = Opcode::from_u8(opcode) {
            assert!(op.is_control(), "Opcode 0x{:X} should be control", opcode);
        }
    }
}

/// RFC 6455 Section 5.2: データフレームのオペコードは 0x0-0x7
#[test]
fn test_rfc6455_data_opcodes() {
    // 0x0, 0x1, 0x2 はデータフレームとして定義
    for opcode in [0x0, 0x1, 0x2] {
        if let Some(op) = Opcode::from_u8(opcode) {
            assert!(op.is_data(), "Opcode 0x{:X} should be data", opcode);
        }
    }
}

/// RFC 6455 Section 5.2: 予約オペコードは実装されていない
#[test]
fn test_rfc6455_reserved_opcodes() {
    // データフレーム用予約（0x3-0x7）
    for opcode in 0x3..=0x7 {
        assert!(
            Opcode::from_u8(opcode).is_none(),
            "Reserved data opcode 0x{:X} should not be valid",
            opcode
        );
    }

    // コントロールフレーム用予約（0xB-0xF）
    for opcode in 0xB..=0xF {
        assert!(
            Opcode::from_u8(opcode).is_none(),
            "Reserved control opcode 0x{:X} should not be valid",
            opcode
        );
    }
}

// ==== Display のテスト ====

#[test]
fn test_display() {
    assert_eq!(format!("{}", Opcode::Continuation), "Continuation");
    assert_eq!(format!("{}", Opcode::Text), "Text");
    assert_eq!(format!("{}", Opcode::Binary), "Binary");
    assert_eq!(format!("{}", Opcode::Close), "Close");
    assert_eq!(format!("{}", Opcode::Ping), "Ping");
    assert_eq!(format!("{}", Opcode::Pong), "Pong");
}

proptest! {
    /// Display はパニックしない
    #[test]
    fn test_display_no_panic(
        opcode in prop::sample::select(VALID_OPCODES.to_vec())
    ) {
        let op = Opcode::from_u8(opcode).unwrap();
        let _ = format!("{}", op);
    }
}

// ==== Debug のテスト ====

proptest! {
    /// Debug はパニックしない
    #[test]
    fn test_debug_no_panic(
        opcode in prop::sample::select(VALID_OPCODES.to_vec())
    ) {
        let op = Opcode::from_u8(opcode).unwrap();
        let _ = format!("{:?}", op);
    }
}

// ==== Clone, Copy, PartialEq, Eq, Hash のテスト ====

proptest! {
    /// Clone と Copy は同じ結果
    #[test]
    fn test_clone_copy(
        opcode in prop::sample::select(VALID_OPCODES.to_vec())
    ) {
        let op = Opcode::from_u8(opcode).unwrap();
        let cloned = op.clone();
        let copied = op;

        prop_assert_eq!(op, cloned);
        prop_assert_eq!(op, copied);
    }

    /// 同じ値は同じハッシュを持つ
    #[test]
    fn test_hash_consistency(
        opcode in prop::sample::select(VALID_OPCODES.to_vec())
    ) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let op1 = Opcode::from_u8(opcode).unwrap();
        let op2 = Opcode::from_u8(opcode).unwrap();

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        op1.hash(&mut hasher1);
        op2.hash(&mut hasher2);

        prop_assert_eq!(hasher1.finish(), hasher2.finish());
    }
}

// ==== 網羅性テスト ====

#[test]
fn test_all_opcodes_categorized() {
    // すべての有効な Opcode は is_control または is_data のどちらか
    for &opcode in &VALID_OPCODES {
        let op = Opcode::from_u8(opcode).unwrap();
        assert!(
            op.is_control() || op.is_data(),
            "Opcode {:?} is neither control nor data",
            op
        );
        assert!(
            !(op.is_control() && op.is_data()),
            "Opcode {:?} is both control and data",
            op
        );
    }
}
