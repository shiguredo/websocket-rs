# flate2 依存を noflate に置き換える

Created: 2026-04-18
Completed: 2026-04-18
Model: Opus 4.7

## 優先度

P2

## 概要

`src/deflate/permessage_deflate.rs` の permessage-deflate 圧縮/解凍実装が現在 `flate2 = "1.1"` を使用している。これをゼロ依存・純 Rust・`#![forbid(unsafe_code)]` の `noflate` に置き換える。破壊的変更を伴う。

## 根拠

- **依存最小化**: CLAUDE.md の「依存は最小限にすること」に沿う。flate2 は `miniz_oxide` / `libz-sys` / `cfg-if` を推移的に引き込むが、noflate はゼロ依存。
- **安全性**: noflate は `#![forbid(unsafe_code)]` を宣言している。
- **RFC 7692 への適合**: noflate は permessage-deflate を明示的にサポートしており、`Encoder::sync_flush()` が RFC 7692 §7.2.1 の 4 バイトトレーラを自前で付与する。`Encoder::reset_history()` で `*_no_context_takeover` 向けの LZ77 履歴リセットを提供する。
- **RFC 7692 のネゴシエーションパラメータは 4 つ** (`server_no_context_takeover` / `client_no_context_takeover` / `server_max_window_bits` / `client_max_window_bits`) のみで、圧縮レベル (0-9) は RFC の概念ではなく zlib 実装固有の API。noflate に置き換えてレベル API を失っても RFC 準拠性・相互運用性には影響しない。

## 破壊的変更

以下の公開 API を削除する:

- `shiguredo_websocket::PerMessageDeflate::set_compression_level(&mut self, level: u32)`
- `shiguredo_websocket::Compressor::set_level(&mut self, level: u32)` (内部だが `Compressor` は re-export されている)
- `shiguredo_websocket::Decompressor::reset(&mut self)` (外部呼び出しなし)

## 変更内容

- `Cargo.toml` の `flate2 = "1.1"` を削除し `noflate = "0.0.3"` を追加
- `src/deflate/permessage_deflate.rs` を noflate ベースで全面書き換え
  - `Encoder` / `Decoder` を保持し feed/output/advance パターンで処理
  - 圧縮: `feed` → `sync_flush` → RFC 7692 トレーラを `truncate` → `reset_after_message` 時は `reset_history()`
  - 解凍: 入力を 8 KiB チャンクで `feed` し、都度 `output().len()` で `max_size` 超過をチェック（Zip Bomb 対策）。末尾に RFC 7692 トレーラを追加してから feed。`reset_after_message` 時は `Decoder::new()` で作り直す
- `src/websocket_client_connection.rs` / `src/websocket_server_connection.rs` の window_bits 制約コメントを noflate 版に更新
- `pbt/tests/prop_permessage_deflate.rs` の `prop_compression_levels_preserve_data` を削除
- `CHANGES.md` の `## develop` に `[CHANGE]` エントリを追加

## 解決方法

- `Cargo.toml`: `flate2 = "1.1"` を削除し `noflate = "0.0.3"` を追加した。
- `src/deflate/permessage_deflate.rs` を `noflate::{Decoder, Encoder}` ベースで全面書き換え:
  - `Compressor::compress` は `encoder.feed` → `encoder.sync_flush` → `encoder.output` + `advance` → RFC 7692 トレーラ (`0x00 0x00 0xFF 0xFF`) を `truncate` → `reset_after_message` 時は `encoder.reset_history()` の手順。
  - `Decompressor::decompress` は入力を 8 KiB チャンク + トレーラに分けて `decoder.feed` し、毎チャンクで累積出力サイズを `max_size` と比較して Zip Bomb を防ぐ。`reset_after_message` 時は `Decoder::new()` で作り直す。
  - 旧 `level` フィールド、`Compressor::set_level`、`Decompressor::reset`、`PerMessageDeflate::set_compression_level` を削除。
  - 旧来の max_iterations ガードは不要になったため削除（noflate の feed は need-more-bytes を `Ok(())` で返すため無限ループしない）。
- `src/websocket_server_connection.rs` / `src/websocket_client_connection.rs` の window_bits 制約コメントを「flate2 の制約」から「noflate の制約」に書き換え。
- `pbt/tests/prop_permessage_deflate.rs`: `prop_compression_levels_preserve_data` を削除。残り 15 件の PBT はすべて成功。
- `CHANGES.md`: `## develop` に `[CHANGE]` エントリを 3 件追記。
- `fuzz/Cargo.lock`: `cargo update` で依存グラフを更新。

テスト結果: `cargo test --workspace` で全テスト合格 (331 件)、`cargo clippy --workspace --all-targets` で警告なし。
