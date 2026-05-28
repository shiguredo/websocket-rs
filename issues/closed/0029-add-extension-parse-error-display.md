# ExtensionParseError に Display / Error を実装し公開する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/add-extension-parse-error-display

## 目的

`ExtensionParseError` に `Display` と `std::error::Error` を実装し、`lib.rs` から公開する。

`PerMessageDeflateConfig::from_extension_for_client_response` / `from_extension_for_server_request` は公開 API として `Result<_, ExtensionParseError>` を返すが、現状 `ExtensionParseError` は `Debug` のみ derive であり `lib.rs` でも未公開のため、利用者は `to_string()` / `format!("{e}")` / `dyn Error` 連携ができない。`0034`（`parse_strict` のエラー型統一）は本 issue で `ExtensionParseError` が `Display + Error` を実装済みになることを前提とするため、本 issue 完了後に着手する。

## 優先度根拠

Medium。拡張ネゴシエーション失敗時のエラー報告が利用者コードで困難。セキュリティクリティカルなクラッシュではないが、公開 `Result` とエラー型の非公開は API として不整合。

## 現状

- `src/websocket_extension.rs:19-34`: `ExtensionParseError` は `#[derive(Debug, Clone, PartialEq, Eq)]` のみ（`Display` / `Error` なし）
- `src/lib.rs`: `pub use websocket_extension::{Extension, ExtensionParam, PerMessageDeflateConfig}` のみ。`ExtensionParseError` / `ExtensionParseContext` は未公開
- `from_extension_validated` は `pub fn` であり引数に `ExtensionParseContext` を取る。利用者がこの関数を呼ぶには `ExtensionParseContext` も公開が必要
- `pbt/tests/prop_websocket_extension.rs` は未存在（本 issue で新規作成）

## 設計方針

1. `src/websocket_extension.rs` に `impl Display for ExtensionParseError` を追加（メッセージは英語）
2. `impl std::error::Error for ExtensionParseError` を追加（`source()` は常に `None`）
3. `src/lib.rs` の `pub use websocket_extension::{ ... }` に `ExtensionParseError`, `ExtensionParseContext` を追加

### Display 出力仕様

| バリアント | 出力 |
|---|---|
| `NotDeflate` | `extension is not permessage-deflate` |
| `UnknownParameter(name)` | `unknown parameter: {name}` |
| `DuplicateParameter(name)` | `duplicate parameter: {name}` |
| `MissingValue(name)` | `missing value for parameter: {name}` |
| `UnexpectedValue(name)` | `unexpected value for parameter: {name}` |
| `InvalidValue(detail)` | `{detail}` をそのまま（生成側が既に `server_max_window_bits: ...` 形式で埋め込む） |

`InvalidValue` の既存 `detail` 例は `from_extension_validated` 内の `format!(...)` をそのまま利用する（Display 実装で再フォーマットしない）。`InvalidValue` のみ `{detail}` をそのまま出力し他バリアントとフォーマット構造が異なるのは、生成側が既にパラメータ名と詳細を含む自己完結的なメッセージを構築しているため。

## 実装手順

1. `src/websocket_extension.rs` に `impl Display for ExtensionParseError` を追加
2. `src/websocket_extension.rs` に `impl std::error::Error for ExtensionParseError` を追加（`source()` は `None`）
3. `src/lib.rs` の `pub use websocket_extension::{ ... }` に `ExtensionParseError`, `ExtensionParseContext` を追加
4. `pbt/tests/prop_websocket_extension.rs` を新規作成（下記 PBT）
5. `cargo test --workspace` で全テスト通過を確認

### PBT（`pbt/tests/prop_websocket_extension.rs`）

- ファイル名は `src/websocket_extension.rs` に対応（AGENTS.md 規約: `prop_<module>.rs`）
- `ExtensionParseError` の各バリアントを strategy で生成し、以下を検証:
  - `NotDeflate`: `to_string()` が固定文言 `"extension is not permessage-deflate"` と一致
  - パラメータ名を持つバリアント: `to_string()` がバリアント固有の prefix（`"unknown parameter: "` 等）で始まり、パラメータ名を含む
  - `InvalidValue`: 任意の `detail: String` で `to_string() == detail` が成立する

## テスト戦略

- PBT: `pbt/tests/prop_websocket_extension.rs`（Display 文言のプロパティ検証）
- 単体テスト: 不要（PBT で各バリアントをカバー）
- fuzzing: 不要

## CHANGES.md 登録内容

`## develop` 直下に次を追加する（機能影響ありのため `### misc` ではなく直下。AGENTS.md 規約により `[ADD]` は既存の `[UPDATE]` エントリより前に配置する）:

```markdown
- [ADD] ExtensionParseError / ExtensionParseContext に Display / Error を実装し lib.rs から公開する
  - @<GitHub ハンドル>
```

## 完了条件

- `ExtensionParseError` が `Display` と `std::error::Error` を実装している
- `lib.rs` で `ExtensionParseError` と `ExtensionParseContext` が公開されている
- `pbt/tests/prop_websocket_extension.rs` が追加され、String を持つ全バリアントの Display 出力を検証する PBT が含まれている
- `tests/test_websocket_extension.rs` が追加され、`NotDeflate` の固定文言を検証する単体テストが含まれている
- `cargo fmt --all -- --check` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo test --workspace` が全件パスする
- `CHANGES.md` の `## develop` に上記 `[ADD]` エントリと担当者行が追加されている

## 解決方法

`src/websocket_extension.rs` の `ExtensionParseError` に `impl std::fmt::Display` と `impl std::error::Error` を追加した。Display 出力は issue 表のとおりで、`InvalidValue` のみ生成側が構築する detail をそのまま出力する。`src/lib.rs` の `pub use` に `ExtensionParseContext` と `ExtensionParseError` を追加して公開した。

PBT は `pbt/tests/prop_websocket_extension.rs` に配置し、入力 `String` を持つ 5 バリアント（`UnknownParameter` / `DuplicateParameter` / `MissingValue` / `UnexpectedValue` / `InvalidValue`）を `any::<String>()` の strategy で検証する。`NotDeflate` は引数を持たないため PBT ではなく `tests/test_websocket_extension.rs` の単体テストで固定文言の一致を検証する（CLAUDE.md「pbt 以下に unittest を書かないこと」「PBT は型情報に基づき入力を生成しプロパティを検証する」に従う）。

検証:

- `cargo fmt --all -- --check` 通過
- `cargo clippy --workspace --all-targets -- -D warnings` 通過
- `cargo test --workspace --test prop_websocket_extension` で 5 件すべて通過
- `cargo test --workspace --test test_websocket_extension` で 1 件通過
- `cargo test --workspace` 全件パス
