# 解凍エラー時に Close フレームを送る

- Priority: Low
- Created: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-close-on-decompression-error

## 目的

permessage-deflate の解凍に失敗したとき、接続を失敗させるだけで Close フレームを送らない。RFC 6455 Section 7.1.7 は、接続確立後に失敗させる場合、適切なステータスコードの Close フレームを送ることを SHOULD としている。他のプロトコル違反は Close 1002 を送っており、解凍エラーだけが例外になっている。

## 優先度根拠

Low。相手からは異常切断 (1006) に見え、診断性が下がる。プロトコルの相互運用性への影響は小さい。

## 現状

- `src/websocket_connection_shared.rs` の `decompress_if_needed` は `deflate.decompress(...)?` でエラーを伝播するだけで、`close_internal` を呼ばない。
- `src/deflate/permessage_deflate.rs` の `Decompressor::decompress` は不正な DEFLATE データで `ErrorKind::InvalidData` を返す。

## 設計方針

`decompress_if_needed` のエラー時に `close_internal(CloseCode::INVALID_PAYLOAD, "invalid compressed data")` を呼び、Close 1007 を送ってからエラーを返す。

## CHANGES.md 登録内容

```markdown
- [FIX] 解凍エラー時に Close フレームを送る
  - @voluntas
```

## 完了条件

- 不正な DEFLATE データを受けたとき、`feed_recv_buf` がエラーを返し、Close 1007 の `SendData` が出力される。
- クライアント側とサーバー側の両方で動作する。
- `cargo test --workspace` が通る。
