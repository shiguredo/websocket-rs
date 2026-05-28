# 0010-rfc-client-request-uri-validation

## 概要

`HandshakeRequest::build()` が `self.path` をそのまま Request-URI に使用しており、
RFC 6455 Section 4.1 で要求される形式検証を行っていない。

## RFC 根拠

RFC 6455 Section 4.1:

> The "Request-URI" part of the request MUST match the /resource
> name/ defined in Section 3 (a relative URI) or be an absolute
> http/https URI that, when parsed, has a /resource name/, /host/,
> and /port/ that match the corresponding ws/wss URI.

RFC 6455 Section 3 (line 755-764):
- パスが空の場合は `"/"`
- それ以外はパス部分 + クエリ部分 (`"?" + query`)
- つまり `"/"` で始まる相対 URI、または絶対 `http/https` URI

## 問題箇所

`src/websocket_handshake.rs:115`

```rust
let mut request = Request::new("GET", &self.path)
```

`self.path` に対して以下の検証が行われていない:
- `"/"` で始まる相対 URI
- `"http://"` または `"https://"` で始まる絶対 URI

それ以外の値 (`"chat"`, `""`, `"ws://..."` など) を渡した場合、不正な Request-URI
が送信されてしまう。

## 対応方針

`HandshakeRequest::build()` の冒頭で `self.path` の形式検証を追加する。

- `"/"` で始まる → OK (resource name)
- `"http://"` または `"https://"` で始まる → OK (absolute URI)
- それ以外 → `Error::invalid_input`

## 解決方法

`src/websocket_handshake.rs` の `HandshakeRequest::build()` 内、key 生成の
直前に path 形式検証を追加した。

`self.path` が `"/"` 始まりでなく、かつ `http://` / `https://` 始まりでもない
場合に `Error::invalid_input` を返す。
大文字小文字を吸収するため `to_ascii_lowercase()` で比較する。
