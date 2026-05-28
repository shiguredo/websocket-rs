//! token.rs の単体テスト
//!
//! ランダム生成する入力を持たないケース（空文字列の判定）を単体テストで扱う。
//! CLAUDE.md「単体テスト: 意図的なエラーパス、境界値など PBT で実現できないケース」に該当する。

use shiguredo_websocket::is_valid_token;

#[test]
fn 空文字列は_token_として無効() {
    assert!(!is_valid_token(""));
}
