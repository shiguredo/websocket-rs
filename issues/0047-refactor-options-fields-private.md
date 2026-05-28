# ClientConnectionOptions / ServerConnectionOptions のフィールドを private にする

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: mimo-v2.5-pro
- Branch: feature/change-options-fields-private

## 目的

Options 構造体の `pub` フィールドを `private` にし、ビルダー（メソドチェーン）経由のみで設定する。

## 優先度根拠

Medium。公開フィールドとビルダーの二重経路は API 表面を曖昧にする。後方互換のない変更（`[CHANGE]`）。

## 現状

- `src/websocket_client_connection.rs:23-48`: `ClientConnectionOptions` 全フィールド `pub`
- `src/websocket_server_connection.rs:29-48`: `ServerConnectionOptions` 全フィールド `pub`
- `pbt/tests/prop_client_connection.rs`, `prop_server_connection.rs`: フィールド直接代入あり

## 設計方針

1. フィールドを private 化（同一モジュール内の `WebSocket*Connection` からのアクセスは可能）
2. PBT は既存ビルダーメソッド（`.max_frame_size()` 等）へ置換
3. 外部クレートがフィールドに依存している場合はコンパイルエラーで検知（意図した破壊的変更）

## CHANGES.md 登録内容

`## develop` に:

```markdown
- [CHANGE] ClientConnectionOptions / ServerConnectionOptions のフィールドを private にする
  - @<GitHub ハンドル>
```

## 完了条件

- 両 Options のフィールドが private
- PBT がビルダーのみで構築している
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[CHANGE]` と担当者行がある
