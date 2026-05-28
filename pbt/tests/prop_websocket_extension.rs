//! websocket_extension.rs のプロパティベーステスト
//!
//! 目的:
//! - ExtensionParseError の Display 出力のプロパティ検証
//!
//! NotDeflate は引数を持たず PBT 化に意味が無いため、対応する単体テストは
//! `tests/test_websocket_extension.rs` 側に配置している。

use proptest::prelude::*;
use shiguredo_websocket::ExtensionParseError;

proptest! {
    /// UnknownParameter の Display はパラメータ名を含む固定書式
    #[test]
    fn prop_unknown_parameter_display(name in any::<String>()) {
        let err = ExtensionParseError::UnknownParameter(name.clone());
        prop_assert_eq!(err.to_string(), format!("unknown parameter: {name}"));
    }

    /// DuplicateParameter の Display はパラメータ名を含む固定書式
    #[test]
    fn prop_duplicate_parameter_display(name in any::<String>()) {
        let err = ExtensionParseError::DuplicateParameter(name.clone());
        prop_assert_eq!(err.to_string(), format!("duplicate parameter: {name}"));
    }

    /// MissingValue の Display はパラメータ名を含む固定書式
    #[test]
    fn prop_missing_value_display(name in any::<String>()) {
        let err = ExtensionParseError::MissingValue(name.clone());
        prop_assert_eq!(err.to_string(), format!("missing value for parameter: {name}"));
    }

    /// UnexpectedValue の Display はパラメータ名を含む固定書式
    #[test]
    fn prop_unexpected_value_display(name in any::<String>()) {
        let err = ExtensionParseError::UnexpectedValue(name.clone());
        prop_assert_eq!(err.to_string(), format!("unexpected value for parameter: {name}"));
    }

    /// InvalidValue の Display は detail をそのまま出力する
    ///
    /// 生成側 (from_extension_validated) が既に完結したメッセージを構築しているため、
    /// 任意の文字列でラウンドトリップが成立することを検証する。検証式は同義反復に近いが、
    /// 将来 Display 実装で誤って prefix を付ける等の回帰を検出するためのガードとして残す。
    #[test]
    fn prop_invalid_value_display(detail in any::<String>()) {
        let err = ExtensionParseError::InvalidValue(detail.clone());
        prop_assert_eq!(err.to_string(), detail);
    }
}
