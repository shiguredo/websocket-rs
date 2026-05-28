# nginx e2e テストを websocket_client/tests/ に移動する

- Priority: High
- Created: 2026-05-26
- Completed: 2026-05-26
- Model: Opus 4.7
- Branch: feature/refactor-nginx-e2e-location

## 目的

nginx 経由の WebSocket e2e テストは「クライアントがプロキシ経由で正しく動作するか」を検証するものであり、`websocket_client` のテストとして配置するのが自然。現在の独立クレート (`examples/websocket_e2e_nginx/`) から `examples/websocket_client/tests/` に移動して、テストの配置を論理的な責務に合わせる。

## 優先度根拠

issue 0024 (curl e2e テスト) の実装前に配置方針を確定させる必要がある。0024 は `websocket_server/tests/` に配置する方針であり、0023 との一貫性を先に確保する。

## 現状

- `examples/websocket_e2e_nginx/` が独立した workspace メンバーとして存在する
- echo サーバー + nginx プロキシヘルパー + 5 つの e2e テストが含まれる
- `Cargo.toml` の workspace members に `examples/websocket_e2e_nginx` が含まれる

## 設計方針

### 移動元と移動先

- `examples/websocket_e2e_nginx/src/lib.rs` の echo サーバー → `examples/websocket_client/tests/helpers/echo_server.rs`
- `examples/websocket_e2e_nginx/tests/helpers/mod.rs` の nginx ヘルパー → `examples/websocket_client/tests/helpers/nginx.rs`
- `examples/websocket_e2e_nginx/tests/nginx_websocket.rs` → `examples/websocket_client/tests/nginx_websocket.rs`
- `examples/websocket_client/tests/helpers/mod.rs` を新規作成して `echo_server` と `nginx` をサブモジュールとして公開する

### 依存関係の変更

`examples/websocket_client/Cargo.toml` の `[dev-dependencies]` に以下を追加する:

```toml
[dev-dependencies]
# Docker コンテナ起動 (実 nginx 相手の e2e テスト)
testcontainers = { version = "0.27", default-features = false, features = ["aws-lc-rs"] }
```

既に `[dependencies]` にある tokio は `macros`, `rt-multi-thread`, `net`, `io-util`, `time` を含むため追加不要。

### 削除

- `examples/websocket_e2e_nginx/` ディレクトリを完全に削除する
- `Cargo.toml` の workspace members から `examples/websocket_e2e_nginx` を削除する

### echo サーバーの変更

- `pub async fn spawn_echo_server()` から `pub` を外す (`pub(super)` または非公開)
- `lib.rs` ではなくテストヘルパーモジュール内に配置するため、クレート外からの呼び出しは不要

### CI

`websocket_e2e_nginx` クレートが消えるため、macOS 除外の条件分岐を更新する:

```yaml
# macOS ランナーには Docker がプリインストールされていないため
# Docker を必要とする e2e テストを除外する
- run: cargo test --workspace
  if: runner.os != 'macOS'
- run: cargo test --workspace --exclude websocket_client
  if: runner.os == 'macOS'
```

ただし、これにより macOS で websocket_client の通常の単体テスト (将来追加された場合) も除外される点に注意が必要。現時点では websocket_client にテストはないため問題ない。

### 後方互換への影響

なし。テストクレートの再配置のみ。

### CHANGES.md

テスト配置の変更は機能に直接影響しないため、`## develop` の `### misc` セクションに記載する。

## 完了条件

- `examples/websocket_e2e_nginx/` が削除されている
- `examples/websocket_client/tests/nginx_websocket.rs` に 5 つの e2e テストが存在する
- Docker が利用可能な環境で `cargo test -p websocket_client` が全て通る
- CI の macOS 除外が正しく動作する

## 解決方法

設計方針どおりに移動を実施した。

### 移動内容

- echo サーバー: `websocket_e2e_nginx/src/lib.rs` → `websocket_client/tests/helpers/echo_server.rs`
- nginx ヘルパー: `websocket_e2e_nginx/tests/helpers/mod.rs` → `websocket_client/tests/helpers/nginx.rs`
- テスト本体: `websocket_e2e_nginx/tests/nginx_websocket.rs` → `websocket_client/tests/nginx_websocket.rs`
- helpers/mod.rs を新規作成し echo_server と nginx をサブモジュールとして整理

### 削除

- `examples/websocket_e2e_nginx/` ディレクトリを完全に削除
- `Cargo.toml` の workspace members から `websocket_e2e_nginx` を削除

### その他の変更

- `websocket_client/Cargo.toml` に testcontainers を dev-dependency として追加
- CI の macOS 除外対象を `websocket_e2e_nginx` から `websocket_client` に変更
