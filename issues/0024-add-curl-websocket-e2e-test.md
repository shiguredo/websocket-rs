# curl を使った WebSocket e2e テストを追加する

- Priority: Medium
- Created: 2026-05-26
- Model: Opus 4.7
- Branch: feature/add-curl-e2e-test

## 目的

shiguredo_websocket のサーバー実装が、curl (異なる WebSocket クライアント実装) からの接続を正しく処理できることを検証する。自前のクライアント・サーバー間テストだけでは実装バイアスを検出できないため、独立した WebSocket クライアント実装との相互運用性を確認する必要がある。

## 優先度根拠

WebSocket ライブラリとして公開するにあたり、自前実装同士のテストだけでは不十分。curl は広く普及したツールであり、curl の WebSocket 実装 (7.86 以降) との相互運用性を確認することで、仕様準拠の信頼性が向上する。nginx の e2e テスト (issue 0023) と合わせて、外部実装との互換性を包括的に検証できる。

## 現状

- 現在のテストは全て shiguredo_websocket 内部のクライアント・サーバーを使った自己完結型テスト
- 外部の WebSocket クライアント実装との相互運用性テストは存在しない
- curl 7.86 以降で `--ws` フラグによる WebSocket 接続がサポートされている

## 設計方針

- `examples/websocket_e2e_curl/` を新規 workspace メンバーとして追加する
- テスト内で以下を起動する:
  1. shiguredo_websocket ベースの echo サーバーを tokio タスクとしてランダムポートで起動する
  2. nginx を testcontainers で起動し、WebSocket プロキシ設定で echo サーバーへ proxy_pass する
  3. curl を `std::process::Command` で実行し、nginx 経由で WebSocket 接続する

### curl の WebSocket テスト方法

curl の `--ws` モードは stdin/stdout ベースで対話的だが、テストでは以下のアプローチを取る:

- ハンドシェイク検証: curl の `--include` + HTTP Upgrade ヘッダーを手動設定して、サーバーの 101 レスポンスを検証する
- エコー検証: curl の `--ws` モードに stdin からデータをパイプし、stdout のレスポンスを検証する

### テストケース

- WebSocket ハンドシェイクレスポンスの検証 (101 Switching Protocols)
- curl --ws を使ったテキストメッセージのエコー (nginx 経由)

### 依存関係

- testcontainers 0.27 (dev-dependencies、aws-lc-rs feature)
- tokio 1.52 (macros, rt-multi-thread, net, io-util, time, process)
- shiguredo_websocket (workspace)
- aws-lc-rs (workspace、masking key / nonce 用)

## 完了条件

- `examples/websocket_e2e_curl/` が workspace メンバーとして追加されている
- Docker と curl (7.86 以降) が利用可能な環境で `cargo test -p websocket_e2e_curl` が全て通る
- Docker または curl が利用不可の場合は即座に失敗する (`#[ignore]` は使わない)
- ハンドシェイク検証とエコー検証のテストが存在する

## 解決方法

(実装後に記載)
