# 0001: Connecting 状態で close() が Close フレームを送信できる [CLOSED]

## 種別

バグ

## 優先度

P1

## 概要

`close()` メソッドが `Connecting` 状態でも呼び出し可能で、WebSocket Close フレームが出力される。
Close フレームは opening handshake 完了後の established connection 上でのみ送信可能であり、RFC 6455 違反となる。

## 該当コード

- `src/websocket_client_connection.rs:485`
- `src/websocket_server_connection.rs:526`

## RFC 根拠

RFC 6455 Section 1.4:

> Either peer can send a control frame with data containing a specified
> control sequence to begin the closing handshake.

Section 5.5 (Control Frames):

> The application MUST NOT send any more data frames after sending a
> Close frame.

Close フレームを送れるのは connection が established された後に限られる。
`Connecting` 中は WebSocket フレームを一切送信すべきでない。

## 現在の挙動

```rust
if self.state == ConnectionState::Disconnected || self.state == ConnectionState::Closed {
    return Err(Error::invalid_state("connection is already closed"));
}
// Connecting 状態でも以降の処理が続く
```

## 期待する挙動

`Connecting` 状態でも `close()` は `Err` を返す。

```rust
if !matches!(self.state, ConnectionState::Open | ConnectionState::Closing) {
    return Err(Error::invalid_state("connection is not established"));
}
```

## 修正方針

`close()` の状態ガードを「Open または Closing 以外は拒否」に変更する。
クライアント・サーバー双方の実装を修正する。

## 再現手順

1. `WebSocketClientConnection` を作成する（まだ `feed_bytes` を呼ばない）
2. `close(CloseCode::Normal, "")` を呼ぶ
3. 現状: `Ok(())` が返り Close フレームが出力キューに積まれる
4. 期待: `Err` が返る

## 修正内容

- `src/websocket_client_connection.rs`: `close()` のガード条件を `Connected | Closing` 以外は拒否に変更
- `src/websocket_server_connection.rs`: 同上
- `pbt/tests/prop_client_connection.rs`: `prop_close_rejected_in_connecting_state` を追加
- `pbt/tests/prop_server_connection.rs`: `prop_close_rejected_in_connecting_state` / `prop_close_rejected_in_disconnected_state` を追加
