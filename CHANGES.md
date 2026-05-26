# 変更履歴

- UPDATE
  - 後方互換がある変更
- ADD
  - 後方互換がある追加
- CHANGE
  - 後方互換のない変更
- FIX
  - バグ修正

## develop

## 2026.3.0

**リリース日**: 2026-05-26

- [UPDATE] shiguredo_http11 を 2026.6.1 に上げる
  - @voluntas

### misc

- [ADD] nginx + testcontainers を使った WebSocket e2e テストを追加する
  - @voluntas
- [UPDATE] nginx e2e テストを websocket_client/tests/ に移動する
  - @voluntas

## 2026.2.0

**リリース日**: 2026-05-16

- [CHANGE] base64 クレートを base64ct 1.8 に切り替えるようにする
  - @voluntas
- [CHANGE] flate2 依存を noflate に置き換えてゼロ依存化する
  - @voluntas
- [CHANGE] Sec-WebSocket-Accept の SHA-1 計算で aws-lc-rs を必須依存にして sha1 クレート依存を削除する
  - `aws_lc_rs` feature は廃止する
  - @voluntas
- [CHANGE] `PerMessageDeflate::set_compression_level` を削除する
  - RFC 7692 のネゴシエーション対象外の内部 API であり、置き換え先の noflate に対応機能が無いため
  - @voluntas
- [CHANGE] `Decompressor::reset` を削除する
  - 内部処理に統合する
  - @voluntas
- [UPDATE] shiguredo_http11 を 2026.5.0 に上げる
  - @voluntas

### misc

- [UPDATE] examples の aws-lc-rs を 1.16 に上げる
  - 併せて固定指定 1.15.4 をマイナーまでの指定 1.16 に揃える
  - @voluntas
- [UPDATE] examples の rustls-platform-verifier を 0.7 に上げる
  - @voluntas

## 2026.1.0

**リリース日**: 2026-03-26

**公開**
