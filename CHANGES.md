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

### misc

## 2026.3.0

**リリース日**: 2026-05-28

- [CHANGE] deprecated な PerMessageDeflateConfig::from_extension を削除する
  - @voluntas
- [CHANGE] 内部向けシンボル `ByteSliceExt` / `VecExt` / `Compressor` / `Decompressor` / `truncate_reason` を通常の pub use から除外し #[doc(hidden)] で再エクスポートに変更する
  - @voluntas
- [CHANGE] Extension::parse_strict のエラー型を String から ExtensionParseError に変更する
  - @voluntas
- [CHANGE] ClientConnectionOptions / ServerConnectionOptions のフィールドを private にする
  - @voluntas
- [CHANGE] Timestamp の内部フィールドを private にする
  - @voluntas
- [ADD] ExtensionParseError に Display / Error を実装し、ExtensionParseContext と合わせて lib.rs から公開する
  - @voluntas
- [UPDATE] shiguredo_http11 を 2026.6.1 に上げる
  - @voluntas
- [UPDATE] close_internal の戻り値型を `()` に統一する
  - @voluntas
- [FIX] close_internal の reason 切り詰めが UTF-8 境界を無視する問題を修正する
  - @voluntas
- [FIX] サーバーレスポンスで複数行の Sec-WebSocket-Extensions を誤って拒否する問題を修正する
  - @voluntas

### misc

- [ADD] nginx + testcontainers を使った WebSocket e2e テストを追加する
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
