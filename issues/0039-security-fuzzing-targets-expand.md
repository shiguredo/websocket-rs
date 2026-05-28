# fuzzing ターゲットを拡充する

- Priority: High
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: mimo-v2.5-pro
- Branch: feature/add-fuzzing-targets-expand

## 目的

ハンドシェイク完了後の接続処理・`Compressor`・`PerMessageDeflateConfig` パースなど、現状 fuzz が触れていないセキュリティクリティカル経路を `cargo-fuzz` でカバーする。

## 優先度根拠

High。任意入力に対するパニック安全性は AGENTS.md で fuzzing の役割。現行 `fuzz_client_connection` / `fuzz_server_connection` は有効な 101 レスポンス / リクエストが無い場合、接続確立後のフレーム処理に到達しにくい。

## 現状

| ターゲット | 不足 |
|---|---|
| `fuzz_client_connection.rs` | ハンドシェイク後のフレーム・フラグメント・Close 等 |
| `fuzz_server_connection.rs` | 同上 |
| `fuzz_deflate_decompress.rs` | `Decompressor` のみ（`Compressor` 未対象） |
| `fuzz_extension_parse.rs` | `Extension::parse` / `parse_strict` のみ（PMCE config 未対象） |

## 設計方針

1. **接続確立後 fuzz（新規推奨）**
   - `fuzz/fuzz_targets/fuzz_client_connection_established.rs`（名称は実装時に `fuzz_` プレフィックスを維持）
   - 最小の合法 101 レスポンス（またはサーバー側は合法リクエスト）でハンドシェイクを完了させた後、残り入力を `feed_recv_buf` へ投入
   - `poll_event` / `poll_output` をループでドレイン（既存 fuzz と同様）
   - サーバー用も対称に追加

2. **`Compressor` fuzz（新規）**
   - `fuzz/fuzz_targets/fuzz_deflate_compress.rs` 等
   - 任意バイト列を `Compressor::compress`（または公開 API 経由）に投入しパニックしないこと

3. **`fuzz_extension_parse.rs` 拡張**
   - `Extension::parse_strict` 成功例から `Extension` を構築し、`PerMessageDeflateConfig::from_extension_for_server_request` / `from_extension_for_client_response` を呼ぶ（`0056` と重複する場合は本 issue に統合してよい）

`fuzz_deflate_roundtrip` のような性質検証は **追加しない**（PBT: `prop_deflate.rs` の役割）。

## 実装手順

1. 各新規 fuzz ターゲットファイルを `fuzz/fuzz_targets/` に作成
2. `fuzz/Cargo.toml` の `[[bin]]` に新規ターゲットを登録
3. `cargo fuzz build` で全ターゲットがコンパイルできることを確認
4. 既存 fuzz ターゲットの regress が無いことを確認

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] ハンドシェイク後処理・Compressor・PMCE パースの fuzz ターゲットを拡充する
  - @<GitHub ハンドル>
```

## 完了条件

- ハンドシェイク完了後に recv をフィードする fuzz ターゲットがクライアント / サーバーにある
- `Compressor` 向け fuzz ターゲットがある
- `fuzz_extension_parse.rs` または専用ターゲットで PMCE config パースが fuzz される
- `cargo fuzz build` が通過する
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある
