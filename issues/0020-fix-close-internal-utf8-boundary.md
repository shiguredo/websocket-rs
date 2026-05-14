# 0020: close_internal の reason 切り詰めが UTF-8 境界を無視してパニックする可能性がある

- Priority: High
- Created: 2026-05-14
- Completed: -
- Model: DeepSeek V4 Flash
- Branch: feature/fix-close-utf8-boundary

## 概要

`close_internal` で Close フレームの reason を 123 バイトに切り詰める際に
`&reason[..123]` を使用している。Rust の `&str` スライシングは
バイトインデックスが UTF-8 文字境界でない場合にパニックする。

現在の全呼び出し元は短い ASCII 文字列のため発現しないが、将来の変更で
マルチバイト文字を含む reason が渡された場合に回復不能なパニックが発生する。
本修正は将来の安全性のための予防的修正である。

## 依存する issue

`close_internal` は issue 0019 でも共通トレイトへの抽出対象となっている。
実装順序により競合が発生するため、以下の順序で対応する:

1. **本 issue (0020) を先に実装する**: `close_internal` の UTF-8 切り詰め修正 + 戻り値型 `()` への統一
2. **その後 issue 0019 を実装する**: 共通トレイトへの抽出（0020 修正済みのコードが対象）

本 issue で戻り値型も `()` に統一することで、0019 での `close_internal` 呼び出しの `?` → `;` 変更を不要にする。
なお `close_internal` 内部の `self.send_frame(frame)?;` は本 issue で修正する。
`let _ = self.send_frame(frame);` としないと `Result` 型自体の `#[must_use]` により
コンパイラ警告が発生するため注意。この `let _ =` は 0019 で `send_frame` が削除され
`policy.encode_and_send()` に置き換えられるまでの一時的な修正である。

## 該当コード

`src/websocket_client_connection.rs:641-645`:

```rust
let truncated_reason = if reason.len() > 123 {
    &reason[..123]  // ← UTF-8 文字の途中だとパニック
} else {
    reason
};
```

`src/websocket_server_connection.rs:691-695` (同様):

```rust
let truncated_reason = if reason.len() > 123 {
    &reason[..123]  // ← 同様
} else {
    reason
};
```

### `close_internal` の全呼び出し元

戻り値型 `()` への統一に伴い、client 側の全 16 箇所で `close_internal(...)?;` → `close_internal(...);` に変更する。

**client 側 (16 箇所、すべて `?` 付き):**

| 行 | 呼び出し元メソッド | reason |
|---|---|---|
| 859 | `process_frames` | `"frame decode error"` |
| 876 | `handle_decoded_frame` | `"masked server frame"` |
| 884 | `handle_frame` | `"frame payload too large"` |
| 889 | `handle_frame` | `"reserved bits set"` |
| 896 | `handle_frame` | `"rsv1 set without permessage-deflate"` |
| 905 | `handle_frame` | `"rsv1 must not be set on control frames"` |
| 915 | `handle_frame` | `"rsv1 must not be set on continuation frames"` |
| 939 | `handle_data_frame` | `"new message started before previous completed"` |
| 955 | `handle_data_frame` | `"message too large"` |
| 966 | `handle_continuation` | `"continuation frame without initial frame"` |
| 979 | `handle_continuation` | `"message too large"` |
| 1002 | `decompress_if_needed` | `"received compressed frame without permessage-deflate"` |
| 1033 | `emit_message` | `"invalid UTF-8"` |
| 1051 | `handle_close` | `"close frame payload length must be 0 or >= 2"` |
| 1067 | `handle_close` | `format!("invalid close code: {}", code_val)` |
| 1080 | `handle_close` | `"close frame reason is not valid UTF-8"` |

**server 側 (15 箇所):** すべて `close_internal(...);` のため変更不要（戻り値型は元々 `()`）。

## RFC 根拠

RFC 6455 Section 5.5 (line 1978):
    All control frames MUST have a payload length of 125 bytes or less.

RFC 6455 Section 5.5.1 (line 1989-1993):
    Close フレームのペイロードは 2 バイトのステータスコード + オプションの
    UTF-8 エンコードされた reason で構成される。
    "Following the 2-byte integer, the body MAY contain UTF-8-encoded data
     with value /reason/..."
    したがって reason 部分の最大長は 125 - 2 = 123 バイト。
    `&reason[..123]` が不正な UTF-8 を生成するのは、reason が UTF-8-encoded data
    と定義されていることへの直接的な違反である。

RFC 6455 Section 8.1 (line 2643-2645):
    UTF-8 として解釈するバイト列が不正な UTF-8 だった場合、
    エンドポイントは _Fail the WebSocket Connection_ しなければならない (MUST)。

RFC 6455 Section 7.1.6 (line 2368-2375):
    Close reason は UTF-8-encoded data と定義されている。

バイト境界を無視した切り詰めは不正な UTF-8 を生成し、受信側が接続を切断する
原因となるため、UTF-8 セーフな切り詰めが必要。

## 修正方針

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

`is_char_boundary(0)` は常に `true` のため、`reason: &str` かつ `max_bytes > 0` であれば
ループは必ず停止する。型シグネチャによって `&str` 制約をコンパイル時に保証する。

client/server 両方の `close_internal` で `&reason[..123]` を
`truncate_reason(reason, 123)` に置換する。

また、本 issue ではクライアント側の `close_internal` の戻り値型を
`Result<(), Error>` から `()` に変更し、全呼び出し元の `?` → `;` 変更も
併せて行う（issue 0019 との競合回避のため）。

## 変更対象ファイルと影響範囲

### `src/websocket_close.rs` — `truncate_reason` を追加

修正方針のコードをそのまま追加する。

### `src/websocket_client_connection.rs:634-661`

- truncation 修正（`&reason[..123]` → `truncate_reason(reason, 123)`）
- 戻り値型 `Result<(), Error>` → `()` への変更に伴う内部の修正:
  - line 636: `return Ok(());` → `return;`
  - line 648: `self.send_frame(frame)?;` → `let _ = self.send_frame(frame);`
    （`Result<T, E>` 型自体に `#[must_use]` が付与されているため `let _ =` が必要。
    `send_frame` は常に `Ok(())` を返すためエラーは発生しない。
    この修正は 0019 で `send_frame` が `policy.encode_and_send()` に置き換えられるまでの一時的なもの）
  - line 660: 末尾の `Ok(())` を削除（関数本体の最後の式が空になる）
- 全呼び出し元 `self.close_internal(...)?;` を `self.close_internal(...);` に変更
  （`close_internal` が `()` を返すため `?` はコンパイルエラーになる。grep `close_internal` で検出可能）

### `src/websocket_server_connection.rs:684-709`

- truncation 修正（`&reason[..123]` → `truncate_reason(reason, 123)`）
- `close_internal` は元々戻り値 `()` なのでシグネチャ変更不要。
  server の `send_frame` は `()` を返すため `let _ =` も不要

### `pbt/tests/prop_websocket_close.rs` — PBT を追加

テスト戦略に従い新規作成する。

### 補足: `close_internal` 内部のフォールバック維持

truncation 修正後も `Frame::close` の `unwrap_or_else` フォールバックは維持する。
truncation によって reason 長が 123 以下になることは保証されるが、
`Frame::close` の将来的なエラー条件追加（例: 予約済み close code のチェック）に
対して堅牢性を保つため。AGENTS.md の「性能より堅牢性を優先する」方針に従う。

```rust
let frame = Frame::close(Some(code.as_u16()), truncated_reason)
    .unwrap_or_else(|_| Frame::close(Some(code.as_u16()), "").unwrap());
```

## テスト戦略

truncation 処理を `src/websocket_close.rs` の `truncate_reason` 関数として切り出す
（修正方針に記載のコードをそのまま使用）。

AGENTS.md の「PBT で実現できるものは PBT で書く」方針に従い、
以下のプロパティを `pbt/tests/prop_websocket_close.rs` に追加する:

1. truncation 結果のバイト長が 123 以下である（reason 長が 123 超の場合のみ実行）
2. truncation 結果が元の文字列の prefix である（文字が欠落しても破損しない）
3. `reason.len() <= 123` の場合は reason がそのまま返る（境界値）

AGENTS.md の方針に従い「任意入力でパニックしないこと」の PBT は書かない
（fuzzing の役割。cargo-fuzz ターゲットに期待する）。

## 完了条件

1. `src/websocket_close.rs` に `truncate_reason` 関数が追加されている
2. client/server 双方の `close_internal` で `&reason[..123]` が `truncate_reason(reason, 123)` に置換されている
3. client 側 `close_internal` の戻り値型が `()` に変更され、全 16 箇所の呼び出し元で `?` が削除されている
4. `pbt/tests/prop_websocket_close.rs` に PBT 3 件が追加され全件パスする
5. `cargo check --workspace` がエラーなしで通る
6. `cargo clippy --workspace --all-targets -- -D warnings` が警告なしで通る
7. `cargo test --workspace` が全件パスする
8. `CHANGES.md` の `## develop` に変更履歴エントリが追加され、`@実装者名` が実装者の GitHub ハンドルに置換されている

## CHANGES.md 登録内容

- [UPDATE] close_internal の戻り値型を `()` に統一する
  - @実装者名

- [FIX] close_internal の reason 切り詰めが UTF-8 境界を無視する問題を修正する
  - @実装者名
