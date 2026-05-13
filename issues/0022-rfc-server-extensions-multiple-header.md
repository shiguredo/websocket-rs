# 0022: サーバーレスポンスで複数行の Sec-WebSocket-Extensions を誤って拒否する

Created: 2026-05-14
Model: deepseek-v4-flash

## 優先度

P1

## 概要

`HandshakeValidator::validate_response()` はサーバーからの HTTP レスポンスに
`Sec-WebSocket-Extensions` ヘッダーが 2 行以上含まれている場合に
エラーとして拒否する。RFC 6455 Section 11.3.2 は拡張の複数行分割を
明示的に許容しており、これは RFC 違反である。

リクエスト側 (`HandshakeRequestValidator`) は複数行を正しくマージして処理している。

## 該当コード

`src/websocket_handshake.rs:609-615`:

```rust
// RFC 6455 Section 11.3.2: HTTP レスポンスでは Sec-WebSocket-Extensions は 1 回のみ許容
let extension_values = response.get_headers("Sec-WebSocket-Extensions");
if extension_values.len() > 1 {
    return Err(Error::handshake_rejected(
        "duplicate Sec-WebSocket-Extensions header in HTTP response",
    ));
}
```

## RFC 根拠

RFC 6455 Section 11.3.2 (refs/rfc6455.txt:1352-1356):

> If multiple extensions are to be used, they can all be listed in a single
> |Sec-WebSocket-Extensions| header field or split between multiple instances
> of the |Sec-WebSocket-Extensions| header field.

RFC 9110 Section 5.3: 同名ヘッダーが複数行の場合はリスト値として統合する。

## 現状のリクエスト側実装

`validate_request` (line 410) では正しくマージして処理している:

```rust
let extension_values = request.get_headers("Sec-WebSocket-Extensions");
// RFC 9110 Section 5.3: 同名ヘッダーが複数行の場合はリスト値として統合する
let extensions: Vec<String> = values
    .iter()
    .flat_map(|v| v.split(','))
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .collect();
```

## 修正方針

レスポンス側もリクエスト側と同様に複数行をマージして処理する:

1. `if extension_values.len() > 1` の拒否チェックを削除する
2. `.iter().flat_map(|v| v.split(',')).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()` の形でマージする (リクエスト側と同一ロジック)
3. ただし複数の拡張に ABNF 違反があった場合の `validate_extension_entry` による検証は維持する
