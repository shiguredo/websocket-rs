# 0019: WebSocketClientConnection と WebSocketServerConnection に send_pong API を追加する

Created: 2026-04-28
Completed: 2026-04-28
Model: Claude Opus 4.7

## 種別

feature

## 優先度

P3

## 概要

`WebSocketClientConnection` および `WebSocketServerConnection` に
`send_pong(&mut self, payload: &[u8])` API を追加する。

## 根拠

- RFC 6455 §5.5.3 (Pong) では unsolicited Pong フレームの送信が明示的に許可されている。
  > A Pong frame MAY be sent unsolicited. This serves as a unidirectional heartbeat.
- 現状のクレートは `send_ping` のみ提供し、unsolicited Pong を送る手段がない。
- 標準 API としての一貫性が欠けている (ping だけ送れて pong だけ送れない)。
- 後続で追加する `examples/wscat` の `--slash` モード (`/pong [data]` コマンド) を
  完全互換で実装するために必要。
- RFC 6455 は今後改訂される可能性があり、その場合 API の挙動も見直す前提とする。

## 方針

### API

```rust
// RFC 6455 §5.5.3 (Pong フレーム): unsolicited Pong フレームの送信は許可されており
// 一方向のハートビートとして使える。本 API はそのために提供する。
// 注: RFC 6455 は今後改訂される可能性があり、その場合本 API の挙動も見直す。
pub fn send_pong(&mut self, payload: &[u8]) -> Result<(), Error>;
```

### 実装

既存の `send_ping` と同形のロジックで `Opcode::Pong` を使う。

- `WebSocketClientConnection::send_pong`
- `WebSocketServerConnection::send_pong`

両者で内部処理を共有しているなら共通関数の Opcode 引数を Pong にするだけで済む。

### ペイロード制約

RFC 6455 §5.5 により、コントロールフレームのペイロードは 125 バイト以下。
`send_ping` 側が既にこの制約を扱っているなら同じ機構をそのまま使う。

## 影響

- `[ADD]` 後方互換あり、新規 API の追加。
- 既存ユーザーへの影響なし。

## テスト

- `WebSocketClientConnection::send_pong` 呼び出し後の `poll_output` で生成される
  フレームのオペコードが Pong (0xA) になることを確認する単体テスト。
- 既存の PBT (もしあれば) のラウンドトリップに送信側 Pong を含める。
- ペイロード長境界 (0 バイト、125 バイト、126 バイト) のエラー有無を確認。

## 参考資料

- RFC 6455 §5.5.3 Pong: https://datatracker.ietf.org/doc/html/rfc6455#section-5.5.3

## 解決方法

- `src/websocket_client_connection.rs` および `src/websocket_server_connection.rs` の
  `send_ping` 直後に `send_pong(&mut self, data: &[u8]) -> Result<(), Error>` を追加した
  - 内部で `Frame::pong(data.to_vec())?` (既存のファクトリ) を使用し、`send_frame` で送出
  - ペイロード長 125 バイト超過チェックは `Frame::pong` 側で実施
  - `send_ping` と異なり `awaiting_pong` や `PongTimeout` の更新は行わない (unsolicited Pong は応答待ちを伴わない)
  - コードコメントに RFC 6455 §5.5.3 の根拠と「将来改訂される可能性」を明記 (AGENTS.md 準拠)
- `pbt/tests/prop_client_connection.rs` に `prop_send_pong_emits_pong_frame` を追加
  - `send_pong` で送信されたバイト列の先頭が `FIN=1, RSV=0, Opcode=Pong, MASK=1, Length=data.len()` になることを検証
- `pbt/tests/prop_server_connection.rs` に同名 PBT を追加
  - サーバー側はマスクなしでヘッダ後にペイロードがそのまま続くことを検証
  - `Opcode` を import に追加
- `CHANGES.md` の `## develop` に `[ADD]` エントリを追記
