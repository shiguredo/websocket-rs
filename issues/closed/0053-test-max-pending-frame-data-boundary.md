# MAX_PENDING_FRAME_DATA の境界値テストを追加する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/add-max-pending-frame-data-boundary-pbt

## 目的

サーバー側 `MAX_PENDING_FRAME_DATA`（1 MiB）の境界（ちょうど成功・1 バイト超過で失敗）を PBT で検証する。

## 優先度根拠

Medium。ハンドシェイク前の DoS 防御（closed `0007`）。オフバイワンを PBT で抑止。

## 現状

- `src/websocket_server_connection.rs:25-26`: `MAX_PENDING_FRAME_DATA = 1024 * 1024`
- `pbt/tests/prop_server_connection.rs`: 小サイズのみ

## 設計方針

`prop_server_connection.rs` に:

- `Connecting` 状態で `pending` 合計が `MAX_PENDING_FRAME_DATA` ちょうど → 成功（または許容される挙動を既存実装から固定）
- `MAX_PENDING_FRAME_DATA + 1` → `Error`（種別は実装に合わせてアサート）

1 MiB のバイト列を毎回生成しないよう、チャンク分割フィード strategy を用いる。

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] MAX_PENDING_FRAME_DATA の境界値を PBT で検証する
  - @<GitHub ハンドル>
```

## 完了条件

- 境界ちょうど / +1 の 2 ケースが PBT にある
- `cargo test --workspace` が全件パスする

## 解決方法

`pbt/tests/prop_server_connection.rs` に 2 件の PBT を追加した:

- `prop_pending_frame_data_at_exact_limit_is_accepted`: 合計バイト数が `MAX_PENDING_FRAME_DATA` (1 MiB) ちょうどなら受理される
- `prop_pending_frame_data_over_limit_by_one_is_rejected`: 境界ちょうどまで詰めた後に 1 バイト追加すると `Err(ErrorKind::ProtocolViolation)` が返る

任意の `chunk_size` (1〜65,536 バイト) で proptest が生成する分割幅でフィードし、`setup_pending_request_server` ヘルパでハンドシェイクリクエスト受信直後 (Connecting 状態、`pending_request = Some`) からの追加データを `feed_chunks_to_pending` で投入する。

`MAX_PENDING_FRAME_DATA` 定数は `src/websocket_server_connection.rs:24` の値と同じ値を PBT ファイル冒頭に再定義 (両側を同期する想定)。`cargo test --workspace` 全件パス。
