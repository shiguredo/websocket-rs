//! websocket_extension.rs のプロパティベーステスト
//!
//! 目的:
//! - ExtensionParseError の Display 出力のプロパティ検証
//! - window bits パラメータの値検証 (符号・先頭ゼロ・非数字・範囲)
//!
//! NotDeflate は引数を持たず PBT 化に意味が無いため、対応する単体テストは
//! `tests/test_websocket_extension.rs` 側に配置している。

use shiguredo_websocket::{Extension, ExtensionParseError, PerMessageDeflateConfig};

/// UnknownParameter の Display はパラメータ名を含む固定書式
#[test]
fn prop_unknown_parameter_display() -> noprop::TestResult {
    pbt::run(|ctx| {
        let name = pbt::sample_text(ctx, 0, 99);
        let err = ExtensionParseError::UnknownParameter(name.clone());
        assert_eq!(err.to_string(), format!("unknown parameter: {name}"));
        Ok(())
    })?;
    Ok(())
}

/// DuplicateParameter の Display はパラメータ名を含む固定書式
#[test]
fn prop_duplicate_parameter_display() -> noprop::TestResult {
    pbt::run(|ctx| {
        let name = pbt::sample_text(ctx, 0, 99);
        let err = ExtensionParseError::DuplicateParameter(name.clone());
        assert_eq!(err.to_string(), format!("duplicate parameter: {name}"));
        Ok(())
    })?;
    Ok(())
}

/// MissingValue の Display はパラメータ名を含む固定書式
#[test]
fn prop_missing_value_display() -> noprop::TestResult {
    pbt::run(|ctx| {
        let name = pbt::sample_text(ctx, 0, 99);
        let err = ExtensionParseError::MissingValue(name.clone());
        assert_eq!(
            err.to_string(),
            format!("missing value for parameter: {name}")
        );
        Ok(())
    })?;
    Ok(())
}

/// UnexpectedValue の Display はパラメータ名を含む固定書式
#[test]
fn prop_unexpected_value_display() -> noprop::TestResult {
    pbt::run(|ctx| {
        let name = pbt::sample_text(ctx, 0, 99);
        let err = ExtensionParseError::UnexpectedValue(name.clone());
        assert_eq!(
            err.to_string(),
            format!("unexpected value for parameter: {name}")
        );
        Ok(())
    })?;
    Ok(())
}

/// InvalidValue の Display は detail をそのまま出力する
///
/// 生成側 (parse_window_bits など) が既に完結したメッセージを構築しているため、
/// 任意の文字列でラウンドトリップが成立することを検証する。検証式は同義反復に近いが、
/// 将来 Display 実装で誤って prefix を付ける等の回帰を検出するためのガードとして残す。
#[test]
fn prop_invalid_value_display() -> noprop::TestResult {
    pbt::run(|ctx| {
        let detail = pbt::sample_text(ctx, 0, 99);
        let err = ExtensionParseError::InvalidValue(detail.clone());
        assert_eq!(err.to_string(), detail);
        Ok(())
    })?;
    Ok(())
}

/// RFC 7692 Section 7.1.2.1 / 7.1.2.2: 符号・先頭ゼロ・非数字・u8 オーバーフローの値は不正
#[test]
fn prop_window_bits_rejects_invalid_values() -> noprop::TestResult {
    pbt::run(|ctx| {
        let param =
            noprop::sample_choice(ctx, &["server_max_window_bits", "client_max_window_bits"]);
        let value = noprop::sample_choice(ctx, &["+8", "+08", "-8", "08", "256", "abc"]);
        let text = format!("permessage-deflate; {param}={value}");
        let ext = Extension::parse(&text)
            .into_iter()
            .next()
            .expect("拡張のパースに失敗してはならない");

        for result in [
            PerMessageDeflateConfig::from_extension_for_server_request(&ext),
            PerMessageDeflateConfig::from_extension_for_client_response(&ext),
        ] {
            assert!(
                matches!(result, Err(ExtensionParseError::InvalidValue(_))),
                "{param}={value} は不正値として拒否される"
            );
        }
        Ok(())
    })?;
    Ok(())
}

/// RFC 7692 Section 7.1.2.1 / 7.1.2.2: 8-15 の値は受理される
#[test]
fn prop_window_bits_accepts_valid_values() -> noprop::TestResult {
    pbt::run(|ctx| {
        let param =
            noprop::sample_choice(ctx, &["server_max_window_bits", "client_max_window_bits"]);
        let bits = noprop::sample_u8_in(ctx, 8..=15);
        let text = format!("permessage-deflate; {param}={bits}");
        let ext = Extension::parse(&text)
            .into_iter()
            .next()
            .expect("拡張のパースに失敗してはならない");

        let server = PerMessageDeflateConfig::from_extension_for_server_request(&ext)
            .expect("8-15 の値はサーバー側で受理される");
        let client = PerMessageDeflateConfig::from_extension_for_client_response(&ext)
            .expect("8-15 の値はクライアント側で受理される");

        if param == "server_max_window_bits" {
            assert_eq!(server.server_max_window_bits, Some(bits));
            assert_eq!(client.server_max_window_bits, Some(bits));
        } else {
            assert_eq!(server.client_max_window_bits, Some(bits));
            assert_eq!(client.client_max_window_bits, Some(bits));
        }
        Ok(())
    })?;
    Ok(())
}

/// RFC 7692 Section 7.1.2.1 / 7.1.2.2: 範囲外の値は拒否される
#[test]
fn prop_window_bits_rejects_out_of_range_values() -> noprop::TestResult {
    pbt::run(|ctx| {
        let param =
            noprop::sample_choice(ctx, &["server_max_window_bits", "client_max_window_bits"]);
        let bits = if noprop::sample_bool(ctx) {
            noprop::sample_u8_in(ctx, 0..=7)
        } else {
            noprop::sample_u8_in(ctx, 16..=255)
        };
        let text = format!("permessage-deflate; {param}={bits}");
        let ext = Extension::parse(&text)
            .into_iter()
            .next()
            .expect("拡張のパースに失敗してはならない");

        for result in [
            PerMessageDeflateConfig::from_extension_for_server_request(&ext),
            PerMessageDeflateConfig::from_extension_for_client_response(&ext),
        ] {
            assert!(
                matches!(result, Err(ExtensionParseError::InvalidValue(_))),
                "{param}={bits} は範囲外として拒否される"
            );
        }
        Ok(())
    })?;
    Ok(())
}
