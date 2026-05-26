# 0005: permessage-deflate の window_bits で先頭ゼロを不正扱いしていない

## 問題

RFC 7692 Section 7.1.2.1, 7.1.2.2:

> a decimal integer value without leading zeroes between 8 to 15

`server_max_window_bits` / `client_max_window_bits` の値は先頭ゼロなしの十進数整数でなければならない。

現実装は `value.parse::<u8>()` のみで `"08"` や `"015"` を受理してしまう。

RFC 7692 Section 7 より、不正値を含む offer はサーバーが MUST decline し、
不正値を含む response はクライアントが MUST Fail the WebSocket Connection。

## 再現手順

```
Sec-WebSocket-Extensions: permessage-deflate; server_max_window_bits=08
```

上記の拡張ヘッダーがエラーにならず受理される。

## 対応方針

パース前に先頭ゼロチェックを追加する。

```rust
if value.starts_with('0') && value.len() > 1 {
    return Err(ExtensionParseError::InvalidValue(...));
}
```

`server_max_window_bits` と `client_max_window_bits` の両方に適用する。

## 修正内容

- `src/websocket_extension.rs`: `server_max_window_bits` と `client_max_window_bits` のパース前に先頭ゼロチェックを追加
- `pbt/tests/prop_handshake.rs`: 先頭ゼロ付き値が拒否されること、有効な値が受理されることのテストを追加
