# window bits パラメータの符号付き表記を拒否する

- Priority: Medium
- Created: 2026-11-27
- Polished: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-window-bits-sign

## 目的

permessage-deflate の `server_max_window_bits` / `client_max_window_bits` に `+8` や `+08` を指定しても受理される。RFC 7692 Section 7.1.2.1 / 7.1.2.2 の ABNF は `1*DIGIT` であり、符号を含む値は不正である。先頭ゼロは同節本文の "a decimal integer value without leading zeroes" により不正である。RFC 7692 Section 7 は、不正値を含む offer をサーバーが拒否し (MUST decline)、不正値を含む応答をクライアントが接続失敗させる (MUST Fail) ことを要求する。

## 優先度根拠

Medium。実装間で受理集合が食い違い、ABNF に厳密な相手との相互接続に失敗する。

## 現状

- `src/websocket_extension.rs` の `PerMessageDeflateConfig::from_extension_validated` は、先頭ゼロ検査の後に `value.parse::<u8>()` を使う。
- Rust の `str::parse::<u8>()` は先頭の `+` を受け付けるため `+8` は 8 になる。
- 先頭ゼロ検査は `starts_with('0')` のみのため `+08` を検出できない。
- `src/websocket_server_connection.rs` の `WebSocketServerConnection::negotiate_deflate` は、offer の `server_max_window_bits` を `value.parse::<u8>().ok()` で直接解釈しており、`from_extension_validated` の検査を通らない。この経路でも `+8` は 8 として扱われる。

## 設計方針

`websocket_extension.rs` に共通の厳密パーサ (`parse_window_bits` など) を置き、次の検査を行う。

- 値が空でないこと
- ASCII 数字だけで構成されること
- 先頭が `0` で 2 文字以上でないこと
- 8-15 の範囲であること

この関数を `from_extension_validated` の `server_max_window_bits` と `client_max_window_bits` の両方で使う。

`negotiate_deflate` の生パースも同じ関数に置き換える。offer の値が不正なら、その offer は受理しない (拡張不採用)。不正な offer に対して `permessage-deflate` を含む応答を組み立てた場合は `handshake_rejected` とする。

0065 も同じ `negotiate_deflate` を変更するため、共通関数は 0065 から再利用できる形にする。

## CHANGES.md 登録内容

```markdown
- [FIX] window bits パラメータの符号付き表記を拒否する
  - @voluntas
```

## 完了条件

- `PerMessageDeflateConfig::from_extension_for_server_request` (offer) と `from_extension_for_client_response` (response) に `+8` / `+08` を渡すと `ExtensionParseError::InvalidValue` を返す PBT / 単体テストが `pbt/tests/prop_websocket_extension.rs` にある。
- 同じ API が 0-7 と 16-255 を `ExtensionParseError::InvalidValue` で拒否する PBT が同ファイルにある (現状は未カバー)。
- `8` と `15` は `from_extension_validated` のレベルで受理される。接続レベルの `client_max_window_bits < 15` 制約は noflate の制約であり、この検査とは別である。
- `accept_handshake` に `server_max_window_bits=+8` の offer と `permessage-deflate` 応答を渡すと `HandshakeRejected` になる。
- 既存の `pbt/tests/prop_websocket_handshake.rs` の先頭ゼロ拒否 PBT (`prop_server_max_window_bits_leading_zeros_rejected` / `prop_client_max_window_bits_leading_zeros_rejected`) が通る。
- `make test`（実体は `cargo test --workspace --exclude websocket_client`）が通る。nginx e2e テストは Docker が必要なため GitHub Actions で実行する。
