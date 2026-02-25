# 0003: Sec-WebSocket-Extensions の解析が quoted-string 内の , / ; を誤分割する

## 種別

バグ (pending)

## 優先度

P2

## 概要

`Sec-WebSocket-Extensions` ヘッダーの解析で `,` による単純分割を行っており、
`quoted-string` 内に `,` や `;` が含まれる正当な拡張値を誤って不正扱いする可能性がある。

## 該当コード

- `src/websocket_handshake.rs:363` — `,` で単純分割（リクエスト解析）
- `src/websocket_handshake.rs:562` — `,` で単純分割（レスポンス解析）
- `src/websocket_handshake.rs:621` — `;` で単純分割（`validate_extension_entry`）

## RFC 根拠

RFC 6455 Section 9.1 ABNF:

```
extension-list  = 1#extension
extension       = extension-token *( ";" extension-param )
extension-param = token [ "=" ( token / ( DQUOTE *QDTEXT DQUOTE ) ) ]
```

`extension-param` の値として `quoted-string` が許容されており、
その中に `,` や `;` を含む可能性がある。
`1#extension` は RFC 7230 の `#rule` (list rule) であり、
quoted-string を考慮した stateful な解析が必要。

## 現在の挙動

```rust
.flat_map(|v| v.split(','))  // quoted-string 内の , も分割してしまう
```

`validate_extension_entry` 内では quoted-string パーサーを実装済みだが、
上流の `,` 分割が先に quoted-string を壊す。矛盾した構造になっている。

## 実害の評価

現時点で広く使われる `permessage-deflate` (RFC 7692) のパラメータはすべて token であり、
quoted-string は使用しない。実害は限定的。

ただし RFC 準拠の観点では不整合であり、将来の拡張実装で問題になり得る。

## 保留理由

RFC 7230 `#rule` に準拠した stateful パーサーの実装が必要で、コストが高い。
permessage-deflate 以外の拡張をサポートする時点で対応する。

## 修正方針

`,` 単純分割を廃止し、RFC 7230 Section 7 の list-rule に準拠した
quoted-string を考慮した extension-list パーサーを実装する。

参考: RFC 7230 Section 7 "ABNF List Extension: #rule"
