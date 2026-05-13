# 0021: 圧縮時に空 DEFLATE ブロックの確認/追加を行っていない

Created: 2026-05-14
Model: deepseek-v4-flash

## 優先度

P1

## 概要

`Compressor::compress()` は DEFLATE 圧縮後に末尾の `0x00 0x00 0xFF 0xFF` (空 DEFLATE ブロック)
を除去しているが、RFC 7692 Section 7.2.1 Step 2-3 で要求されている
「出力が空 DEFLATE ブロックで終わっていない場合は追加する」処理を行っていない。

現在は `noflate::Encoder::sync_flush()` が常に空 DEFLATE ブロックを出力する前提だが、
実装依存であり、将来の noflate の変更や異なる実装で trailer が出力されなかった場合に
不正な payload を送信する可能性がある。

## 該当コード

`src/deflate/permessage_deflate.rs:70-72`:

```rust
// RFC 7692: 終端の 0x00 0x00 0xFF 0xFF を除去
if out.ends_with(&DEFLATE_TRAILER) {
    out.truncate(out.len() - 4);
}
```

## RFC 根拠

RFC 7692 Section 7.2.1 (refs/rfc7692.txt:1036-1043):

> 2. If the resulting data does not end with an empty DEFLATE block with no
>    compression (the "BTYPE" bits are set to 00), append an empty DEFLATE
>    block with no compression to the tail end.
> 3. Remove 4 octets (that are 0x00 0x00 0xff 0xff) from the tail end.

ステップ 2 と 3 により、以下の処理が必要:
- 出力が空 DEFLATE ブロックで終わっていない場合は事前に追加する
- その上で末尾 4 オクテットを除去する

## 修正方針

`compress()` 内で、trailer の有無を確認し、存在しない場合は追加してから除去する:

```rust
if !out.ends_with(&DEFLATE_TRAILER) {
    out.extend_from_slice(&DEFLATE_TRAILER);
}
out.truncate(out.len() - 4);
```
