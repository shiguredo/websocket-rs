# 0004: Sec-WebSocket-Version 不一致時に RFC 必須ヘッダーを返せない

## 問題

RFC 6455 Section 4.4:

> If the server doesn't support the requested version, it MUST respond with a
> Sec-WebSocket-Version header field (or multiple Sec-WebSocket-Version header
> fields) containing all versions it is willing to use.

`reject_handshake(status_code, reason)` は追加ヘッダーを渡す手段がないため、
バージョン不一致時でも `Sec-WebSocket-Version: 13` を返せない。

## 再現手順

```rust
// クライアントが Sec-WebSocket-Version: 8 を送信した場合
// サーバーは Sec-WebSocket-Version: 13 を含む 426 レスポンスを返す MUST
conn.reject_handshake(426, "Upgrade Required")?;
// ↑ Sec-WebSocket-Version ヘッダーを付加する手段がない
```

## 対応方針

`reject_handshake()` に `headers: &[(&str, &str)]` パラメータを追加する。

```rust
pub fn reject_handshake(
    &mut self,
    status_code: u16,
    reason: &str,
    headers: &[(&str, &str)],
) -> Result<(), Error>
```

これにより呼び出し側でバージョン不一致時に:

```rust
conn.reject_handshake(426, "Upgrade Required", &[("Sec-WebSocket-Version", "13")])?;
```

と記述できる。

## 修正内容

- `reject_handshake(status_code, reason)` → `reject_handshake(status_code, reason, headers)` にシグネチャを変更
- `src/websocket_server_connection.rs`: `reject_handshake()` に `headers: &[(&str, &str)]` を追加
- `pbt/tests/prop_server_connection.rs`: 既存の呼び出しに `&[]` を追加、headers 付きのテストを追加
