# prop_close_code.rs に送信禁止コードの受信時テストを追加する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/add-close-code-receive-pbt

## 目的

受信 Close フレームに含まれる reserved / 送信禁止コードに対する `handle_close` の返送動作を PBT で検証する（RFC 6455 Section 7.4.1）。

## 優先度根拠

Medium。`prop_close_code.rs` は `is_valid` / `is_sendable` の論理のみ。接続統合パスが未カバー。

## 現状

- `src/websocket_connection_shared.rs:471-481`: `!is_valid()`（1005, 1006, 1015 等）を受信した場合、`close_internal(PROTOCOL_ERROR, ...)` で返送しつつ `Err(Error::protocol_violation(...))` を返す
- `src/websocket_connection_shared.rs:506-516`: 受信コードが `is_valid() && !is_sendable()`（1004, 2000-2999 等）の場合、返送する Close フレームのコードを 1000（Normal Closure）にフォールバックする
- `prop_close_code.rs`: 型の論理プロパティのみで、上記 2 つの接続層パスは未カバー

## 設計方針

PBT で次を検証:

- `!is_valid()`（1005, 1006, 1015 等）→ プロトコルエラー
- `is_valid() && !is_sendable()`（1004, 2000-2999 等）→ 1000 で返送（既存挙動をコードから読み取り固定）

フレームは手動エンコード（2 バイトコード + 任意 reason）し、接続を `Connected` にしてから `feed_recv_buf`。

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] 送信禁止 Close コード受信時の返送動作を PBT で検証する
  - @<GitHub ハンドル>
```

## 完了条件

- 上記 2 系統の PBT が `prop_close_code.rs` にある
- `cargo test --workspace` が全件パスする
