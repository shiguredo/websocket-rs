# 大量フラグメント攻撃のシミュレーションテストを追加する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
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

## 解決方法

`pbt/tests/prop_client_connection.rs` 末尾に `prop_fragment_flood_exceeds_max_message_size` を追加した。

`ClientConnectionOptions::new().max_message_size(100)` で接続を作り、`setup_connected_client_with_max_message_size` ヘルパでハンドシェイク完了状態まで進める。10 バイトずつの Text + Continuation フラグメントを 15〜30 個 (proptest 生成) サーバから流し込み、累積サイズが 100 バイトを超えた時点で:

- `feed_recv_buf` が `Err(ErrorKind::ProtocolViolation)` を返す
- `poll_output` から取り出した Close フレームの code が `CloseCode::MESSAGE_TOO_BIG (1009)` である

ことを検証する。フレーム構築のため `build_unmasked_text_fragment` / `build_unmasked_continuation_fragment` をファイル内に追加。`ErrorKind` を import 文に追加。`cargo test --workspace` 全件パス。
