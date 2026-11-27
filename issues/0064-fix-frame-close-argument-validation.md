# Frame::close の不正な引数を拒否する

- Priority: Low
- Created: 2026-11-27
- Polished: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-frame-close-validation

## 目的

低レベル API の `Frame::close` が、RFC 6455 に反する Close フレームを生成できる。`Frame::close(Some(1005), "")` は送信禁止のコード 1005 を持つフレームを返し、`Frame::close(None, "reason")` は理由を捨てて空のボディを返す。RFC 6455 Section 7.4.1 は 1005 / 1006 / 1015 をステータスコードとして設定することを MUST NOT とし、Section 5.5.1 はボディがある場合の先頭 2 バイトをステータスコードと定めている。

## 優先度根拠

Low。高レベル API の `close()` は `CloseCode::is_sendable` を検査するが、低レベル API の利用者が不正なフレームを送る可能性が残る。理由の破棄は利用者から見て不可解な挙動である。戻り値の型は変わらないが、従来 `Ok` を返していた入力の一部が `Err` になる。

## 現状

- `src/websocket_frame.rs` の `Frame::close` は、`code` が `Some` のときに `CloseCode::is_sendable` を検査しない。
- `code` が `None` のときは `reason` を無視して空のペイロードを返す。
- 戻り値は `Result<Self, Error>` であり、検証を追加してもシグネチャは変わらない。

## 設計方針

- `Some(code)` は `CloseCode::new(code).is_sendable()` を検査し、送信不可なら `Error::invalid_input` を返す。
- `None` で `reason` が空でない場合は `Error::invalid_input` を返す。
- `close_internal` など内部の呼び出し元が送るコードはすべて送信可能であり、影響しない。
- `Frame::close` の doc comment に、送信禁止コードと理由のみのボディを拒否することを追記する。
- 0063 の変更後は 1016-1999 も送信不可になる。スタックでは 0063 の上に積むため、`is_sendable` の新しい許可範囲に従う。

## CHANGES.md 登録内容

```markdown
- [FIX] Frame::close が送信禁止コードと理由のみのボディを拒否する
  - @voluntas
```

## 完了条件

- `Frame::close(Some(1005), "")`、`Some(1006)`、`Some(1015)` が Err になる。
- `Frame::close(Some(1016), "")` が Err になる（0063 適用後）。
- `Frame::close(None, "reason")` が Err になる。
- `Frame::close(None, "")` と `Frame::close(Some(1000), "reason")` は Ok のまま。
- `Frame::close` の doc comment が更新されている。
- `WebSocketClientConnection::close` / `WebSocketServerConnection::close` と `handle_close` の既存テストが通る。
- `make test`（実体は `cargo test --workspace --exclude websocket_client`）が通る。nginx e2e テストは Docker が必要なため GitHub Actions で実行する。
