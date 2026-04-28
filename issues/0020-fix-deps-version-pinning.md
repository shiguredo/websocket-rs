# 0020: examples 配下の依存バージョン指定を AGENTS.md ルールに揃える

Created: 2026-04-28
Model: Claude Opus 4.7

## 種別

fix

## 優先度

P4

## 概要

`examples/websocket_client/Cargo.toml`、`examples/websocket_server/Cargo.toml`、
`examples/websocket_reverse_proxy/Cargo.toml` の依存バージョン指定が
AGENTS.md のルール「バージョン番号はマイナーまで指定すること」に沿っていない。
これらをマイナー桁指定に揃える。加えて、用途コメントが付いていない依存に
コメントを追加する。

## 根拠

AGENTS.md より:

> バージョン番号はマイナーバージョンまで指定すること
>   例: `nojson = "0.3.10"` ではなく `nojson = "0.3"` とする

> 依存ライブラリには用途をコメントで明記すること

該当箇所 (2026-04-28 時点):

### `examples/websocket_client/Cargo.toml`

| 依存 | 現状 | 問題 |
|---|---|---|
| `shiguredo_http11` | `"2026.1.0-canary.4"` | パッチ + canary suffix まで固定 |
| `aws-lc-rs` | `"1.15.4"` | パッチまで固定 |
| `tokio` | `"1"` | メジャーのみ |

### `examples/websocket_server/Cargo.toml`

| 依存 | 現状 | 問題 |
|---|---|---|
| `tokio` | `"1"` | メジャーのみ |

### `examples/websocket_reverse_proxy/Cargo.toml`

| 依存 | 現状 | 問題 |
|---|---|---|
| `shiguredo_http11` | `"2026.1.0-canary.4"` | パッチ + canary suffix まで固定 |
| `aws-lc-rs` | `"1.15.4"` | パッチまで固定 |
| `tokio` | `"1"` | メジャーのみ |

すべての Cargo.toml で各依存に用途コメント (`# ...`) が付いていない。

## 方針

### バージョン指定

`Cargo.lock` で実際に解決されているバージョンに揃え、マイナー桁までで止める。
2026-04-28 時点の解決値:

- `tokio = "1.52"` (Cargo.lock では 1.52.1)
- `aws-lc-rs = "1.16"` (メインクレート Cargo.toml と同じ)
- `shiguredo_http11 = "2026.1"` (メインクレート Cargo.toml と同じ)

### 用途コメント

各依存の上に 1 行で用途を日本語コメント。例:

```toml
# WebSocket クライアント実装本体
shiguredo_websocket = { path = "../.." }
# 非同期ランタイム
tokio = { version = "1.52", features = [...] }
```

### CHANGES.md

`### misc` サブセクションに `[UPDATE]` で記載。機能影響なし。

## 影響

- `[UPDATE]` 後方互換あり、依存指定とコメントの整理のみ。
- 各 example のビルド対象 crate バージョンは Cargo.lock の解決上限と一致するため
  挙動変化はない。

## テスト

- `cargo build --workspace` が通ることを確認。
- `cargo clippy --workspace -- -D warnings` が通ることを確認。

## 備考

このルール違反は `examples/wscat` (issue #0019 関連) を作る過程で気づいた。
wscat 自体は新規追加なのでルールに完全準拠して作る。本 issue は既存 example の
修正用で、wscat の追加ブランチとは別ブランチ (`feature/fix-examples-deps`) で対応する。
