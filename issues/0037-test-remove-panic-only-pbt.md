# PBT でパニックしないことだけを検証するテストを削除する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: mimo-v2.5-pro
- Branch: feature/refactor-remove-panic-only-pbt

## 目的

「任意入力でパニックしないことだけ」を検証する PBT を削除する。パニック安全性の検証は fuzzing の役割であり PBT には書かない（AGENTS.md 規約）。

## 優先度根拠

Medium。恒真式アサーションや状態取得のみのテストは PBT の価値が低く、fuzz の方が広いパスをカバーする。

## 現状

削除対象（5 関数）:

| ファイル | 関数 |
|---|---|
| `pbt/tests/prop_client_connection.rs` | `prop_random_bytes_handling`, `prop_random_bytes_during_handshake` |
| `pbt/tests/prop_server_connection.rs` | `prop_random_bytes_handling`, `prop_random_bytes_during_handshake` |
| `pbt/tests/prop_violations.rs` | `prop_random_bytes_handling`（`prop_assert!(result.is_ok() \|\| result.is_err())` は恒真） |

代替 fuzz: `fuzz/fuzz_targets/fuzz_client_connection.rs`, `fuzz_server_connection.rs`, `fuzz_frame_decode.rs`（`poll_*` ドレイン・サイズ制限・複数フレーム処理を含む）。

## 設計方針

上記 5 テスト関数と、それ専用の `use` / `mod` コメントブロックが空になる場合は整理して削除。他のプロパティテストは残す。

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] パニックのみ検証する PBT を削除し fuzzing に役割を寄せる
  - @<GitHub ハンドル>
```

## 完了条件

- 5 関数が削除されている
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある
