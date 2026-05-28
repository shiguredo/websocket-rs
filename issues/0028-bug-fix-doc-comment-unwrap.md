# doc comment 内の .unwrap() を .expect() に修正する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/refactor-doc-comment-unwrap

## 目的

AGENTS.md の規約に違反している doc comment 内の `.unwrap()` を `.expect("MESSAGE")` に修正する。

AGENTS.md で「`.unwrap()` ではなく `.expect("MESSAGE")` を使用する」と明記されている。doc comment 内のコード例は利用者がコピーペーストするため規約の対象とする（「Don't live with broken windows」原則）。

## 優先度根拠

Medium。規約違反がクレートの入門例（`src/lib.rs` の crate-level doc）に残っており、利用者がコピーした際に `.unwrap()` 慣習を広げるリスクがある。ただしランタイム挙動に影響しない doc 1 行の修正であり、最優先ではないため Medium とする。

## 現状

`src/lib.rs` の crate-level doc（`//!`）内、クライアント接続例の 1 行のみ:

```43:43:src/lib.rs
//! ws.connect().unwrap();
```

`src/` 内の実行コードに `.unwrap()` は無い（`rg '\\.unwrap\\(' src` で本行のみ）。

## 設計方針

`ws.connect().unwrap();` を `ws.connect().expect("handshake must succeed");` に変更する。メッセージは英語とし、doc 例であるため「デモ用途では接続成功を前提とする」旨が分かる文言とする。

doc 例は `no_run` 属性付きのため実行はされないが、コンパイルは検証される。`no_run` は維持する（`connect()` はネットワーク接続を要するため）。

## 実装手順

1. `src/lib.rs:43` の `//! ws.connect().unwrap();` を `//! ws.connect().expect("handshake must succeed");` に置換
2. `rg '\.unwrap\(' src` で `src/` 内に `.unwrap()` が残っていないことを確認
3. `cargo test --doc` で doctest のコンパイルが通ることを確認

## テスト戦略

- 新規テスト・PBT・fuzz の追加は不要
- `cargo test --doc` で doc 例のコンパイルを検証（`no_run` のため実行はされない）

## CHANGES.md 登録内容

`## develop` > `### misc` に次を追加する:

```markdown
- [UPDATE] crate-level doc 内の `.unwrap()` を `.expect()` に修正する
  - @<GitHub ハンドル>
```

## 完了条件

- `src/lib.rs` の doc 内に `.unwrap()` が残っていない（`rg '\.unwrap\(' src` が 0 件）
- `cargo test --doc` が通過する（コンパイル検証）
- `CHANGES.md` の `## develop` > `### misc` に上記 `[UPDATE]` エントリと担当者行が追加されている
