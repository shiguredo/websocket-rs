//! time.rs のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - Timestamp の演算の整合性
//! - オーバーフローの安全性

use shiguredo_websocket::Timestamp;

const EXTREME_MILLIS: [u64; 4] = [0, 1, u64::MAX - 1, u64::MAX];

// ==== Timestamp 生成のテスト ====

/// from_millis と as_millis は逆変換
#[test]
fn prop_timestamp_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let millis = noprop::sample_u64(ctx);
        let ts = Timestamp::from_millis(millis);
        assert_eq!(ts.as_millis(), millis);
        Ok(())
    })?;
    Ok(())
}

// ==== Timestamp 比較のテスト ====

/// Timestamp の順序関係は u64 と一致
#[test]
fn prop_timestamp_ordering() -> noprop::TestResult {
    pbt::run(|ctx| {
        let a = noprop::sample_u64(ctx);
        let b = noprop::sample_u64(ctx);
        let ts_a = Timestamp::from_millis(a);
        let ts_b = Timestamp::from_millis(b);

        assert_eq!(ts_a < ts_b, a < b);
        assert_eq!(ts_a <= ts_b, a <= b);
        assert_eq!(ts_a > ts_b, a > b);
        assert_eq!(ts_a >= ts_b, a >= b);
        assert_eq!(ts_a == ts_b, a == b);
        Ok(())
    })?;
    Ok(())
}

/// PartialOrd と Ord が一致
#[test]
fn prop_timestamp_partial_ord_consistency() -> noprop::TestResult {
    pbt::run(|ctx| {
        let a = noprop::sample_u64(ctx);
        let b = noprop::sample_u64(ctx);
        let ts_a = Timestamp::from_millis(a);
        let ts_b = Timestamp::from_millis(b);

        assert_eq!(ts_a.partial_cmp(&ts_b), Some(ts_a.cmp(&ts_b)));
        Ok(())
    })?;
    Ok(())
}

// ==== saturating_sub のテスト ====

/// saturating_sub は負にならない
#[test]
fn prop_saturating_sub_non_negative() -> noprop::TestResult {
    pbt::run(|ctx| {
        let a = noprop::sample_u64(ctx);
        let b = noprop::sample_u64(ctx);
        let ts_a = Timestamp::from_millis(a);
        let ts_b = Timestamp::from_millis(b);

        let result = ts_a.saturating_sub(ts_b);
        // 結果は常に 0 以上（u64 なので自明だが、ロジック確認）
        assert!(result <= a);
        Ok(())
    })?;
    Ok(())
}

/// saturating_sub は正しい差分を返す
#[test]
fn prop_saturating_sub_correct() -> noprop::TestResult {
    pbt::run(|ctx| {
        let a = noprop::sample_u64(ctx);
        let b = noprop::sample_u64(ctx);
        let ts_a = Timestamp::from_millis(a);
        let ts_b = Timestamp::from_millis(b);

        let result = ts_a.saturating_sub(ts_b);

        if a >= b {
            assert_eq!(result, a - b);
        } else {
            assert_eq!(result, 0);
        }
        Ok(())
    })?;
    Ok(())
}

/// saturating_sub は a >= b のとき ts_b + result == ts_a
#[test]
fn prop_saturating_sub_inverse() -> noprop::TestResult {
    pbt::run(|ctx| {
        let a = noprop::sample_u64(ctx);
        let b = noprop::sample_u64(ctx);
        // a >= b の場合のみテスト
        if a >= b {
            let ts_a = Timestamp::from_millis(a);
            let ts_b = Timestamp::from_millis(b);

            let diff = ts_a.saturating_sub(ts_b);
            let restored = ts_b.add_millis(diff);

            assert_eq!(restored, ts_a);
        }
        Ok(())
    })?;
    Ok(())
}

// ==== add_millis のテスト ====

/// add_millis は saturating add
#[test]
fn prop_add_millis_saturating() -> noprop::TestResult {
    pbt::run(|ctx| {
        let base = noprop::sample_u64(ctx);
        let add = noprop::sample_u64(ctx);
        let ts = Timestamp::from_millis(base);
        let result = ts.add_millis(add);

        let expected = base.saturating_add(add);
        assert_eq!(result.as_millis(), expected);
        Ok(())
    })?;
    Ok(())
}

// ==== Add<u64> 実装のテスト ====

/// Timestamp + u64 は add_millis と同じ
#[test]
fn prop_add_trait_equals_add_millis() -> noprop::TestResult {
    pbt::run(|ctx| {
        let base = noprop::sample_u64(ctx);
        let add = noprop::sample_u64(ctx);
        let ts = Timestamp::from_millis(base);

        let via_trait = ts + add;
        let via_method = ts.add_millis(add);

        assert_eq!(via_trait, via_method);
        Ok(())
    })?;
    Ok(())
}

// ==== Sub 実装のテスト ====

/// Timestamp - Timestamp は saturating_sub と同じ
#[test]
fn prop_sub_trait_equals_saturating_sub() -> noprop::TestResult {
    pbt::run(|ctx| {
        let a = noprop::sample_u64(ctx);
        let b = noprop::sample_u64(ctx);
        let ts_a = Timestamp::from_millis(a);
        let ts_b = Timestamp::from_millis(b);

        let via_trait: u64 = ts_a - ts_b;
        let via_method = ts_a.saturating_sub(ts_b);

        assert_eq!(via_trait, via_method);
        Ok(())
    })?;
    Ok(())
}

// ==== オーバーフローのテスト ====

/// add_millis はオーバーフロー時に飽和する
#[test]
fn prop_add_millis_overflow_saturates() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0 を除く。足す値が 0 だと飽和が起きない。
        let add = noprop::sample_u64_in(ctx, 1..=u64::MAX);
        let ts = Timestamp::from_millis(u64::MAX);
        let result = ts.add_millis(add);
        assert_eq!(result.as_millis(), u64::MAX);
        Ok(())
    })?;
    Ok(())
}

/// + 演算子はオーバーフロー時に飽和する
#[test]
fn prop_add_trait_overflow_saturates() -> noprop::TestResult {
    pbt::run(|ctx| {
        let add = noprop::sample_u64_in(ctx, 1..=u64::MAX);
        let ts = Timestamp::from_millis(u64::MAX);
        let result = ts + add;
        assert_eq!(result.as_millis(), u64::MAX);
        Ok(())
    })?;
    Ok(())
}

/// saturating_sub はアンダーフロー時に 0 を返す
#[test]
fn prop_saturating_sub_underflow() -> noprop::TestResult {
    pbt::run(|ctx| {
        let small = noprop::sample_u64_in(ctx, 0..1000);
        let large = noprop::sample_u64_in(ctx, 1001..10000);
        let ts_small = Timestamp::from_millis(small);
        let ts_large = Timestamp::from_millis(large);
        let result = ts_small.saturating_sub(ts_large);
        assert_eq!(result, 0);
        Ok(())
    })?;
    Ok(())
}

/// - 演算子はアンダーフロー時に 0 を返す
#[test]
fn prop_sub_trait_underflow() -> noprop::TestResult {
    pbt::run(|ctx| {
        let small = noprop::sample_u64_in(ctx, 0..1000);
        let large = noprop::sample_u64_in(ctx, 1001..10000);
        let ts_small = Timestamp::from_millis(small);
        let ts_large = Timestamp::from_millis(large);
        let result: u64 = ts_small - ts_large;
        assert_eq!(result, 0);
        Ok(())
    })?;
    Ok(())
}

// ==== 境界値のテスト ====

/// 極端な値でもパニックしない
#[test]
fn prop_extreme_values() -> noprop::TestResult {
    pbt::run(|ctx| {
        let base = noprop::sample_choice(ctx, &EXTREME_MILLIS);
        let add = noprop::sample_choice(ctx, &EXTREME_MILLIS);
        let ts = Timestamp::from_millis(base);

        // これらの操作はパニックしない
        let _ = ts.add_millis(add);
        let _ = ts + add;
        let _ = ts.saturating_sub(Timestamp::from_millis(add));
        let _ = ts - Timestamp::from_millis(add);
        let _ = ts.as_millis();
        Ok(())
    })?;
    Ok(())
}

// ==== Hash の整合性テスト ====

/// 同じ値は同じハッシュを持つ
#[test]
fn prop_hash_consistency() -> noprop::TestResult {
    pbt::run(|ctx| {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let millis = noprop::sample_u64(ctx);
        let ts1 = Timestamp::from_millis(millis);
        let ts2 = Timestamp::from_millis(millis);

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        ts1.hash(&mut hasher1);
        ts2.hash(&mut hasher2);

        assert_eq!(hasher1.finish(), hasher2.finish());
        Ok(())
    })?;
    Ok(())
}

// ==== Debug 出力のテスト ====

/// Debug 出力がパニックしない
#[test]
fn prop_debug_no_panic() -> noprop::TestResult {
    pbt::run(|ctx| {
        let millis = noprop::sample_u64(ctx);
        let ts = Timestamp::from_millis(millis);
        let _ = format!("{ts:?}");
        Ok(())
    })?;
    Ok(())
}
