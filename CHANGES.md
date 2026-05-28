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
- [UPDATE] close_internal の戻り値型を `()` に統一する
  - @voluntas
- [FIX] close_internal の reason 切り詰めが UTF-8 境界を無視する問題を修正する
  - @voluntas
- [FIX] サーバーレスポンスで複数行の Sec-WebSocket-Extensions を誤って拒否する問題を修正する
  - @voluntas

### misc

- [UPDATE] クライアント / サーバー間のフレーム処理ロジックを SharedConnectionState に共通化する
  - @voluntas
- [UPDATE] 不要な `#[allow(dead_code)]` を削除し、未使用の `unsupported` のみ `#[expect(dead_code)]` に変更する
  - @voluntas
- [UPDATE] shiguredo_http11 のバージョン指定をマイナーまでに修正する
  - @voluntas
- [UPDATE] crate-level doc 内の `.unwrap()` を `.expect()` に修正する
  - @voluntas
- [UPDATE] is_valid_token の重複実装を src/token.rs に共通化する
  - @voluntas
- [UPDATE] close メソッドの検証ロジックを SharedConnectionState に共通化する
  - @voluntas
- [UPDATE] send_text 等の重複ロジックを SharedConnectionState に集約する
  - @voluntas
- [UPDATE] websocket_handshake.rs の解決済み issue 参照の TODO コメントを設計メモコメントに整理する
  - @voluntas
- [UPDATE] パニックのみ検証する PBT を削除し fuzzing に役割を寄せる
  - @voluntas
- [UPDATE] prop_error.rs の ERROR_KINDS に全 ErrorKind バリアントを含める
  - @voluntas
- [UPDATE] ハンドシェイク後処理・Compressor・PMCE パースの fuzz ターゲットを拡充する
  - @voluntas
- [UPDATE] RFC 引用コメントの節番号を一次資料に合わせて修正する
  - @voluntas
- [UPDATE] SharedConnectionState のフィールドをカプセル化する
  - @voluntas
- [UPDATE] ConnectionState の許可遷移を set_state で検証する
  - @voluntas
- [UPDATE] websocket_connection_shared.rs を fragment_buffer / frame_policy に分割する
  - @voluntas
- [UPDATE] websocket_handshake.rs をリクエスト / レスポンスモジュールに分割する
  - @voluntas
- [UPDATE] accept_handshake を責務ごとの private メソッドに分割する
  - @voluntas
- [UPDATE] complete_handshake から deflate ネゴシエーション検証を抽出する
  - @voluntas
- [UPDATE] Extension::parse と parse_strict の共通パースロジックを抽出する
  - @voluntas

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
