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
- 既存ビルダーメソッド（クライアント / サーバー共通）: `protocol` / `deflate` / `header` / `ping_interval` / `max_frame_size` / `max_message_size` / `max_decompressed_size`。クライアントは追加で `origin`。
- 既存ビルダーが **未提供** のフィールド: `pong_timeout_millis`, `close_timeout_millis`（クライアント / サーバー両方）。これらは現状フィールドへの直接代入でしか設定できない。
- `pbt/tests/prop_client_connection.rs`, `prop_server_connection.rs`: フィールドの **読み取り** には `prop_assert_eq!(options.host, ...)` 等を使用しているが、設定はすでにビルダー経由（再束縛）で行っており **代入は無い**。フィールド private 化は読み取りも遮断するため PBT の参照箇所も影響を受ける。

## 設計方針

1. フィールドを private 化（同一モジュール内の `WebSocket*Connection` からのアクセスは可能）
2. private 化に伴って欠落するセッタを追加する:
   - `ClientConnectionOptions::pong_timeout(mut self, millis: u64) -> Self`
   - `ClientConnectionOptions::close_timeout(mut self, millis: u64) -> Self`
   - `ServerConnectionOptions::pong_timeout(mut self, millis: u64) -> Self`
   - `ServerConnectionOptions::close_timeout(mut self, millis: u64) -> Self`
3. PBT 側のフィールド読み取り（`options.host` 等）はビルダーで設定した値を **入力側で保持** し直してアサートする、または接続インスタンスから取得できる getter があればそちらに置換する。既存ビルダーメソッド（`.max_frame_size()` 等）でカバーされる構築箇所はそのまま維持する。
4. 外部クレートがフィールドに依存している場合はコンパイルエラーで検知（意図した破壊的変更）

## CHANGES.md 登録内容

`## develop` に:

```markdown
- [CHANGE] ClientConnectionOptions / ServerConnectionOptions のフィールドを private にする
  - @<GitHub ハンドル>
```

## 完了条件

- 両 Options のフィールドが private
- `pong_timeout` / `close_timeout` のビルダーが両 Options に存在する
- PBT がビルダーのみで構築し、フィールド読み取りに依存していない
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[CHANGE]` と担当者行がある
