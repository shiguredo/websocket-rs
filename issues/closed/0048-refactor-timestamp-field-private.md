# Timestamp の pub 内部フィールドを private にする

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: mimo-v2.5-pro
- Branch: feature/change-timestamp-field-private

## 目的

`Timestamp(pub u64)` のタプル構造体直接構築を廃止し、`from_millis` / `as_millis` に統一する。

## 優先度根拠

Low。API 一貫性。`[CHANGE]`（`Timestamp(0)` 構文の利用者があれば破壊）。

## 現状

- `src/time.rs:5`: `pub struct Timestamp(pub u64)`
- `from_millis` / `as_millis` は既に存在
- `rg 'Timestamp\('` で確認したところ、クレート内に `Timestamp(N)` 形式の直接構築箇所は **定義行（`src/time.rs:5`）以外に存在しない**。利用側は既に `Timestamp::from_millis(...)` 経由に統一済みのため、フィールドの可視性変更のみで完結する。

## 設計方針

`pub struct Timestamp(pub u64);` の内側のフィールド可視性を `pub` から削り `pub struct Timestamp(u64);` にする。クレート内の構築は既に `Timestamp::from_millis` 経由に統一されているため、置換作業は不要。外部クレートが `Timestamp(0)` 等で直接構築している場合のみコンパイルエラーで検知される（意図した破壊的変更）。

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

## 解決方法

`src/time.rs` の `pub struct Timestamp(pub u64);` を `pub struct Timestamp(u64);` に変更し、タプルフィールドの可視性を `pub` から private に下げた。

事前の grep で確認した通り、クレート内で `Timestamp(N)` 形式の直接構築箇所は定義行以外に存在せず、置換作業は不要。`from_millis` / `as_millis` / `saturating_sub` / `add_millis` / `Add` / `Sub` 実装はすべて維持。挙動変化なし、`cargo test --workspace` 全件パス。

外部クレートが `Timestamp(0)` 等で直接構築している場合のみコンパイルエラー (`[CHANGE]` 後方互換のない変更)。
