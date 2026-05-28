# SharedConnectionState のフィールドをカプセル化する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/refactor-shared-state-encapsulation

## 目的

`SharedConnectionState` の `pub(crate)` フィールド直アクセスをやめ、`state` 変更は `set_state` 経由に限定する。

## 優先度根拠

Medium。`state` の直接代入は不正遷移の温床。`0042`（遷移表）の前提となる。

## 現状

`src/websocket_connection_shared.rs:81-98`: 全フィールドが `pub(crate)`。`websocket_client_connection.rs` / `websocket_server_connection.rs` / 同一ファイル内から直接読み書き。

## 設計方針

1. フィールドの `pub(crate)` を削除
2. 読み取り用 `fn state(&self) -> ConnectionState` 等、既存呼び出しに必要な最小限のアクセサを追加
3. `state` の書き込みは既存 `set_state`（または `0042` で強化後の API）のみ
4. 同一モジュール外からの `shared.state = ...` をコンパイルエラーにし、呼び出し側を置換

## 依存

- `0042` と並行可能だが、先に本 issue でフィールドを private 化してから遷移検証を入れると差分が追いやすい

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] SharedConnectionState のフィールドをカプセル化する
  - @<GitHub ハンドル>
```

## 完了条件

- `SharedConnectionState` のフィールドが外部から直接アクセス不可
- 必要なアクセサが追加され、クライアント / サーバー / shared 内の参照がコンパイルできる
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある
