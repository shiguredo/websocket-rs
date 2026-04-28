# 0018: sha1 のバックエンドを feature で aws-lc-rs に切り替えられるようにする

Created: 2026-04-28
Model: Claude Opus 4.7

## 種別

enhancement

## 優先度

P3

## 概要

`Sec-WebSocket-Accept` の計算に使う SHA-1 のバックエンドを `aws_lc_rs` feature で
aws-lc-rs に切り替え可能にする。デフォルトは現状の `sha1` クレート (RustCrypto) のまま。

## 根拠

- 本クレートでの SHA-1 用途は `src/websocket_handshake.rs:651` (`calculate_accept_from_key`) の 1 箇所のみ
- 実運用では rustls + aws-lc-rs と同居するケースが多く、SHA-1 実装が二重に取り込まれる
- aws-lc-rs の SHA-1 はハードウェア命令 (x86 SHA-NI / ARMv8 Crypto Extensions) を使うため、
  pure Rust 実装の `sha1` クレートと比べて性能差が大きい
- `Sec-WebSocket-Accept` の計算は接続ごとに発生するため、大量接続を捌くサーバ用途では
  SHA-1 実装の差が効いてくる
- CLAUDE.md の方針「暗号ライブラリは aws-lc-rs を使うこと」とも整合する

## 方針

### Cargo.toml

```toml
[dependencies]
sha1 = "0.11"
aws-lc-rs = { version = "...", optional = true }

[features]
default = []
aws_lc_rs = ["dep:aws-lc-rs"]
```

- `sha1` クレートはベース依存のまま（feature 無効時のフォールバック）
- `aws_lc_rs` feature を有効化すると aws-lc-rs を使う
- feature 名は `aws_lc_rs`（rustls の慣例に合わせる）

### コード

`src/websocket_handshake.rs` の SHA-1 呼び出しを `#[cfg(feature = "aws_lc_rs")]` で分岐する。

aws-lc-rs 側の API:

```rust
use aws_lc_rs::digest;
let hash = digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, combined.as_bytes());
Base64::encode_string(hash.as_ref())
```

`SHA1_FOR_LEGACY_USE_ONLY` という命名だが、RFC 6455 で固定されたハンドシェイク処理であり
他に選択肢はないのでそのまま使う。

## 影響

- `[ADD]` 後方互換あり、opt-in feature の追加
- 既存ユーザのビルド条件は変わらない
- `aws_lc_rs` feature 有効時も `sha1` クレートは依存に残る（未使用）

## テスト

- `calculate_accept_from_key` の既存テストが両 feature で通ることを確認
- CI で `--features aws_lc_rs` を含むビルドマトリクスを追加
