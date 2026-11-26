# Frame::close の不正な引数を拒否する

- Priority: Low
- Created: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-frame-close-validation

## 目的

低レベル API の `Frame::close` が、RFC 6455 に反する Close フレームを生成できる。`Frame::close(Some(1005), "")` は送信禁止のコード 1005 を持つフレームを返し、`Frame::close(None, "reason")` は理由を捨てて空のボディを返す。RFC 6455 Section 7.4.1 は 1005 / 1006 / 1015 をステータスコードとして設定することを MUST NOT とし、Section 5.5.1 はボディがある場合の先頭 2 バイトをステータスコードと定めている。

## 優先度根拠

Low。高レベル API の `close()` は `CloseCode::is_sendable` を検査するが、低レベル API の利用者が不正なフレームを送る可能性が残る。理由の破棄は利用者から見て不可解な挙動である。

## 現状

- `src/websocket_frame.rs` の `Frame::close` は、`code` が `Some` のときに `CloseCode::is_sendable` を検査しない。
- `code` が `None` のときは `reason` を無視して空のペイロードを返す。
- 戻り値は `Result<Self, Error>` であり、検証を追加してもシグネチャは変わらない。

## 設計方針

- `Some(code)` は `CloseCode::new(code).is_sendable()` を検査し、送信不可なら `Error::invalid_input` を返す。
- `None` で `reason` が空でない場合は `Error::invalid_input` を返す。
- `close_internal` など内部の呼び出し元が送るコードはすべて送信可能であり、影響しない。

## CHANGES.md 登録内容

```markdown
- [CHANGE] Frame::close が送信禁止コードと理由のみのボディを拒否する
  - @voluntas
```

## 完了条件

- `Frame::close(Some(1005), "")`、`Some(1006)`、`Some(1015)` が Err になる。
- `Frame::close(None, "reason")` が Err になる。
- `Frame::close(None, "")` と `Frame::close(Some(1000), "reason")` は Ok のまま。
- 高レベル API のクローズ処理が従来どおり動作する。
- `cargo test --workspace` が通る。
