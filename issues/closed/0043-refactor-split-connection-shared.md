# websocket_connection_shared.rs を分割する

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/refactor-split-connection-shared

## 目的

約 710 行の `websocket_connection_shared.rs` を責務ごとに分割し、可読性を上げる。

## 優先度根拠

Low。挙動変更なし。`FragmentBuffer` と `FramePolicy` 族は独立した概念。

## 現状

`src/websocket_connection_shared.rs` に `SharedConnectionState`, `FragmentBuffer`, `FramePolicy`, `ClientFramePolicy`, `ServerFramePolicy` が同居。

## 設計方針

| 新規ファイル | 移動対象 |
|---|---|
| `src/fragment_buffer.rs` | `FragmentBuffer` とその impl |
| `src/frame_policy.rs` | `FramePolicy`, `ClientFramePolicy`, `ServerFramePolicy` |
| `src/websocket_connection_shared.rs` | `SharedConnectionState` とフレーム処理本体 |

`src/lib.rs` に `mod fragment_buffer;` / `mod frame_policy;` を追加。公開 API は `lib.rs` の `pub use` を変更しない（内部整理のみ）。

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] websocket_connection_shared.rs を fragment_buffer / frame_policy に分割する
  - @<GitHub ハンドル>
```

## 完了条件

- 上記分割が完了している
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある

## 解決方法

`src/websocket_connection_shared.rs` を 3 ファイルに分割した:

- `src/fragment_buffer.rs`: `FragmentBuffer` (RFC 6455 Section 5.4 フラグメント収集バッファ)
- `src/frame_policy.rs`: `FramePolicy` トレイト + `ClientFramePolicy` + `ServerFramePolicy` (RFC 6455 Section 5.1 マスク方向の差分)
- `src/websocket_connection_shared.rs`: `SharedConnectionState` 本体 + 共通定数 + 内部単体テスト

`src/lib.rs` に `mod fragment_buffer;` と `mod frame_policy;` を追加。公開 API (`pub use`) は変更なし。`websocket_client_connection.rs` / `websocket_server_connection.rs` の use を新パスに更新。挙動変化なし、`cargo test --workspace` 全件パス。
