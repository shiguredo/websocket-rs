# 0021: 圧縮時に空 DEFLATE ブロックの確認/追加を行っていない

- Created: 2026-05-14
- Completed: 2026-05-26
- Model: deepseek-v4-flash

## 優先度

P2

## 概要

`Compressor::compress()` は DEFLATE 圧縮後に末尾の `0x00 0x00 0xFF 0xFF` (4 オクテット) を
除去しているが、RFC 7692 Section 7.2.1 Step 2 で要求されている
「出力が空 DEFLATE ブロックで終わっていない場合は追加する」処理を行っていない。

現在は `noflate::Encoder::sync_flush()` が常に空 DEFLATE ブロックを出力する前提だが、
実装依存であり、将来の noflate の変更で trailer が出力されなかった場合に
不正な payload を送信する可能性がある。

## 該当コード

`src/deflate/permessage_deflate.rs:70-72`:

```rust
// RFC 7692: 終端の 0x00 0x00 0xFF 0xFF を除去
if out.ends_with(&DEFLATE_TRAILER) {
    out.truncate(out.len() - 4);
}
```

`DEFLATE_TRAILER` は `[0x00, 0x00, 0xFF, 0xFF]` と定義されている。これは
DEFLATE 空ブロックの LEN + NLEN 部分のみであり、BFINAL/BTYPE ヘッダ (`0x00`) を含まない。

## RFC 根拠

RFC 7692 Section 7.2.1:

> 2. If the resulting data does not end with an empty DEFLATE block with no
>    compression (the "BTYPE" bits are set to 00), append an empty DEFLATE
>    block with no compression to the tail end.
> 3. Remove 4 octets (that are 0x00 0x00 0xff 0xff) from the tail end。
>    After this step, the last octet of the compressed data contains
>    (possibly part of) the DEFLATE header bits with the "BTYPE" bits set
>    to 00.

Step 2 で要求される空 DEFLATE ブロックは BFINAL=0, BTYPE=00 の stored block (LEN=0)
である。RFC 1951 Section 3.2.4 (Non-compressed blocks) に定義されており、
バイト境界から開始する場合、以下の最大 5 オクテットで構成される
（直前ブロック終端がバイト境界でない場合、BFINAL/BTYPE は先行ブロック最終バイトに
パックされるが、`sync_flush()` がバイト境界を保証するため本実装では 5 オクテット固定とみなせる）:

```
0x00        (BFINAL=0, BTYPE=00)
0x00 0x00   (LEN = 0)
0xFF 0xFF   (NLEN = ~LEN)
```

## 修正方針

`compress()` 内で、DEFLATE 空ブロック全体（5 オクテット）が存在しない場合のみ
追加した上でトランジションを行う:

```rust
const DEFLATE_EMPTY_BLOCK_HEADER: u8 = 0x00;

if !out.ends_with(&DEFLATE_TRAILER) {
    // RFC 7692 Section 7.2.1 Step 2: 空 DEFLATE ブロックを追加
    out.push(DEFLATE_EMPTY_BLOCK_HEADER);
    out.extend_from_slice(&DEFLATE_TRAILER);
}
// RFC 7692 Section 7.2.1 Step 3: 末尾 4 オクテットを除去
out.truncate(out.len() - 4);
```

修正の要点:

- `DEFLATE_TRAILER` は LEN + NLEN の 4 オクテットのみ。空 DEFLATE ブロック全体は
  ヘッダ `0x00` を含む 5 オクテット。この 5 オクテットを追加してから末尾 4 オクテットを
  除去することで、BTYPE=00 のマーカが出力末尾に残る
- trailer が既に存在する場合、`!out.ends_with(&DEFLATE_TRAILER)` が false のため
  追加は発生せず、除去のみ実行される（現在と同じ動作）
- `out.truncate(out.len() - 4)` にガードは不要。`out.ends_with` が true なら
  `out.len() >= 4` が保証され、false でも trailer 追加後のため同様

## テスト戦略

### PBT (`pbt/tests/prop_permessage_deflate.rs`)

以下のプロパティを追加する:

1. 任意入力に対し `compress()` の出力末尾バイトの下位 2 ビットが BTYPE=00 であること
   （出力が空でない場合。proptest strategy で生成した任意データを与えて検証する）

既存のラウンドトリップ PBT は修正後も通過する。

### 単体テスト (`tests/test_permessage_deflate.rs`)

PBT でカバーできないエラーパスをテストする:

1. Encoder の feed / sync_flush がエラーを返すケース（noflate のエラー伝播確認）

### Fuzzing

既存の fuzz ターゲットでパニック耐性を確認する（修正前後でパニックが発生しないこと）。

## CHANGES.md 登録内容

- [FIX] RFC 7692 Section 7.2.1 に従い、空 DEFLATE ブロックを必要に応じて追加してから末尾 4 オクテットを除去する
  - @実装者名

## 解決方法

対応不要としてクローズ。

noflate の `sync_flush()` は DEFLATE 仕様上、常に空 stored ブロック
(BFINAL=0, BTYPE=00, LEN=0, NLEN=0xFFFF) を末尾に出力する。
これは sync flush の定義そのものであり、実装依存ではない。
noflate のソースコード (`encode.rs` の `sync_flush` → `emit_stored_chunk(&[], false)`)
およびテスト (`sync_flush_marker_is_empty_stored_block`) で確認済み。

したがって、RFC 7692 Section 7.2.1 Step 1 の「空 DEFLATE ブロックで終わっていなければ
追加する」条件は `sync_flush()` 使用時に常に満たされており、明示的なチェック/追加コードは
不要。提案された修正は到達不可能なコードパスを追加するだけであり、CLAUDE.md の
「到達不可能なコード → デッドコードとして削除する」方針に反する。
