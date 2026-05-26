# curl を使った WebSocket e2e テストを追加する

- Priority: Medium
- Created: 2026-05-26
- Polished: 2026-05-26
- Model: Opus 4.7
- Branch: feature/add-curl-e2e-test

## 目的

shiguredo_websocket のサーバー実装が、curl（独立した WebSocket クライアント実装）からの接続を正しく処理できることを検証する。自前のクライアント・サーバー間テストだけでは実装バイアスを検出できないため、独立したクライアント実装との相互運用性を確認する。

## 優先度根拠

自前実装同士のテスト (issue 0023) だけでは、両者が同じ RFC 違反をしていてもテストが通ってしまう。curl は広く普及したツールであり、curl の WebSocket 実装との相互運用性を確認することで仕様準拠の信頼性が向上する。

## 現状

- 現在のテストは全て shiguredo_websocket 内部のクライアント・サーバーを使った自己完結型テスト
- 外部の WebSocket クライアント実装との相互運用性テストは存在しない

## 設計方針

### curl の WebSocket サポート

curl は `ws://` / `wss://` URL スキームで WebSocket 接続をサポートしている。ただし WebSocket サポートはビルド時に有効化が必要な機能であり、OS 標準の curl では無効な場合が多い（macOS の curl 8.7.1 では `ws://` が未サポート）。

テストの再現性と CI での安定動作を保証するため、Docker イメージ `curlimages/curl` を使用する。このイメージは全プロトコルを有効化してビルドされており、`ws://` / `wss://` をサポートしている（Protocols に `ws wss` が含まれることを確認済み）。

curl の WebSocket 利用方法:
- `ws://` URL スキームを指定すると、curl が自動で WebSocket ハンドシェイク（HTTP Upgrade）を送信する
- ハンドシェイク成功（101 Switching Protocols）後、stdin からの入力が WebSocket フレームとして送信され、受信フレームのペイロードが stdout に出力される
- `--include` オプションで 101 レスポンスヘッダーを stdout に含めることができる

### テスト構成

nginx を経由しない。curl テストの目的は「外部クライアントとの相互運用性検証」であり、プロキシとの互換性検証は issue 0023 が担当する。nginx を挟むと問題の切り分けが困難になるため、curl は echo サーバーに直接接続する。

ただし Docker コンテナ内の curl からホスト上の echo サーバーに接続する必要があるため、`host.docker.internal` を使用する。Linux の Docker Engine ではデフォルトで `host.docker.internal` が解決できないため、`--add-host host.docker.internal:host-gateway` オプションを付与する。

### プロジェクト構成

`examples/websocket_server/tests/` に e2e テストを追加する。curl は外部クライアントとしてサーバーに接続するものであり、`websocket_server` のテストとして配置するのが自然。

新規作成ファイル:

- `examples/websocket_server/tests/helpers/mod.rs` (echo サーバーと Docker / curl ヘルパー)
- `examples/websocket_server/tests/curl_websocket.rs` (テスト本体)

### echo サーバー

CLAUDE.md の「番号が小さい issues から順番に対応すること」に従い、0023 が先に完了する前提とする。echo サーバーのコードは 0023 と同等のものを `src/lib.rs` に独立して定義する（コード量が少ないため共通クレートへの切り出しは不要）。

echo サーバーの設計は 0023 に準ずる:
- `TcpListener::bind("0.0.0.0:0")` でランダムポートにバインド（Docker コンテナから `host.docker.internal` 経由でアクセスするため、ループバック `127.0.0.1` ではなく全インターフェースにバインドする）
- `ServerConnectionOptions::new().ping_interval(0)` で ping を無効化
- Sans I/O ループでテキスト・バイナリをエコーバック
- Close 応答はライブラリが自動送信

### curl コンテナの実行

テストコード内で `std::process::Command` を使い `docker run` で curl を実行する。testcontainers は使用しない（curl は長時間稼働するサービスではなく、コマンド実行のため `std::process::Command` が適切）。

```rust
// ハンドシェイク検証の例
let output = std::process::Command::new("docker")
    .args([
        "run", "--rm",
        "--add-host", "host.docker.internal:host-gateway",
        "curlimages/curl:8.12.0",
        "--include",
        "--max-time", "5",
        &format!("ws://host.docker.internal:{}/", port),
    ])
    .output()
    .expect("failed to execute docker");
```

```rust
// エコー検証の例
let mut child = std::process::Command::new("docker")
    .args([
        "run", "--rm", "-i",
        "--add-host", "host.docker.internal:host-gateway",
        "curlimages/curl:8.12.0",
        "--max-time", "5",
        "-N",
        &format!("ws://host.docker.internal:{}/", port),
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("failed to spawn docker");
```

### ヘルパー関数

`tests/helpers/mod.rs` に以下を定義する:

- `ensure_docker()`: Docker daemon の存在を確認し、不在なら panic する（`#[ignore]` は使わない）
- `ensure_curl_image()`: `curlimages/curl:8.12.0` イメージの pull を事前に行う。イメージが既にローカルにあればスキップする
- `docker_curl(args: &[&str]) -> std::process::Output`: docker run でcurl を実行する共通ヘルパー

### テストケース

- ハンドシェイク検証: `curl --include ws://host:port/` を実行し、stdout に含まれる 101 レスポンスを検証する。検証項目:
  - ステータスコードが 101 Switching Protocols
  - `Upgrade: websocket` ヘッダーの存在
  - `Connection: Upgrade` ヘッダーの存在
- テキストメッセージのエコー: stdin からテキストを送信し、stdout のレスポンスが送信データと一致することを検証する

テストケースが 2 つに留まる理由: curl の WebSocket サポートは `ws://` URL スキームによる基本的な接続・送受信のみであり、Ping/Pong フレームの明示的な送信、バイナリフレームの送信、Close コードの指定など細かな制御ができない。これらの詳細なプロトコル検証は issue 0023 の shiguredo_websocket クライアントを使ったテストが担当する。

### 依存関係

`examples/websocket_server/Cargo.toml` の既存依存に加え、追加の依存は不要。websocket_server は既に `shiguredo_websocket` と `tokio` に依存しており、echo サーバーの実装に必要な機能は揃っている。curl は `std::process::Command` で Docker 経由で実行するため外部クレートの追加も不要。

### issue 0023 との関係

- 0023: 自前クライアント + 自前サーバーが nginx 経由で動作するか（プロキシ互換性）
- 0024: 外部クライアント (curl) + 自前サーバーが直接通信できるか（相互運用性）
- echo サーバーのコードは同等だが、nginx ヘルパーは 0024 では使用しない
- CLAUDE.md の「番号が小さい issues から順番に対応すること」に従い、0023 が先に完了する前提

### CI への影響

`cargo test --workspace` に含まれるため、Docker が必要になる。macOS ランナーには Docker がないため除外が必要。issue 0025 の refactor で websocket_client が macOS 除外対象になっている前提:

```yaml
- run: cargo test --workspace
  if: runner.os != 'macOS'
- run: cargo test --workspace --exclude websocket_client --exclude websocket_server
  if: runner.os == 'macOS'
```

### 後方互換への影響

なし。新規テストクレートの追加のみ。

### CHANGES.md

テスト追加は機能に直接影響しないため、`## develop` の `### misc` セクションに記載する。

## 完了条件

- `examples/websocket_server/tests/curl_websocket.rs` にテストが存在する
- Docker が利用可能な環境で `cargo test -p websocket_server` が全て通る
- Docker が利用不可の場合は即座に失敗する (`#[ignore]` は使わない)
- ハンドシェイク検証とテキストエコーのテストが存在する
- CI の macOS ステップで `websocket_server` を除外する修正が入っている

## pending 理由

curl CLI の WebSocket モードでは stdin からのデータを WebSocket フレームとして送信する機能が **未実装** (curl 8.20.0 時点)。curl の計画では stdin/stdout 経由の WebSocket データ転送をサポートする予定だが、まだ開発が開始されていない。

- ハンドシェイク (101 Switching Protocols) までは動作する
- stdin → WebSocket テキストフレーム送信が未実装のため、テキストエコーテストが実現できない
- libcurl C API (`curl_ws_send`/`curl_ws_recv`) では可能だが、CLI ツールでは不可

curl CLI の WebSocket stdin/stdout 対応が実装されたタイミングで再開する。

## 解決方法
