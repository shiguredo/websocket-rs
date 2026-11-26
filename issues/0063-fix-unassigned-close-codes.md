# 未登録のクローズコード 1016-1999 を送信不可にする

- Priority: Low
- Created: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-unassigned-close-codes

## 目的

`CloseCode::is_sendable` は 2000-2999 を送信不可としているが、同じ予約範囲である 1016-1999 は送信可能のままである。RFC 6455 Section 7.4.2 は 1000-2999 を「このプロトコルと将来の拡張が定義するための予約」と定めており、IANA の WebSocket Close Code Number registry で 1016-2999 は unassigned である。

## 優先度根拠

Low。未登録コードを送るアプリケーションを止められないが、受信側の挙動には影響しない。

## 現状

- `src/websocket_close.rs` の `CloseCode::is_sendable` は 0-999、1004、1005、1006、1015、2000-2999、5000 以上を拒否する。
- 1016-1999 は拒否条件に含まれておらず、送信可能と判定される。
- 受信側の `CloseCode::is_valid` は 1016-2999 を許容しており、こちらは意図的な緩和である。

## 設計方針

`CloseCode::is_sendable` を、1000-1003、1007-1014、3000-4999 に限定する。`CloseCode::is_valid` は受信互換のため変更しない。

## CHANGES.md 登録内容

```markdown
- [CHANGE] 未登録のクローズコード 1016-1999 を送信不可にする
  - @voluntas
```

## 完了条件

- 1016 と 1100 が `is_sendable() == false` になる。
- 1000、1011、1012、3000、4999 が送信可能のままである。
- 境界値を含む PBT が通る。
- `cargo test --workspace` が通る。
