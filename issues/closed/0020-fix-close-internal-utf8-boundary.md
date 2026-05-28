# 0020: close_internal の reason 切り詰めが UTF-8 境界を無視してパニックする可能性がある

- Priority: High
- Created: 2026-05-14
- Completed: 2026-05-26
- Polished: 2026-05-26
- Model: DeepSeek V4 Flash
- Branch: feature/fix-close-utf8-boundary

## 目的

`close_internal` で Close フレームの reason を 123 バイトに切り詰める際に
`&reason[..123]` を使用している。Rust の `&str` スライシングは
バイトインデックスが UTF-8 文字境界でない場合にパニックする。

現在の全呼び出し元は短い ASCII 文字列のため発現しないが、将来の変更で
マルチバイト文字を含む reason が渡された場合に回復不能なパニックが発生する。
本修正は将来の安全性のための予防的修正である。

また、client 側 `close_internal` の戻り値型を `Result<(), Error>` から `()` に統一する。
server 側は既に `()` を返しており、issue 0019（共通化リファクタリング）の前提条件として
client/server 間の戻り値型を揃える必要がある。戻り値型統一を別 issue にすると
0019 の前提 issue が 3 つに分かれて複雑化するため、本 issue に含める。

## 優先度根拠

High。以下の根拠から判断する。

- issue 0019（共通化リファクタリング）の前提修正であり、0019 より先に対応が必須
- `&reason[..123]` による UTF-8 境界無視パニックは、発現すれば回復不能
- 修正量は小さく、副作用リスクも低い

## 現状

### 該当コード

client 側 `src/websocket_client_connection.rs` の `close_internal`:

```rust
let truncated_reason = if reason.len() > 123 {
    &reason[..123]  // UTF-8 文字の途中だとパニック
} else {
    reason
};
```

server 側 `src/websocket_server_connection.rs` の `close_internal` にも同一コードが存在。

### client 側 `close_internal` の全呼び出し元

戻り値型 `()` への統一に伴い、全 16 箇所で `close_internal(...)?;` → `close_internal(...);` に変更する。
すべて短い ASCII リテラルまたは `format!` 経由の短い文字列であり、123 バイトを超える reason は現存しない。

| 呼び出し元メソッド     | reason                                                          | 最大バイト長 |
| ---------------------- | --------------------------------------------------------------- | ------------ |
| `process_frames`       | `"frame decode error"`                                          | 18           |
| `handle_decoded_frame` | `"masked server frame"`                                         | 19           |
| `handle_frame`         | `"frame payload too large"`                                     | 23           |
| `handle_frame`         | `"reserved bits set"`                                           | 17           |
| `handle_frame`         | `"rsv1 set without permessage-deflate"`                         | 35           |
| `handle_frame`         | `"rsv1 must not be set on control frames"`                      | 38           |
| `handle_frame`         | `"rsv1 must not be set on continuation frames"`                 | 43           |
| `handle_data_frame`    | `"new message started before previous completed"`               | 45           |
| `handle_data_frame`    | `"message too large"`                                           | 17           |
| `handle_continuation`  | `"continuation frame without initial frame"`                    | 40           |
| `handle_continuation`  | `"message too large"`                                           | 17           |
| `decompress_if_needed` | `"received compressed frame without permessage-deflate"`        | 50           |
| `emit_message`         | `"invalid UTF-8"`                                               | 13           |
| `handle_close`         | `"close frame payload length must be 0 or >= 2"`               | 45           |
| `handle_close`         | `format!("invalid close code: {}", code_val)` (最大 u16=65535) | 25           |
| `handle_close`         | `"close frame reason is not valid UTF-8"`                       | 37           |

server 側 (15 箇所) はすべて `close_internal(...);` のため変更不要（戻り値型は元々 `()`）。

### RFC 根拠

RFC 6455 Section 5.5 (refs/rfc6455.txt line 1978):

> All control frames MUST have a payload length of 125 bytes or less.

RFC 6455 Section 5.5.1 (refs/rfc6455.txt line 1989-1993):

> Following the 2-byte integer, the body MAY contain UTF-8-encoded data
> with value /reason/...

reason 部分の最大長は 125 - 2 = 123 バイト。
`&reason[..123]` が不正な UTF-8 を生成するのは、reason が UTF-8-encoded data
と定義されていることへの直接的な違反である。

不正な UTF-8 の reason を受信した側は RFC 6455 Section 8.1 (refs/rfc6455.txt line
2643-2645) に従い接続を強制切断しなければならない (MUST)。

## 設計方針

UTF-8 セーフな reason 切り詰め関数 `truncate_reason` を `src/websocket_close.rs` に
追加し、client/server 両方の `close_internal` から呼び出す。
PBT でのテスト容易性のため、インライン展開ではなく関数として切り出す。

```rust
/// reason が max_bytes を超える場合、UTF-8 文字境界で切り詰める
pub(crate) fn truncate_reason(reason: &str, max_bytes: usize) -> &str {
    if reason.len() > max_bytes {
        let mut end = max_bytes;
        while !reason.is_char_boundary(end) {
            end -= 1;
        }
        &reason[..end]
    } else {
        reason
    }
}
```

`is_char_boundary(0)` は常に `true` のため、`max_bytes` が 0 以上であれば
ループは必ず停止する。`max_bytes == 0` の場合は空文字列が返る。

truncation 修正後も `Frame::close` の `unwrap_or_else` フォールバックは維持する。
`Frame::close` の将来的なエラー条件追加に対する堅牢性を保つため。

```rust
let frame = Frame::close(Some(code.as_u16()), truncated_reason)
    .unwrap_or_else(|_| Frame::close(Some(code.as_u16()), "")
        .expect("empty reason close frame must always succeed"));
```

## 完了条件

1. `src/websocket_close.rs` に `truncate_reason` 関数が追加されている
2. client/server 双方の `close_internal` で `&reason[..123]` が `truncate_reason(reason, 123)` に置換されている
3. client 側 `close_internal` の戻り値型が `()` に変更され、全 16 箇所の呼び出し元で `?` が削除されている
4. `pbt/tests/prop_close_code.rs` に PBT 3 件が追加され全件パスする
5. `cargo check --workspace` がエラーなしで通る
6. `cargo clippy --workspace --all-targets -- -D warnings` が警告なしで通る
7. `cargo test --workspace` が全件パスする
8. `CHANGES.md` の `## develop` に変更履歴エントリが追加されている

## 解決方法

### `src/websocket_close.rs` — `truncate_reason` を追加

設計方針のコードをそのまま追加する。

### `src/lib.rs` — `truncate_reason` を re-export

PBT 外部クレートからテストするため `lib.rs` に re-export を追加する:

```rust
pub use websocket_close::truncate_reason;
```

### `src/websocket_client_connection.rs` — `close_internal` を修正

- truncation 修正: `&reason[..123]` → `truncate_reason(reason, 123)`
- フォールバック内の `.unwrap()` → `.expect("empty reason close frame must always succeed")`
  （CLAUDE.md の `.expect()` 規約に準拠）
- 戻り値型 `Result<(), Error>` → `()` への変更:
  - `return Ok(());` → `return;`
  - `self.send_frame(frame)?;` → `let _ = self.send_frame(frame);`
    （`Result` 型の `#[must_use]` のため `let _ =` が必要。`send_frame` は常に
    `Ok(())` を返すため実害はない。0019 で `policy.encode_and_send()` に置き換えられる
    までの一時的な修正）
  - 末尾の `Ok(())` を削除
- 全呼び出し元の `self.close_internal(...)?;` → `self.close_internal(...);`

### `src/websocket_server_connection.rs` — `close_internal` を修正

- truncation 修正: `&reason[..123]` → `truncate_reason(reason, 123)`
- フォールバック内の `.unwrap()` → `.expect("empty reason close frame must always succeed")`
- `close_internal` は元々戻り値 `()` なのでシグネチャ変更不要

### `pbt/tests/prop_close_code.rs` — PBT を追加

既存の `prop_close_code.rs` は `src/websocket_close.rs` に対応する PBT ファイルであるため、
`truncate_reason` の PBT もここに追加する（新規ファイルは作成しない）。

以下のプロパティを追加する:

1. truncation 結果のバイト長が `max_bytes` 以下である（`reason.len() > max_bytes` の場合）
2. truncation 結果が元の文字列の prefix である
3. `reason.len() <= max_bytes` の場合は reason がそのまま返る

PBT の strategy では `max_bytes` に 0 を含む範囲 (`0..=200` 等) を使用し、
境界値を網羅する。

「任意入力でパニックしないこと」の PBT は書かない（fuzzing の役割）。
fuzzing ターゲットの追加は本 issue のスコープ外とし、必要に応じて別途対応する。

## CHANGES.md 登録内容

- [UPDATE] close_internal の戻り値型を `()` に統一する
  - @実装者名

- [FIX] close_internal の reason 切り詰めが UTF-8 境界を無視する問題を修正する
  - @実装者名
