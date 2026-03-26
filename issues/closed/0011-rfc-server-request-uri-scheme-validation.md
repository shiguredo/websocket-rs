# 0011-rfc-server-request-uri-scheme-validation

## 概要

サーバー側 `HandshakeRequestValidator::validate_request()` において、
absolute-form の Request-URI に `ws://` や `wss://` などの非 http/https スキームが
使われた場合を拒否していない。

## RFC 根拠

RFC 6455 Section 4.2.1 (line 1143-1146):

> An HTTP/1.1 or higher GET request, including a "Request-URI" that should be
> interpreted as a /resource name/ defined in Section 3 (or an absolute
> HTTP/HTTPS URI containing the /resource name/).

RFC 6455 Section 4.1 (line 939-942):

> The "Request-URI" part of the request MUST match the /resource name/ or
> be an absolute http/https URI

RFC 6455 Section 4.2.1 冒頭 (line 1138-1141):

> the server MUST stop processing the client's handshake and return an
> HTTP response with an appropriate error code (such as 400 Bad Request)

## 現状

HTTP デコーダー (`shiguredo_http11`) の `parse_request_target_form` +
`validate_request_target_for_method` により以下は担保済み:

- origin-form (`/` 始まり) の構文検証
- GET メソッドに対して authority-form / asterisk-form を拒否
- `http`/`https` absolute-form の追加検証 (userinfo 禁止, 空 host 禁止)

## 残差

absolute-form として `ws://example.com/chat` や `wss://example.com/chat` などが
渡された場合、HTTP デコーダーはスキーム文字種のみを検証して通過させてしまう。
RFC 6455 は absolute URI として `http/https` のみを許可するが、WebSocket 層での
スキーム制限がない。

## 対応方針

`validate_request()` 内で `request.uri` に対して:

- `"/"` で始まる → origin-form、問題なし
- `"http://"` または `"https://"` で始まる → absolute-form、問題なし
- それ以外の absolute-form (スキームあり) → `Error::handshake_rejected`

## 解決方法

`src/websocket_handshake.rs` の `validate_request()` 内、HTTP バージョン検証の
直後に URI スキーム検証を追加した。

`request.uri` が `"/"` 始まりでない場合に、`http://` または `https://` で
始まるかをチェックし、どちらでもなければ `Error::handshake_rejected` を返す。
大文字小文字を吸収するため `to_ascii_lowercase()` で比較する。
