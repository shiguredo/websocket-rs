# 不要に公開されている内部 API を #[doc(hidden)] に変更する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/refactor-internal-api-doc-hidden

## 目的

外部利用者が直接使う想定が薄い内部 API を、通常の公開 API から切り離す。`pub(crate)` への変更は **採用しない**（後述の PBT / fuzz が別クレートのため）。

## 優先度根拠

Medium。不要な公開 API は将来の変更コストを上げる。`Compressor` のシグネチャ変更などが外部に波及しうる。

## 現状

| シンボル | 定義 | `src/` 内利用 | 外部利用 |
|---|---|---|---|
| `ByteSliceExt` / `VecExt` | `src/buf.rs` | 無し（PBT のみ） | `pbt/tests/prop_buf.rs` |
| `Compressor` | `src/deflate/permessage_deflate.rs` | 内部のみ | なし（`PerMessageDeflate` 経由で内部利用） |
| `Decompressor` | `src/deflate/permessage_deflate.rs` | 内部 | `fuzz/fuzz_targets/fuzz_deflate_decompress.rs` |
| `truncate_reason` | `src/websocket_close.rs` | 内部 | `pbt/tests/prop_close_code.rs` |

`src/lib.rs` では通常の `pub use` で公開されている。

## 設計方針

1. 定義側の可視性は **`pub` のまま**（`pub(crate)` にすると `pbt` / `fuzz` から参照不可）
2. `src/lib.rs` の通常 `pub use` から上記を除外
3. 同じシンボルを `#[doc(hidden)] pub use` で再エクスポート（PBT / fuzz 用。ドキュメント上は非表示）
4. `truncate_reason` の定義から `#[doc(hidden)]` を削除（`lib.rs` 側で hidden 再エクスポートするため）
5. `src/deflate/mod.rs` の `pub use` から `Compressor` / `Decompressor` を除外（`PerMessageDeflate` のみ維持）

`ByteSliceExt` / `VecExt` は `src/` 未使用だが PBT が依存するため **削除は本 issue のスコープ外**。

## 実装手順

1. `src/lib.rs` の `pub use buf::{ByteSliceExt, VecExt}` / `pub use deflate::{Compressor, Decompressor, ...}` / `pub use websocket_close::{..., truncate_reason}` から対象シンボルを除外
2. `src/lib.rs` に `#[doc(hidden)] pub use` ブロックを追加し、上記シンボルを再エクスポート
3. `src/websocket_close.rs` の `truncate_reason` 関数定義から `#[doc(hidden)]` を除去
4. `src/deflate/mod.rs` の `pub use permessage_deflate::{...}` から `Compressor` / `Decompressor` を除外（`PerMessageDeflate` のみ維持）
5. `cargo test --workspace` および `cargo fuzz build` で回帰確認

## テスト戦略

- 新規テスト不要
- `cargo test --workspace` で既存テスト・PBT の回帰確認
- `cargo fuzz build` で fuzz ターゲットのコンパイル確認

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] 内部向け API を通常の pub use から除外し #[doc(hidden)] で再エクスポートする
  - @<GitHub ハンドル>
```

## 完了条件

- `ByteSliceExt` / `VecExt` / `Compressor` / `Decompressor` / `truncate_reason` の定義は `pub` のまま
- `lib.rs` の通常 `pub use` に上記が含まれていない
- `lib.rs` に `#[doc(hidden)] pub use` で再エクスポートがある
- `truncate_reason` 定義の `#[doc(hidden)]` が削除されている
- `src/deflate/mod.rs` の公開再エクスポートが `PerMessageDeflate` のみ
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo test --workspace` が全件パスする
- `cargo fuzz build` が通過する（fuzz ターゲットのコンパイル確認）
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある
