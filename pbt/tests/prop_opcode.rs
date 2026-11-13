//! websocket_opcode.rs のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - Opcode の変換の整合性
//! - RFC 6455 準拠の検証

use shiguredo_websocket::Opcode;

/// 有効な Opcode 値
const VALID_OPCODES: [u8; 6] = [0x0, 0x1, 0x2, 0x8, 0x9, 0xA];

/// 無効な Opcode 値（RFC 6455 で予約または未使用）
const INVALID_OPCODES: [u8; 10] = [0x3, 0x4, 0x5, 0x6, 0x7, 0xB, 0xC, 0xD, 0xE, 0xF];

/// コントロールフレームの Opcode
const CONTROL_OPCODES: [u8; 3] = [0x8, 0x9, 0xA];

/// データフレームの Opcode
const DATA_OPCODES: [u8; 3] = [0x0, 0x1, 0x2];

fn expected_opcode(opcode: u8) -> Option<Opcode> {
    match opcode {
        0x0 => Some(Opcode::Continuation),
        0x1 => Some(Opcode::Text),
        0x2 => Some(Opcode::Binary),
        0x8 => Some(Opcode::Close),
        0x9 => Some(Opcode::Ping),
        0xA => Some(Opcode::Pong),
        _ => None,
    }
}

// ==== from_u8 のテスト ====

/// 有効な Opcode は正しい値を返す
#[test]
fn prop_from_u8_valid_opcodes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &VALID_OPCODES);
        assert_eq!(Opcode::from_u8(opcode), expected_opcode(opcode));
        Ok(())
    })?;
    Ok(())
}

/// 有効な Opcode は Some を返す
#[test]
fn prop_from_u8_valid() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &VALID_OPCODES);
        assert!(Opcode::from_u8(opcode).is_some());
        Ok(())
    })?;
    Ok(())
}

/// 無効な Opcode は None を返す
#[test]
fn prop_from_u8_invalid() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &INVALID_OPCODES);
        assert!(Opcode::from_u8(opcode).is_none());
        Ok(())
    })?;
    Ok(())
}

/// 16 以上の値は無効
#[test]
fn prop_from_u8_out_of_range() -> noprop::TestResult {
    pbt::run(|ctx| {
        let value = noprop::sample_u8_in(ctx, 16..=u8::MAX);
        assert!(Opcode::from_u8(value).is_none());
        Ok(())
    })?;
    Ok(())
}

/// from_u8 と as_u8 はラウンドトリップ
#[test]
fn prop_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &VALID_OPCODES);
        let op = Opcode::from_u8(opcode).unwrap();
        assert_eq!(op.as_u8(), opcode);
        Ok(())
    })?;
    Ok(())
}

// ==== as_u8 のテスト ====

/// as_u8 は正しい値を返す
#[test]
fn prop_as_u8_all_variants() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &VALID_OPCODES);
        let op = Opcode::from_u8(opcode).unwrap();
        let expected = match op {
            Opcode::Continuation => 0x0,
            Opcode::Text => 0x1,
            Opcode::Binary => 0x2,
            Opcode::Close => 0x8,
            Opcode::Ping => 0x9,
            Opcode::Pong => 0xA,
        };
        assert_eq!(op.as_u8(), expected);
        Ok(())
    })?;
    Ok(())
}

// ==== is_control のテスト ====

/// コントロールフレームの Opcode は is_control が true
#[test]
fn prop_is_control_true() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &CONTROL_OPCODES);
        let op = Opcode::from_u8(opcode).unwrap();
        assert!(op.is_control());
        assert!(!op.is_data());
        Ok(())
    })?;
    Ok(())
}

/// データフレームの Opcode は is_control が false
#[test]
fn prop_is_control_false() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &DATA_OPCODES);
        let op = Opcode::from_u8(opcode).unwrap();
        assert!(!op.is_control());
        assert!(op.is_data());
        Ok(())
    })?;
    Ok(())
}

/// is_control と is_data は排他的
#[test]
fn prop_control_data_exclusive() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &VALID_OPCODES);
        let op = Opcode::from_u8(opcode).unwrap();
        assert_ne!(op.is_control(), op.is_data());
        Ok(())
    })?;
    Ok(())
}

// ==== is_data のテスト ====

/// データフレームの Opcode は is_data が true
#[test]
fn prop_is_data_true() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &DATA_OPCODES);
        let op = Opcode::from_u8(opcode).unwrap();
        assert!(op.is_data());
        Ok(())
    })?;
    Ok(())
}

/// コントロールフレームの Opcode は is_data が false
#[test]
fn prop_is_data_false() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &CONTROL_OPCODES);
        let op = Opcode::from_u8(opcode).unwrap();
        assert!(!op.is_data());
        Ok(())
    })?;
    Ok(())
}

// ==== RFC 6455 準拠テスト ====

/// RFC 6455 Section 5.2: コントロールフレームのオペコードは 0x8-0xF
#[test]
fn prop_rfc6455_control_opcodes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &CONTROL_OPCODES);
        let op = Opcode::from_u8(opcode).unwrap();
        assert!(op.is_control(), "Opcode 0x{:X} should be control", opcode);
        Ok(())
    })?;
    Ok(())
}

/// RFC 6455 Section 5.2: データフレームのオペコードは 0x0-0x7
#[test]
fn prop_rfc6455_data_opcodes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &DATA_OPCODES);
        let op = Opcode::from_u8(opcode).unwrap();
        assert!(op.is_data(), "Opcode 0x{:X} should be data", opcode);
        Ok(())
    })?;
    Ok(())
}

/// RFC 6455 Section 5.2: 予約オペコード (データフレーム用) は実装されていない
#[test]
fn prop_rfc6455_reserved_data_opcodes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_u8_in(ctx, 0x3..=0x7);
        assert!(
            Opcode::from_u8(opcode).is_none(),
            "Reserved data opcode 0x{:X} should not be valid",
            opcode
        );
        Ok(())
    })?;
    Ok(())
}

/// RFC 6455 Section 5.2: 予約オペコード (コントロールフレーム用) は実装されていない
#[test]
fn prop_rfc6455_reserved_control_opcodes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_u8_in(ctx, 0xB..=0xF);
        assert!(
            Opcode::from_u8(opcode).is_none(),
            "Reserved control opcode 0x{:X} should not be valid",
            opcode
        );
        Ok(())
    })?;
    Ok(())
}

// ==== Display のテスト ====

/// Display は正しい文字列を返す
#[test]
fn prop_display() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &VALID_OPCODES);
        let op = Opcode::from_u8(opcode).unwrap();
        let expected = match op {
            Opcode::Continuation => "Continuation",
            Opcode::Text => "Text",
            Opcode::Binary => "Binary",
            Opcode::Close => "Close",
            Opcode::Ping => "Ping",
            Opcode::Pong => "Pong",
        };
        assert_eq!(format!("{op}"), expected);
        Ok(())
    })?;
    Ok(())
}

// ==== Hash のテスト ====

/// 同じ値は同じハッシュを持つ
#[test]
fn prop_hash_consistency() -> noprop::TestResult {
    pbt::run(|ctx| {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let opcode = noprop::sample_choice(ctx, &VALID_OPCODES);
        let op1 = Opcode::from_u8(opcode).unwrap();
        let op2 = Opcode::from_u8(opcode).unwrap();

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        op1.hash(&mut hasher1);
        op2.hash(&mut hasher2);

        assert_eq!(hasher1.finish(), hasher2.finish());
        Ok(())
    })?;
    Ok(())
}

// ==== 網羅性テスト ====

/// すべての有効な Opcode は is_control または is_data のどちらか
#[test]
fn prop_all_opcodes_categorized() -> noprop::TestResult {
    pbt::run(|ctx| {
        let opcode = noprop::sample_choice(ctx, &VALID_OPCODES);
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
        Ok(())
    })?;
    Ok(())
}
