# prop_error.rs に ErrorKind の全バリアントを追加する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: mimo-v2.5-pro
- Branch: feature/add-error-kind-variants

## 目的

`pbt/tests/prop_error.rs` の `ERROR_KINDS` に不足している `HttpResponse` と `VersionNotSupported` を追加し、`ErrorKind` 全 10 バリアントを PBT の入力空間に含める。

## 優先度根拠

Medium。`VersionNotSupported` はハンドシェイクのバージョン不一致パス。`ERROR_KINDS` を型の全バリアントと揃えるのは PBT の基本。

## 現状

- `pbt/tests/prop_error.rs:11-20`: 8 バリアントのみ
- `src/error.rs`: `ErrorKind` は 10 バリアント（`HttpResponse`, `VersionNotSupported` を含む）

## 設計方針

`ERROR_KINDS` に `ErrorKind::VersionNotSupported` と `ErrorKind::HttpResponse` を追加。`HttpResponse` 固有の `HttpResponseInfo` 検証は本 issue のスコープ外（`ErrorKind` の列挙カバレッジのみ）。

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] prop_error.rs の ERROR_KINDS に全 ErrorKind バリアントを含める
  - @<GitHub ハンドル>
```

## 完了条件

- `ERROR_KINDS` が 10 バリアントを含む
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある

## 解決方法

- `pbt/tests/prop_error.rs` の `ERROR_KINDS` 配列に `ErrorKind::VersionNotSupported` と `ErrorKind::HttpResponse` を追加し、配列サイズを `[ErrorKind; 8]` から `[ErrorKind; 10]` に変更した
- 並び順は `src/error.rs` の `pub enum ErrorKind` の定義順 (`InvalidInput / InvalidData / InvalidState / InsufficientBuffer / Unsupported / ProtocolViolation / HandshakeRejected / VersionNotSupported / InvalidUtf8 / HttpResponse`) と一致させた
- `Error::new` / `Error::with_reason` / Display 実装は `ErrorKind` を特別扱いしないため後続 PBT (`prop_error_new` 等) に影響なし
- `CHANGES.md` の `## develop` `### misc` 末尾に `- [UPDATE] prop_error.rs の ERROR_KINDS に全 ErrorKind バリアントを含める` を追記した
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が全件パスすることを確認した
