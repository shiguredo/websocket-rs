# nginx + testcontainers を使った WebSocket e2e テストを追加する

- Priority: Medium
- Created: 2026-05-26
- Completed: 2026-05-26
- Polished: 2026-05-26
- Model: Opus 4.7
- Branch: feature/add-nginx-e2e-test

## 目的

shiguredo_websocket の Sans I/O クライアント・サーバー実装が、実際の WebSocket プロキシ (nginx) を経由しても正しく動作することを検証する。

## 優先度根拠

ライブラリとしての正常動作は PBT で検証済みだが、実際のミドルウェアを挟んだ e2e テストがないと本番環境での動作保証が弱い。

## 現状

- `examples/` には `websocket_client`、`websocket_server`、`websocket_reverse_proxy` が存在する
- テストは `pbt/` (proptest) と `fuzz/` (libfuzzer) のみで、実サーバーを使った e2e テストは存在しない

## 設計方針

### プロジェクト構成

`examples/websocket_e2e_nginx/` を新規 workspace メンバーとして追加する。http11-rs の `examples/http11_client`（`src/lib.rs` + `tests/` 構成）のパターンを踏襲する。

ルート `Cargo.toml` の変更:

```toml
[workspace]
members = [
  "examples/websocket_client",
  "examples/websocket_e2e_nginx",  # 追加
  "examples/websocket_reverse_proxy",
  "examples/websocket_server",
  "pbt",
]
```

新規作成ファイル:

- `examples/websocket_e2e_nginx/Cargo.toml`
- `examples/websocket_e2e_nginx/src/lib.rs` (echo サーバーとヘルパー関数)
- `examples/websocket_e2e_nginx/tests/helpers/mod.rs` (testcontainers ヘルパー)
- `examples/websocket_e2e_nginx/tests/nginx_websocket.rs` (テスト本体)

### echo サーバー

`src/lib.rs` に echo サーバーを定義する。

- `TcpListener::bind("0.0.0.0:0")` でランダムポートにバインドし、`local_addr().port()` でポート番号を取得する（Docker コンテナから `host.docker.internal` 経由でアクセスするため、ループバック `127.0.0.1` ではなく全インターフェースにバインドする）
- バインド完了後に `tokio::spawn` で echo サーバータスクを起動する（バインド後に spawn するためリッスン可能状態の待機は不要）
- `ServerConnectionOptions::new().ping_interval(0)` で ping を無効化する（テスト用のためタイマー管理を省略する）
- Sans I/O ループ:
  1. TCP からデータを読み `feed_recv_buf` に渡す
  2. `accept_handshake_auto` でハンドシェイクを受諾する（状態が `Connecting` かつ `handshake_request().is_some()` のとき）
  3. `poll_event` でイベントを処理する:
     - `TextMessage` / `BinaryMessage`: そのままエコーバック（`send_text` / `send_binary`）
     - `Close`: ライブラリが自動で Close 応答を送信するため、手動で `close()` を呼ぶ必要はない
     - その他: 無視
  4. `poll_output` で出力を処理する:
     - `SendData`: TCP ソケットに書き出す
     - `CloseConnection`: TCP 接続を切断しタスクを終了する
     - その他 (`SetTimer` / `ClearTimer`): `ping_interval(0)` により発行されないが、網羅性のために無視する

### クライアント（テスト側）

テストコード内で `WebSocketClientConnection` を使って nginx 経由で echo サーバーに接続する。

- `ClientConnectionOptions::new("127.0.0.1:{nginx_port}", "/")` で接続オプションを生成する（host は nginx コンテナのホスト側アドレス）
- `RandomSource` トレイトの実装が必要（`aws-lc-rs::rand::fill` を使用、既存 example の `SecureRandom` と同等）
- `WebSocketClientConnection::feed_recv_buf` はサーバー側と異なり `Timestamp` 引数を取る。`SystemTime::now().duration_since(UNIX_EPOCH)` から `Timestamp::from_millis` で生成する（既存 example と同パターン）
- 各テストの受信ループは `tokio::time::timeout` でタイムアウトを設ける（5 秒程度）。Sans I/O クライアントは自分で TCP read + `feed_recv_buf` + `poll_event` のループを回す必要があり、タイムアウトがないとテストがハングするリスクがある

### nginx コンテナ

`tests/helpers/mod.rs` に testcontainers ヘルパーを定義する。http11-rs の `tests/helpers/mod.rs` のパターンを踏襲する。

- `ensure_docker()` で Docker daemon の存在を確認し、不在の場合は panic で即座に失敗する（`#[ignore]` は使わない）
- `nginx:1.27-alpine` を使用する（バージョンを固定して再現性を担保する）
- `WaitFor::message_on_either_std("start worker processes")` で nginx の起動完了を待機する
- echo サーバーのポート番号を埋め込んだ nginx.conf を `format!` で動的生成し、`with_copy_to("/etc/nginx/conf.d/default.conf", ...)` でコンテナに渡す
- **Linux 環境対応**: `host.docker.internal` は macOS の Docker Desktop ではデフォルトで解決できるが、Linux の Docker Engine ではデフォルトで解決できない。testcontainers の `with_host("host.docker.internal", Host::HostGateway)` を設定して Linux 環境でも動作するようにする

nginx の WebSocket プロキシ設定テンプレート:

```nginx
server {
    listen 80;
    location / {
        proxy_pass http://host.docker.internal:{port};
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_read_timeout 10s;
    }
}
```

### issue 0024 との関係

issue 0024（curl を使った e2e テスト）は nginx を経由せず、curl から echo サーバーへ直接接続する設計。echo サーバーのコードは 0024 で独立して定義する（同等のコード）。nginx ヘルパーは 0024 では使用しない。

### テストケース

- テキストメッセージのエコー (nginx 経由)
- バイナリメッセージのエコー (nginx 経由)
- 大きなメッセージのエコー (nginx 経由、128 KiB。WebSocket フレームの 64bit 拡張ペイロード長エンコーディングが使われるサイズで、nginx のバッファリング動作を含めて検証する)
- Ping/Pong の往復 (nginx 経由。クライアントから Ping を送信し、`ConnectionEvent::Pong(data)` で受信した Pong ペイロードが送信した Ping ペイロードと一致することを検証する)
- Close ハンドシェイク (nginx 経由。`CloseCode::NORMAL` (1000) で Close を送信し、サーバーの自動 Close 応答のステータスコードが 1000 であることを検証する。なお、サーバーの自動応答では reason は常に空文字になる)

### 依存関係

`examples/websocket_e2e_nginx/Cargo.toml`:

```toml
[package]
name = "websocket_e2e_nginx"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
# WebSocket クライアント・サーバー本体
shiguredo_websocket.workspace = true
# masking key / nonce 用の暗号論的乱数生成に使用する
aws-lc-rs.workspace = true
# 非同期ランタイム (echo サーバーの TCP 処理に使用する)
tokio = { version = "1.52", features = ["macros", "rt-multi-thread", "net", "io-util", "time"] }

[dev-dependencies]
# Docker コンテナ起動 (実 nginx 相手の e2e テスト)
testcontainers = { version = "0.27", default-features = false, features = ["aws-lc-rs"] }
```

### CI への影響

`cargo test --workspace` に新しいテストクレートが含まれるため、CI 環境で Docker が必要になる。GitHub Actions の Ubuntu ランナーには Docker がプリインストールされている。macOS ランナー (`macos-26`) には Docker がプリインストールされていないため CI 修正が必要。

現在の CI は matrix 戦略で全 OS 共通の `cargo test --workspace` を実行している。macOS のみ除外するには、テストステップを OS で条件分岐する:

```yaml
- run: cargo test --workspace
  if: runner.os != 'macOS'
- run: cargo test --workspace --exclude websocket_e2e_nginx
  if: runner.os == 'macOS'
```

### 後方互換への影響

なし。新規テストクレートの追加のみ。

### CHANGES.md

テスト追加は機能に直接影響しないため、`## develop` の `### misc` セクションに記載する。

## 完了条件

- `examples/websocket_e2e_nginx/` が workspace メンバーとして追加されている
- Docker が利用可能な環境で `cargo test -p websocket_e2e_nginx` が全て通る
- Docker が利用不可の場合は即座に失敗する (`#[ignore]` は使わない)
- テキスト・バイナリ・大サイズメッセージ・Ping/Pong・Close のテストが存在する
- CI の macOS ステップで `websocket_e2e_nginx` を除外する修正が入っている

## 解決方法

設計方針どおりに `examples/websocket_e2e_nginx/` を新規 workspace メンバーとして追加した。

### 追加したファイル

- `examples/websocket_e2e_nginx/Cargo.toml`: クレート定義
- `examples/websocket_e2e_nginx/src/lib.rs`: Sans I/O パターンの echo サーバー (`spawn_echo_server`)
- `examples/websocket_e2e_nginx/tests/helpers/mod.rs`: testcontainers ベースの nginx WebSocket プロキシヘルパー
- `examples/websocket_e2e_nginx/tests/nginx_websocket.rs`: 5 つの e2e テスト

### 変更したファイル

- `Cargo.toml`: workspace members に `examples/websocket_e2e_nginx` を追加
- `.github/workflows/ci.yml`: macOS ランナーで Docker を必要とする e2e テストクレートを除外

### テストケース

- テキストメッセージのエコー (nginx 経由)
- バイナリメッセージのエコー (nginx 経由)
- 大きなメッセージのエコー (nginx 経由、128 KiB)
- Ping/Pong の往復 (nginx 経由)
- Close ハンドシェイク (nginx 経由)
