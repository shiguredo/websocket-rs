# nginx + testcontainers を使った WebSocket e2e テストを追加する

- Priority: Medium
- Created: 2026-05-26
- Model: Opus 4.7
- Branch: feature/add-nginx-e2e-test

## 目的

shiguredo_websocket の Sans I/O クライアント・サーバー実装が、実際の WebSocket プロキシ (nginx) を経由しても正しく動作することを検証する。現在の PBT や fuzzing は protocol 層の正しさを保証しているが、実際のプロキシサーバーを経由した e2e 通信テストは存在しない。nginx は WebSocket プロキシとして広く使われており、相互運用性を担保する上で重要な検証対象である。

## 優先度根拠

ライブラリとしての正常動作は PBT で検証済みだが、実際のミドルウェアを挟んだ e2e テストがないと本番環境での動作保証が弱い。http11-rs の examples/http11_client で同様のパターン (testcontainers + nginx) が既に確立されており、それを参考にすることで効率的に構築できる。

## 現状

- `examples/` には `websocket_client`、`websocket_server`、`websocket_reverse_proxy` が存在する
- テストは `pbt/` (proptest) と `fuzz/` (libfuzzer) のみで、実サーバーを使った e2e テストは存在しない
- http11-rs の `examples/http11_client` に testcontainers + nginx のパターンが確立済み

## 設計方針

- `examples/websocket_e2e_nginx/` を新規 workspace メンバーとして追加する
- http11-rs の `examples/http11_client/tests/helpers/mod.rs` のパターンを踏襲する
- テスト内で以下を起動する:
  1. shiguredo_websocket ベースの echo サーバーを tokio タスクとしてランダムポートで起動する
  2. nginx を testcontainers で起動し、WebSocket プロキシ設定で echo サーバーへ proxy_pass する
- shiguredo_websocket クライアントから nginx 経由で echo サーバーに接続し、各種シナリオを検証する

### nginx の WebSocket プロキシ設定

```nginx
server {
    listen 80;
    location / {
        proxy_pass http://host.docker.internal:<PORT>;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}
```

### テストケース

- テキストメッセージのエコー (nginx 経由)
- バイナリメッセージのエコー (nginx 経由)
- 大きなメッセージのエコー (nginx 経由、バッファリング確認)
- Close ハンドシェイク (nginx 経由)

### 依存関係

- testcontainers 0.27 (dev-dependencies、aws-lc-rs feature)
- tokio 1.52 (macros, rt-multi-thread, net, io-util, time)
- shiguredo_websocket (workspace)
- aws-lc-rs (workspace、masking key / nonce 用)
- shiguredo_http11 (workspace、ハンドシェイク HTTP 処理用)

## 完了条件

- `examples/websocket_e2e_nginx/` が workspace メンバーとして追加されている
- Docker が利用可能な環境で `cargo test -p websocket_e2e_nginx` が全て通る
- Docker が利用不可の場合は即座に失敗する (`#[ignore]` は使わない)
- テキスト・バイナリ・大サイズメッセージ・Close のテストが存在する

## 解決方法

(実装後に記載)
