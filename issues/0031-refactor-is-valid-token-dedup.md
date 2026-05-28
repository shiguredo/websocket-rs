# is_valid_token の重複実装を共通化する

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: mimo-v2.5-pro
- Branch: feature/refactor-is-valid-token-dedup

## 目的

RFC 9110 Section 5.6.2 の token ABNF 検証 `is_valid_token` が 2 箇所に重複しており、修正漏れのリスクがある。`src/token.rs` に 1 箇所へ集約する。

## 優先度根拠

Low。いずれもプライベート関数で外部 API ではないが、同一ロジックの二重管理は保守性を損なう。

## 現状

- `src/websocket_extension.rs:268-276`: `impl Extension` 内の `fn is_valid_token`（呼び出し 8 箇所、`Self::is_valid_token`）
- `src/websocket_handshake.rs:677-685`: モジュール private `fn is_valid_token`（呼び出し 7 箇所）

両者の実装は同一。

## 設計方針

1. `src/token.rs` を新規作成し `pub(crate) fn is_valid_token(s: &str) -> bool` を定義（根拠: RFC 9110 Section 5.6.2 token）
2. `src/lib.rs` に `mod token;` を追加
3. `websocket_extension.rs` / `websocket_handshake.rs` のローカル定義を削除し `crate::token::is_valid_token` を参照

## 実装手順

1. `src/token.rs` を新規作成し `pub(crate) fn is_valid_token(s: &str) -> bool` を定義
2. `src/lib.rs` に `mod token;` を追加
3. `src/websocket_extension.rs` の `impl Extension` 内 `fn is_valid_token` を削除し、呼び出し箇所を `Self::is_valid_token(...)` から `crate::token::is_valid_token(...)` に変更
4. `src/websocket_handshake.rs` のモジュール private `fn is_valid_token` を削除し、呼び出し箇所を `crate::token::is_valid_token(...)` に変更
5. `rg 'fn is_valid_token' src` が `token.rs` の 1 件のみであることを確認
6. `cargo test --workspace` で回帰確認

## テスト戦略

- PBT: `pbt/tests/prop_token.rs` を新規追加。以下のプロパティを検証:
  - tchar のみから構成される非空文字列を strategy で生成し、`is_valid_token` が `true` を返すこと
  - 非 tchar 文字（delimiter、制御文字等）を 1 文字以上含む文字列で `is_valid_token` が `false` を返すこと
  - 空文字列で `is_valid_token` が `false` を返すこと
- 単体テスト: 不要（PBT でカバー）
- fuzzing: 不要（`is_valid_token` はパニックしない自明な関数であり、fuzzing 対象としての優先度が低い）

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] is_valid_token の重複実装を src/token.rs に共通化する
  - @<GitHub ハンドル>
```

## 完了条件

- `is_valid_token` が `src/token.rs` に 1 定義のみ
- `websocket_extension.rs` / `websocket_handshake.rs` が `crate::token::is_valid_token` を参照
- `pbt/tests/prop_token.rs` が追加されている
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある
