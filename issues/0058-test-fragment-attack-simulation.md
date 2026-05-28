# 大量フラグメント攻撃のシミュレーションテストを追加する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/add-fragment-attack-simulation-pbt

## 目的

`max_message_size` を小さく設定し、多数の小フラグメントで上限到達時にメッセージが拒否されることを PBT で検証する。

## 優先度根拠

Medium。古典的 DoS ベクトル。既存フラグメント PBT は 2–5 パートのみ。

## 現状

- `pbt/tests/prop_client_connection.rs:652-720`: 少数パート
- `src/websocket_connection_shared.rs:400-403`: `max_message_size` 超過でエラー

## 設計方針

- `max_message_size = 100`（または 128）程度に設定
- 10 バイト程度の Text フラグメントを N 個送り、合計が上限超過 → `Error::protocol_violation("message too large")`（`ErrorKind::ProtocolViolation`）が返ること（`src/websocket_connection_shared.rs:400-403` の実装に対応）
- N は proptest で 15–30 程度（上限到達を確実に）

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] 大量フラグメントによる max_message_size 到達を PBT で検証する
  - @<GitHub ハンドル>
```

## 完了条件

- 上記シナリオの PBT が `prop_client_connection.rs` にある
- `cargo test --workspace` が全件パスする
