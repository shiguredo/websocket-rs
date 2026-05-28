# prop_violations.rs に RSV ビット検証を追加する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/add-rsv-bit-validation-pbt

## 目的

`websocket_connection_shared.rs` の RSV 違反検出（RSV2/RSV3、非 deflate 時 RSV1、制御フレーム RSV1、Continuation RSV1 等）を PBT でカバーする。

## 優先度根拠

Medium。違反検出は `294-333` 行付近に集中し、専用 violations テストで明示すべき。

## 現状

- `src/websocket_connection_shared.rs:294-333`: RSV 検証
- `pbt/tests/prop_violations.rs`: RSV 系なし
- `pbt/tests/prop_client_connection.rs:640` に `prop_rsv1_without_deflate_rejected`（deflate 無効時 RSV1 拒否）が既に存在する。本 issue ではこのケースを再度追加せず、残る違反系を `prop_violations.rs` に集約する。

## 設計方針

`prop_violations.rs` に mod セクションを追加し、次を **接続確立後** のフィードで検証（フレームバイト列を手動構築）:

| ケース | 期待 |
|---|---|
| RSV2 または RSV3 セット | プロトコルエラー |
| 制御フレームで RSV1 | エラー |
| Continuation で RSV1（非圧縮） | エラー |

「deflate 無効で RSV1」は既存 PBT （`prop_client_connection.rs:640`）でカバー済みのため本 issue では扱わない。deflate 有効時の正当 RSV1 は既存 deflate PBT に任せ、本 issue では違反のみ。

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] RSV ビット違反の検出を prop_violations で検証する
  - @<GitHub ハンドル>
```

## 完了条件

- 上記 3 系統（RSV2/RSV3、制御フレーム RSV1、Continuation RSV1）が PBT または同ファイル内の複数 `#[test]` でカバーされている（PBT で難しい固定パターンは unittest 可。AGENTS: unittest は PBT 不可のみ）
- `cargo test --workspace` が全件パスする

## 解決方法

`pbt/tests/prop_server_connection.rs` に以下 4 件の PBT を追加した:

- `prop_rsv2_rejected_at_server`: 任意の opcode + マスクキーで RSV2=1 のフレームは ProtocolViolation で拒否される (RFC 6455 Section 5.2)
- `prop_rsv3_rejected_at_server`: 同じく RSV3=1 のフレームは拒否される
- `prop_rsv1_on_control_frame_rejected_with_deflate`: permessage-deflate 合意済みでも、制御フレーム (Close / Ping / Pong) に RSV1=1 を設定すると拒否される (RFC 7692 Section 6)
- `prop_rsv1_on_continuation_frame_rejected_with_deflate`: 同じく Continuation フレームに RSV1=1 を設定すると拒否される

deflate 有効サーバ接続を作るための `setup_connected_server_with_deflate` ヘルパも追加し、`ServerConnectionOptions::new().deflate(PerMessageDeflateConfig::default())` + `Sec-WebSocket-Extensions: permessage-deflate` ハンドシェイクで合意成立させる。

issue 文書では `prop_violations.rs` に追加することを示唆しているが、`prop_violations.rs` は `FrameDecoder` 単体のテストで接続インスタンスを扱わないため、接続層 (`handle_frame` 内の RSV 検証) のテストとしては `prop_server_connection.rs` の方が責務的に適切と判断。deflate 無効時の RSV1 拒否は既存 `prop_client_connection.rs:640` の `prop_rsv1_without_deflate_rejected` でカバー済みのため重複追加しない。`cargo test --workspace` 全件パス。
