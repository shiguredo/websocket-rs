# 0020: close_internal の reason 切り詰めが UTF-8 境界を無視してパニックする可能性がある

Created: 2026-05-14
Model: deepseek-v4-flash

## 優先度

P1

## 概要

`close_internal` で Close フレームの reason を 123 バイトに切り詰める際に
`&reason[..123]` を使用している。Rust の `&str` スライシングは
バイトインデックスが UTF-8 文字境界でない場合にパニックする。

現在の全呼び出し元は短い ASCII 文字列 (`"frame decode error"`、`"masked server frame"` 等) のため
発現しないが、将来の変更でマルチバイト文字を含む reason が渡された場合に
回復不能なパニックが発生する。

## 該当コード

`src/websocket_client_connection.rs:641-643`:

```rust
let truncated_reason = if reason.len() > 123 {
    &reason[..123]  // ← UTF-8 文字の途中だとパニック
} else {
    reason
};
```

`src/websocket_server_connection.rs:691-693` (同様):

```rust
let truncated_reason = if reason.len() > 123 {
    &reason[..123]  // ← 同様
} else {
    reason
};
```

## RFC 根拠

RFC 6455 Section 5.5.1: Close フレームの reason は UTF-8 でエンコードされ、
ペイロード全体 (コード 2 バイト + reason) が 125 バイト以下でなければならない。
reason 部分の制限は 123 バイト。

## 修正方針

`reason.char_indices()` を用いて UTF-8 セーフな切り詰めを行う:

```rust
let truncated_reason = if reason.len() > 123 {
    let mut end = 123;
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    &reason[..end]
} else {
    reason
};
```

client/server 両方の `close_internal` を修正する。
