# 0002: additional_headers で予約済みヘッダーを重複生成できる [CLOSED]

## 種別

バグ

## 優先度

P3

## 概要

`HandshakeRequestBuilder` および `HandshakeResponse` の `additional_headers` に予約済みヘッダー名を渡しても拒否されない。
RFC 6455 で "MUST appear" かつ "MUST NOT appear more than once" とされるヘッダーが重複したリクエスト/レスポンスを生成できてしまう。

## 該当コード

- `src/websocket_handshake.rs:114` (クライアント側 additional_headers 適用)
- `src/websocket_server_connection.rs:333` (サーバー側 additional_headers 適用)

## RFC 根拠

RFC 6455 Section 4.1 (クライアントリクエスト):

> The request MUST contain a |Sec-WebSocket-Key| header field ...
> The request MUST contain a |Sec-WebSocket-Version| header field ...

RFC 6455 Section 4.2.2 (サーバーレスポンス):

> The handshake MUST contain a |Sec-WebSocket-Accept| header field ...

RFC 6455 Section 11.3 (IANA 登録ヘッダー群) は各ヘッダーが "appear only once" であることを前提とする。

## 現在の挙動

```rust
for (name, value) in &self.additional_headers {
    request = request.header(name, value); // 重複チェックなし
}
```

以下のコードが通ってしまう:

```rust
builder.additional_header("Sec-WebSocket-Key", "AAAAAAAAAAAAAAAAAAAAAA==")
```

## 期待する挙動

予約済みヘッダー名が `additional_headers` に含まれている場合、`build()` 時に `Err` を返す。

予約済みヘッダー（クライアント）:
- `Host`
- `Upgrade`
- `Connection`
- `Sec-WebSocket-Key`
- `Sec-WebSocket-Version`

予約済みヘッダー（サーバー）:
- `Upgrade`
- `Connection`
- `Sec-WebSocket-Accept`
- `Sec-WebSocket-Protocol`
- `Sec-WebSocket-Extensions`

## 修正方針

`build()` / `accept()` 内で `additional_headers` を適用する前に予約済みヘッダー名と突合し、
重複が見つかった場合は `Err(Error::invalid_input(...))` を返す。

## 修正内容

- `src/websocket_handshake.rs`: `HandshakeRequest::build()` に予約済みヘッダーセット `RESERVED` を定義し、`additional_headers` 適用前に突合チェックを追加
- `src/websocket_server_connection.rs`: `accept_handshake()` に同様のチェックを追加
- `pbt/tests/prop_handshake.rs`: `prop_build_rejects_reserved_header` を追加
- `pbt/tests/prop_server_connection.rs`: `prop_accept_handshake_rejects_reserved_header` を追加
