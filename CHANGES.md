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

- [UPDATE] sha1 を 0.11 に上げる
  - @voluntas
- [UPDATE] shiguredo_http11 を 2026.4 に上げる
  - @voluntas
- [ADD] `aws_lc_rs` feature を追加し Sec-WebSocket-Accept の SHA-1 計算を aws-lc-rs に切り替え可能にする
  - @voluntas
- [CHANGE] base64 クレートを base64ct 1.8 に切り替えるようにする
  - @voluntas
- [CHANGE] flate2 依存を noflate に置き換えてゼロ依存化する
  - @voluntas
- [CHANGE] `PerMessageDeflate::set_compression_level` を削除する
  - RFC 7692 のネゴシエーション対象外の内部 API であり、置き換え先の noflate に対応機能が無いため
  - @voluntas
- [CHANGE] `Decompressor::reset` を削除する
  - 内部処理に統合する
  - @voluntas

### misc


## 2026.1.0

**リリース日**: 2026-03-26

**公開**
