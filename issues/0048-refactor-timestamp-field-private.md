# Timestamp の pub 内部フィールドを private にする

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: mimo-v2.5-pro
- Branch: feature/change-timestamp-field-private

## 目的

`Timestamp(pub u64)` のタプル構造体直接構築を廃止し、`from_millis` / `as_millis` に統一する。

## 優先度根拠

Low。API 一貫性。`[CHANGE]`（`Timestamp(0)` 構文の利用者があれば破壊）。

## 現状

- `src/time.rs:5`: `pub struct Timestamp(pub u64)`
- `from_millis` / `as_millis` は既に存在

## 設計方針

`Timestamp(u64)` → `Timestamp(u64)` で private 化。`rg 'Timestamp\\('` でクレート全体を置換。

## CHANGES.md 登録内容

`## develop` に:

```markdown
- [CHANGE] Timestamp の内部フィールドを private にする
  - @<GitHub ハンドル>
```

## 完了条件

- タプルフィールドが private
- クレート内に `Timestamp(` の公開構築子利用が無い（`from_millis` のみ）
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[CHANGE]` と担当者行がある
