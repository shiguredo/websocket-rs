//! websocket_extension.rs の単体テスト
//!
//! ランダム生成する入力を持たないケース（`NotDeflate` の固定文言、`std::error::Error` トレイト実装の存在）
//! を単体テストとして置く。CLAUDE.md「単体テスト: 意図的なエラーパス、境界値など PBT で実現できないケース」
//! に該当する。

use shiguredo_websocket::ExtensionParseError;

#[test]
fn not_deflate_の_display_は固定文言と一致する() {
    let err = ExtensionParseError::NotDeflate;
    assert_eq!(err.to_string(), "extension is not permessage-deflate");
}

/// `impl std::error::Error for ExtensionParseError {}` の存在を回帰検出するためのテスト。
/// `impl` が消されると `&dyn std::error::Error` への代入と `.source()` 呼び出しが
/// コンパイルエラーになるため、Display 経由のテストだけでは見逃される回帰を捕捉できる。
#[test]
fn extension_parse_error_は_std_error_を実装している() {
    let err = ExtensionParseError::NotDeflate;
    let dyn_err: &dyn std::error::Error = &err;
    assert!(dyn_err.source().is_none());
}
