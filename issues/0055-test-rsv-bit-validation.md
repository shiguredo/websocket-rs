# prop_violations.rs に RSV ビット検証を追加する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
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
