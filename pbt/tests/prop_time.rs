//! time.rs のプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - Timestamp の演算の整合性
//! - オーバーフローの安全性

use proptest::prelude::*;
use shiguredo_websocket::Timestamp;

// ==== Timestamp 生成のテスト ====

proptest! {
    /// from_millis と as_millis は逆変換
    #[test]
    fn prop_timestamp_roundtrip(millis in any::<u64>()) {
        let ts = Timestamp::from_millis(millis);
        prop_assert_eq!(ts.as_millis(), millis);
    }
}

// ==== Timestamp 比較のテスト ====

proptest! {
    /// Timestamp の順序関係は u64 と一致
    #[test]
    fn prop_timestamp_ordering(a in any::<u64>(), b in any::<u64>()) {
        let ts_a = Timestamp::from_millis(a);
        let ts_b = Timestamp::from_millis(b);

        prop_assert_eq!(ts_a < ts_b, a < b);
        prop_assert_eq!(ts_a <= ts_b, a <= b);
        prop_assert_eq!(ts_a > ts_b, a > b);
        prop_assert_eq!(ts_a >= ts_b, a >= b);
        prop_assert_eq!(ts_a == ts_b, a == b);
    }

    /// PartialOrd と Ord が一致
    #[test]
    fn prop_timestamp_partial_ord_consistency(a in any::<u64>(), b in any::<u64>()) {
        let ts_a = Timestamp::from_millis(a);
        let ts_b = Timestamp::from_millis(b);

        prop_assert_eq!(ts_a.partial_cmp(&ts_b), Some(ts_a.cmp(&ts_b)));
    }
}

// ==== saturating_sub のテスト ====

proptest! {
    /// saturating_sub は負にならない
    #[test]
    fn prop_saturating_sub_non_negative(a in any::<u64>(), b in any::<u64>()) {
        let ts_a = Timestamp::from_millis(a);
        let ts_b = Timestamp::from_millis(b);

        let result = ts_a.saturating_sub(ts_b);
        // 結果は常に 0 以上（u64 なので自明だが、ロジック確認）
        prop_assert!(result <= a);
    }

    /// saturating_sub は正しい差分を返す
    #[test]
    fn prop_saturating_sub_correct(a in any::<u64>(), b in any::<u64>()) {
        let ts_a = Timestamp::from_millis(a);
        let ts_b = Timestamp::from_millis(b);

        let result = ts_a.saturating_sub(ts_b);

        if a >= b {
            prop_assert_eq!(result, a - b);
        } else {
            prop_assert_eq!(result, 0);
        }
    }

    /// saturating_sub は a >= b のとき ts_b + result == ts_a
    #[test]
    fn prop_saturating_sub_inverse(a in any::<u64>(), b in any::<u64>()) {
        // a >= b の場合のみテスト
        if a >= b {
            let ts_a = Timestamp::from_millis(a);
            let ts_b = Timestamp::from_millis(b);

            let diff = ts_a.saturating_sub(ts_b);
            let restored = ts_b.add_millis(diff);

            prop_assert_eq!(restored, ts_a);
        }
    }
}

// ==== add_millis のテスト ====

proptest! {
    /// add_millis は saturating add
    #[test]
    fn prop_add_millis_saturating(base in any::<u64>(), add in any::<u64>()) {
        let ts = Timestamp::from_millis(base);
        let result = ts.add_millis(add);

        let expected = base.saturating_add(add);
        prop_assert_eq!(result.as_millis(), expected);
    }
}

// ==== Add<u64> 実装のテスト ====

proptest! {
    /// Timestamp + u64 は add_millis と同じ
    #[test]
    fn prop_add_trait_equals_add_millis(base in any::<u64>(), add in any::<u64>()) {
        let ts = Timestamp::from_millis(base);

        let via_trait = ts + add;
        let via_method = ts.add_millis(add);

        prop_assert_eq!(via_trait, via_method);
    }
}

// ==== Sub 実装のテスト ====

proptest! {
    /// Timestamp - Timestamp は saturating_sub と同じ
    #[test]
    fn prop_sub_trait_equals_saturating_sub(a in any::<u64>(), b in any::<u64>()) {
        let ts_a = Timestamp::from_millis(a);
        let ts_b = Timestamp::from_millis(b);

        let via_trait: u64 = ts_a - ts_b;
        let via_method = ts_a.saturating_sub(ts_b);

        prop_assert_eq!(via_trait, via_method);
    }
}

proptest! {
    // ==== オーバーフローのテスト ====

    /// add_millis はオーバーフロー時に飽和する
    #[test]
    fn prop_add_millis_overflow_saturates(add in 1u64..=u64::MAX) {
        let ts = Timestamp::from_millis(u64::MAX);
        let result = ts.add_millis(add);
        prop_assert_eq!(result.as_millis(), u64::MAX);
    }

    /// + 演算子はオーバーフロー時に飽和する
    #[test]
    fn prop_add_trait_overflow_saturates(add in 1u64..=u64::MAX) {
        let ts = Timestamp::from_millis(u64::MAX);
        let result = ts + add;
        prop_assert_eq!(result.as_millis(), u64::MAX);
    }

    /// saturating_sub はアンダーフロー時に 0 を返す
    #[test]
    fn prop_saturating_sub_underflow(
        small in 0u64..1000,
        large in 1001u64..10000
    ) {
        let ts_small = Timestamp::from_millis(small);
        let ts_large = Timestamp::from_millis(large);
        let result = ts_small.saturating_sub(ts_large);
        prop_assert_eq!(result, 0);
    }

    /// - 演算子はアンダーフロー時に 0 を返す
    #[test]
    fn prop_sub_trait_underflow(
        small in 0u64..1000,
        large in 1001u64..10000
    ) {
        let ts_small = Timestamp::from_millis(small);
        let ts_large = Timestamp::from_millis(large);
        let result: u64 = ts_small - ts_large;
        prop_assert_eq!(result, 0);
    }
}

// ==== 境界値のテスト ====

proptest! {
    /// 極端な値でもパニックしない
    #[test]
    fn prop_extreme_values(
        base in prop::sample::select(vec![0, 1, u64::MAX - 1, u64::MAX]),
        add in prop::sample::select(vec![0, 1, u64::MAX - 1, u64::MAX])
    ) {
        let ts = Timestamp::from_millis(base);

        // これらの操作はパニックしない
        let _ = ts.add_millis(add);
        let _ = ts + add;
        let _ = ts.saturating_sub(Timestamp::from_millis(add));
        let _ = ts - Timestamp::from_millis(add);
        let _ = ts.as_millis();
    }
}

// ==== Hash の整合性テスト ====

proptest! {
    /// 同じ値は同じハッシュを持つ
    #[test]
    fn prop_hash_consistency(millis in any::<u64>()) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let ts1 = Timestamp::from_millis(millis);
        let ts2 = Timestamp::from_millis(millis);

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        ts1.hash(&mut hasher1);
        ts2.hash(&mut hasher2);

        prop_assert_eq!(hasher1.finish(), hasher2.finish());
    }
}

// ==== Debug 出力のテスト ====

proptest! {
    /// Debug 出力がパニックしない
    #[test]
    fn prop_debug_no_panic(millis in any::<u64>()) {
        let ts = Timestamp::from_millis(millis);
        let _ = format!("{:?}", ts);
    }
}
