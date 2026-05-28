# 解決済み issue を参照する TODO コメントを整理する

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: mimo-v2.5-pro
- Branch: feature/refactor-remove-resolved-todo

## 目的

`src/websocket_handshake.rs` に残る `TODO: issues/closed/0003-...` 参照を整理する。`0003` は **未修正のまま closed**（将来対応の設計メモ）であり、TODO プレフィクスは「未着手作業」と誤解される。

## 優先度根拠

Low。技術的文脈（`,` 単純分割の限界、将来の stateful パーサー必要性）は有効なため、コメント本文は残し **TODO プレフィクスと issue パス参照のみ除去**する。

## 現状

`src/websocket_handshake.rs` の 3 箇所（行 417 / 620 / 692 付近）に同一趣旨のブロック:

- `TODO: issues/closed/0003-rfc-quoted-string-split-in-extensions.md`
- RFC 9110 Section 5.6.1 の `#rule` と quoted-string 誤分割の説明

`Extension::parse_strict` / `split_respecting_quotes` は extension モジュール側で改善済みだが、**ハンドシェイク経路の `,` 単純分割は未置換**（`0003` の closed 理由どおり）。

## 設計方針

各ブロックで:

1. 先頭行の `TODO: issues/closed/0003-...` を削除
2. 残りの技術説明（RFC 参照、permessage-deflate は token のみで実害限定、将来 stateful 化）を通常コメントとして維持
3. 必要なら「ハンドシェイク層の list 分割は未対応」と一文で明示（issue パスは書かない）

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] 解決済み issue 参照の TODO コメントを設計メモコメントに整理する
  - @<GitHub ハンドル>
```

## 完了条件

- 3 箇所から `TODO:` プレフィクスと `issues/closed/0003` パス参照が無い
- 技術的文脈のコメントが残っている
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある
