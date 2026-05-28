# MAX_PENDING_FRAME_DATA の境界値テストを追加する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
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
