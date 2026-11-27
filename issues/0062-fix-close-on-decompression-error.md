# 解凍エラー時に適切な Close フレームを送る

- Priority: Low
- Created: 2026-11-27
- Polished: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-close-on-decompression-error

## 目的

permessage-deflate の解凍に失敗したとき、接続を失敗させるだけで Close フレームを送らない。RFC 6455 Section 7.1.7 は、接続確立後に失敗させる場合、適切なステータスコードの Close フレームを送ることを SHOULD としている。

受信経路では、フレームデコード・マスキング・RSV・Close ボディの不正に 1002、不正な UTF-8 に 1007、フレームサイズ・メッセージサイズの超過に 1009、Pong タイムアウトに 1008 を送っており、解凍エラーだけが Close を送らない例外になっている。

## 優先度根拠

Low。相手からは異常切断 (1006) に見え、診断性が下がる。プロトコルの相互運用性への影響は小さい。

## 現状

- `src/websocket_connection_shared.rs` の `decompress_if_needed` は `deflate.decompress(...)` の結果をそのまま返すだけで、`close_internal` を呼ばない。
- `src/deflate/permessage_deflate.rs` の `Decompressor::decompress` は、不正な DEFLATE ストリームと `max_decompressed_size` の超過の両方で `ErrorKind::InvalidData` を返す。エラーだけでは原因を区別できない。
- noflate の `Decoder` は入力の枯渇をエラーにしないため、切り詰められたストリームが `Ok` になる場合がある。テストに使うデータは、`Decoder::feed` がエラーを返すものを選ぶ必要がある。
- `pbt/tests/prop_websocket_client_connection.rs` の `prop_deflate_zip_bomb_protection` は `Err` の理由だけを確認しており、出力される Close コードを見ていない。

## 設計方針

解凍エラーの原因別に Close コードを選ぶ。

- 不正な DEFLATE ストリーム: Close 1007 (invalid payload data)。圧縮ペイロードの内容が壊れているため、不正 UTF-8 と同じ「ペイロード不正」として扱う。
- 解凍後サイズが `max_decompressed_size` を超えた場合: Close 1009 (message too big)。フレームサイズ・メッセージサイズの超過と同じ扱いにする。

区別の方法は `Error::with_close_code` を使う。

- `Decompressor::decompress` の `decoder.feed` エラーに `CloseCode::INVALID_PAYLOAD.as_u16()` を設定する。
- サイズ超過エラーに `CloseCode::MESSAGE_TOO_BIG.as_u16()` を設定する。
- `decompress_if_needed` は `Err(e)` のときに `e.close_code` を取り出し、`close_internal(CloseCode::new(code), "decompression failed", policy)` を呼んでから `Err(e)` を返す。
- `close_code` がない場合は 1002 (protocol error) にフォールバックする。
- `ErrorKind` は `InvalidData` のまま変更しない (公開 enum の破壊的変更を避ける)。

切り詰められたストリームの完全性検出は本 issue の対象外とする。permessage-deflate は BFINAL を立てないため、`is_finished()` による終端検査は使えない。

## CHANGES.md 登録内容

```markdown
- [FIX] 解凍エラー時に適切な Close フレームを送る
  - @voluntas
```

## 完了条件

- RSV1=1 のテキストフレームで payload を `[0xFF]` にすると、クライアント接続とサーバー接続の両方で `feed_recv_buf` が `Err` を返し、Close 1007 の `SendData` が出力される。テストは `tests/test_websocket_connection_shared.rs` に permessage-deflate を合意した接続を作るヘルパを追加して行う。
- `max_decompressed_size` を小さくし、有効な圧縮データを送ると `Err` かつ Close 1009 が出力される。既存の `prop_deflate_zip_bomb_protection` に Close コードの確認を追加する。
- 返される `Error` の `close_code` が 1007 / 1009 になっており、`ErrorKind` は `InvalidData` のままである。
- 切り詰められたストリームの検出は本 issue の対象外であることをコードコメントとテストに残す。
- `make test`（実体は `cargo test --workspace --exclude websocket_client`）が通る。nginx e2e テストは Docker が必要なため GitHub Actions で実行する。
