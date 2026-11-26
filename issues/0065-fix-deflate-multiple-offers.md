# 複数の permessage-deflate offer から正しく選択する

- Priority: Medium
- Created: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-deflate-multiple-offers

## 目的

クライアントが `permessage-deflate; server_max_window_bits=8, permessage-deflate; server_max_window_bits=15` のように複数の offer を送ると、`accept_handshake_auto` が 2 番目の offer を選んでも、検証は 1 番目の 8 と比較して失敗する。RFC 7692 Section 7.1.3 は、このようなフォールバック付きの offer を例示しており、サーバーは任意の 1 つを選んで受理できる。

## 優先度根拠

Medium。RFC が例示する offer 形式でハンドシェイクが失敗し、相互接続性が損なわれる。

## 現状

- `src/websocket_server_connection.rs` の `select_deflate` は、受理できる最初の offer を選んで `PerMessageDeflateConfig::negotiate` に渡す。
- 同じファイルの `negotiate_deflate` は、応答の `server_max_window_bits` を `request.extensions` の最初に見つかった `server_max_window_bits` と比較する。
- 1 番目の offer が `8`、2 番目が `15` の場合、`select_deflate` は 1 番目をスキップして 2 番目を選ぶが、`negotiate_deflate` は 1 番目の 8 と比較するため `HandshakeRejected` になる。

## 設計方針

応答の `server_max_window_bits` を、offer 全体の最大値と比較する。値を持たない offer は 15 とみなす。これにより、応答がいずれかの offer の制約を満たす場合に受理される。`client_max_window_bits` の未 offer 検査は、既存どおり offer 全体の存在で評価する。

## CHANGES.md 登録内容

```markdown
- [FIX] 複数の permessage-deflate offer から正しく選択する
  - @voluntas
```

## 完了条件

- `smwb=8` と `smwb=15` の 2 offer で `accept_handshake_auto` が成功し、接続が `Connected` になる。
- 応答の `Sec-WebSocket-Extensions` が `server_max_window_bits=15` を含む。
- 単一 offer の挙動が変わらない。
- `cargo test --workspace` が通る。
