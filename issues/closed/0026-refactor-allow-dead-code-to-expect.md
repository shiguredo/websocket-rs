# #[allow(dead_code)] を整理する（未使用のみ #[expect(dead_code)]）

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/refactor-allow-dead-code-to-expect

## 目的

AGENTS.md の規約に違反している `#[allow(dead_code)]` を整理する。

AGENTS.md で「lint 警告を抑制する必要がある時は `#[allow(...)]` ではなく `#[expect(...)]` を使う」と明記されている。5 箇所のうち実際に未使用なのは 1 箇所のみであるため、一括置換ではなく各メソッドの使用状況に応じて属性削除または `#[expect(dead_code)]` を選択する。

本 issue は closed `0019`（`encode_unmasked` の `#[allow(dead_code)]` 除去）の後追いであり、残存 5 箇所を対象とする。`0028`（doc コメントの `.unwrap()` 整理）とは独立して並行実装可能。

## 優先度根拠

Medium。lint 属性の変更はランタイム挙動・公開 API に影響しないが、AGENTS.md の基本規約違反である。動作に影響するバグや機能追加と優先度が衝突しないため Medium とする。

## 現状

`src/` 内の `#[allow(dead_code)]` は次の 5 箇所のみ（`rg '#\[allow\(dead_code\)\]' --glob '*.rs' src` で確認可能）。`src/` 内に `#[expect(...)]` の先例はない。

| メソッド | ファイル | 可視性 | 使用箇所数 (定義除く) | 備考 | 対応 |
|---|---|---|---|---|---|
| `invalid_input` | error.rs:100 | `pub(crate)` | 27 (`src/` のみ) | 過去の誤付与と推定。lint はもともと不要 | 属性削除 |
| `unsupported` | error.rs:116 | `pub(crate)` | 0 | `src/` 内で未使用。PBT では `ErrorKind::Unsupported` を直接使用 | `#[expect(dead_code)]` に変更 |
| `handshake_rejected` | error.rs:132 | `pub(crate)` | 56 (`src/` のみ) | 過去の誤付与と推定。lint はもともと不要 | 属性削除 |
| `with_close_code` | error.rs:157 | `pub` | 4 (`pbt/tests/prop_error.rs` のみ) | `pub` は `dead_code` 対象外 | 属性削除 |
| `buffer_len` | websocket_frame.rs:324 | `pub` | 0 | `pub` は `dead_code` 対象外。メソッド自体は公開 API として存続 | 属性削除 |

`unsupported` を `#[expect(dead_code)]` で温存する理由: 他の `ErrorKind` バリアントにはすべて対応するコンストラクタが存在しており、`Unsupported` のみ削除すると一貫性が崩れる。関数本体の削除は別 issue とし、本 issue では lint 規約違反の解消に限定する。将来 `Error::unsupported` を呼び出した場合は `expect` が `unfulfilled` となり、使用開始を検知できる。

## 設計方針

- 上記表の「対応」列どおりに編集する（機械的一括置換禁止）
- 操作対象は `#[allow(dead_code)]` のみ。同メソッドに付与されている `#[track_caller]` 等の他属性はそのまま維持する
- 実装後、`src/` で `#[allow(dead_code)]` が 0 件、`#[expect(dead_code)]` が `unsupported` の 1 件のみであること
- 公開 API・ランタイム挙動に変更なし（lint 属性の整理のみ）

## 変更しないファイル

- `pbt/tests/prop_error.rs`（`with_close_code` の利用確認用。編集不要）

## テスト戦略

- 新規テスト・PBT・fuzz の追加・変更は不要（lint 属性のみの変更）
- 回帰: `cargo test --workspace` および既存 `prop_error` の `with_close_code` 関連テストで十分

## 実装手順

1. `rg '#\[allow\(dead_code\)\]' --glob '*.rs' src` で対象が 5 件のみであることを確認
2. 各メソッドの使用状況を確認（`rg '\.invalid_input\(' src`、`rg '\.unsupported\(' src`、`rg '\.handshake_rejected\(' src` で表の使用箇所数と一致することを検証）。不一致の場合は実装を中断し issue を再調査する
3. 「現状」表のとおり 5 メソッドを編集
4. `rg '#\[allow\(dead_code\)\]' --glob '*.rs' src` が 0 件、`rg '#\[expect\(dead_code\)\]' --glob '*.rs' src` が `unsupported` の 1 件のみであることを確認
5. `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` を実行
6. `CHANGES.md` を更新

## CHANGES.md 登録内容

`## develop` > `### misc` に次を追加する:

```markdown
- [UPDATE] 不要な `#[allow(dead_code)]` を削除し、未使用の `unsupported` のみ `#[expect(dead_code)]` に変更する
  - @<GitHub ハンドル>
```

## 完了条件

- 上記 5 箇所が表の「対応」どおりに変更されている
- `src/` に `#[allow(dead_code)]` が 0 件、`#[expect(dead_code)]` が `unsupported` の 1 件のみ
- `cargo fmt --all -- --check` が通過する
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo test --workspace` が全件パスする
- `CHANGES.md` の `## develop` > `### misc` に上記 `[UPDATE]` エントリと担当者行が追加されている

## 解決方法

`src/error.rs` の `invalid_input` (l.100), `handshake_rejected` (l.133), `with_close_code` (l.157) の `#[allow(dead_code)]` を削除し、`unsupported` (l.118) のみ `#[expect(dead_code)]` に変更した。あわせて `unsupported` の上に温存理由を示す日本語コメントを追加した。`src/websocket_frame.rs` の `buffer_len` (l.324) の `#[allow(dead_code)]` も削除した。`#[track_caller]` 等の他属性はそのまま維持している。

検証:

- `rg '#\[allow\(dead_code\)\]' --glob '*.rs' src` が 0 件
- `rg '#\[expect\(dead_code\)\]' --glob '*.rs' src` が `src/error.rs:117` の 1 件のみ
- `cargo fmt --all -- --check` 通過
- `cargo clippy --workspace --all-targets -- -D warnings` 通過
- `cargo test --workspace` 全件パス
