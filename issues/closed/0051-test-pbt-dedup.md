# PBT テストの重複を解消する

- Priority: Medium
- Created: 2026-05-27
- Model: opencode mimo-v2.5-pro

## 目的

PBT テストファイル間でテスト内容が重複している。解消する。

## 優先度根拠

テストの重複は保守性を損なう。RFC セクション対応テストは `prop_rfc6455.rs` に集約し、各モジュールの PBT はモジュール固有のテストに絞るべき。

## 現状

- `pbt/tests/prop_frame.rs` と `pbt/tests/prop_rfc6455.rs` のテスト重複
- `pbt/tests/prop_handshake.rs` と `pbt/tests/prop_rfc6455.rs` のテスト重複

## 設計方針

RFC セクション対応テストは `prop_rfc6455.rs` に集約し、各モジュールの PBT はモジュール固有のテストに絞る。

## 完了条件

- テスト重複が解消されている
- RFC セクション対応テストが `prop_rfc6455.rs` に集約されている
- `cargo test --all` が通過する

## 解決方法

1. `prop_frame.rs` の RFC 準拠テストを `prop_rfc6455.rs` に統合する
2. `prop_frame.rs` は純粋な Frame 型のラウンドトリップテストのみに絞る
3. `prop_handshake.rs` の RFC セクション対応テストを `prop_rfc6455.rs` に統合する
4. `prop_handshake.rs` はビルダーパターンと Extension パースのテストに絞る
