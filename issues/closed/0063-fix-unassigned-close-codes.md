# 未登録のクローズコード 1016-1999 を送信不可にする

- Priority: Low
- Created: 2026-11-27
- Polished: 2026-11-27
- Completed: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-unassigned-close-codes

## 目的

`CloseCode::is_sendable` は 2000-2999 を送信不可としているが、同じ予約範囲である 1016-1999 は送信可能のままである。RFC 6455 Section 7.4.2 は 1000-2999 を「このプロトコルと将来の拡張が定義するための予約」と定めており、IANA の WebSocket Close Code Number registry で 1016-2999 は unassigned である。未登録のコードを送れる状態は、予約範囲の誤用である。

## 優先度根拠

Low。次の 2 つの挙動が変わる。

- 高レベル API の `close()` は 1016-1999 を渡すと `ErrorKind::InvalidInput` を返すようになる。
- ピアから 1016-1999 の Close を受信したとき、`handle_close` の返送コードは受信コードのエコーから 1000 (normal closure) に変わる。`is_valid` は変えないため、受信自体は従来どおり成功する。

低レベル API の `Frame::close` の引数検証は 0064 の範囲であり、本 issue では扱わない。

## 現状

- `src/websocket_close.rs` の `CloseCode::is_sendable` は 0-999、1004、1005、1006、1015、2000-2999、5000 以上を拒否する。
- 1016-1999 は拒否条件に含まれておらず、送信可能と判定される。
- 受信側の `CloseCode::is_valid` は 1000-1004、1007-1014、1016-4999 を許容しており、こちらは意図的な緩和である。
- `pbt/tests/prop_websocket_close.rs` の `prop_reserved_range_1016_1999` は 1016-1999 について `is_sendable() == true` を断言しており、変更すると失敗する。
- 同じファイルの `VALID_BUT_UNSENDABLE` は 1004 と 2000-2999 だけを含み、`prop_valid_but_unsendable_close_code_responds_with_normal_closure` の対象に 1016-1999 が入っていない。
- `src/websocket_close.rs` の `is_sendable` の doc comment は拒否範囲として 2000-2999 だけを挙げており、`skills/shiguredo-websocket/SKILL.md` の `is_sendable()` と RFC 準拠メモも同じ拒否リストになっている。

## 設計方針

`CloseCode::is_sendable` の許可範囲を 1000-1003、1007-1014、3000-4999 に限定する。すなわち 1016-1999 を送信不可に加える。`CloseCode::is_valid` は受信互換のため変更しない。

- `handle_close` の返送コードは従来どおり `is_sendable` で判定し、1016-1999 受信時は 1000 にフォールバックする。
- `is_sendable` の doc comment に 1016-1999 を加え、RFC 6455 Section 7.4.2 と IANA 未登録を根拠として記載する。
- `skills/shiguredo-websocket/SKILL.md` の `is_sendable()` の拒否範囲と RFC 準拠メモを更新する。
- 既存 PBT の `prop_reserved_range_1016_1999` を「受信は有効、送信は不可」に更新する。

CHANGES.md の種別は `[FIX]` とする。未登録コードの送信は予約範囲の誤用であり、`is_sendable` が 2000-2999 を既に拒否していることとの一貫性を欠くバグである。公開 API の受理範囲が狭まる点は優先度根拠と完了条件に明記する。

## CHANGES.md 登録内容

```markdown
- [FIX] 未登録のクローズコード 1016-1999 を送信不可にする
  - @voluntas
```

## 完了条件

- 1016、1100、1999 が `is_sendable() == false`、`is_valid() == true` になる。
- 1000、1011、1012、3000、4999 が送信可能のままである。
- `pbt/tests/prop_websocket_close.rs` の `prop_reserved_range_1016_1999` が `is_valid() == true` / `is_sendable() == false` を検証するよう更新されている。
- `VALID_BUT_UNSENDABLE` に 1016、1100、1999 を追加し、`prop_valid_but_unsendable_close_code_responds_with_normal_closure` が 1016-1999 受信時の返送コード 1000 を検証する。
- 1003/1004、1006/1007、1014/1015、1015/1016、1999/2000、2999/3000、4999/5000 の境界を、`tests/test_websocket_close.rs` の単体テストで決定的に検証する。
- `is_sendable` の doc comment と `skills/shiguredo-websocket/SKILL.md` の拒否範囲が更新されている。
- `make test`（実体は `cargo test --workspace --exclude websocket_client`）が通る。nginx e2e テストは Docker が必要なため GitHub Actions で実行する。

## 解決方法

`src/websocket_close.rs` の `CloseCode::is_sendable` を 1000-1003、1007-1014、3000-4999 に限定し、1016-1999 を送信不可にした。`is_valid` は受信互換のため変更していない。

`pbt/tests/prop_websocket_close.rs` の `prop_reserved_range_1016_1999` を `is_valid() == true` / `is_sendable() == false` に更新し、`VALID_BUT_UNSENDABLE` に 1016、1100、1999 を追加した。`tests/test_websocket_close.rs` に送信・受信の境界テストを追加した。

`skills/shiguredo-websocket/SKILL.md` の `is_sendable()` の拒否範囲と RFC 準拠メモを更新した。

`examples/websocket_reverse_proxy` の Close 転送で `is_sendable()` を通して 1000 に正規化し、受信した 1016-1999 を転送時に `InvalidInput` にしないようにした。`tests/test_websocket_connection_shared.rs` に `close()` が送信不可コードを `InvalidInput` で拒否するテストを追加した。

`make test` が通る (26 個のテストターゲットすべて ok)。
