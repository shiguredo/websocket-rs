# 複数の permessage-deflate offer から正しく選択する

- Priority: Medium
- Created: 2026-11-27
- Polished: 2026-11-27
- Completed: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-deflate-multiple-offers

## 目的

クライアントが `permessage-deflate; server_max_window_bits=8, permessage-deflate; server_max_window_bits=15` のように複数の offer を送ると、`accept_handshake_auto` が 2 番目の offer を選んでも、検証は 1 番目の 8 と比較して失敗する。RFC 7692 Section 7.1.3 は、このようなフォールバック付きの offer を例示しており、サーバーは任意の 1 つを選んで受理できる。

## 優先度根拠

Medium。RFC が例示する offer 形式でハンドシェイクが失敗し、相互接続性が損なわれる。

## 現状

- `src/websocket_server_connection.rs` の `select_deflate` は、受理できる最初の offer を選んで `PerMessageDeflateConfig::negotiate` に渡す。noflate は window_bits=15 固定のため、`server_max_window_bits < 15` の offer はスキップされる。
- 同じファイルの `negotiate_deflate` は、応答の `server_max_window_bits` を `request.extensions` の最初に見つかった `server_max_window_bits` とだけ比較する。
- `client_max_window_bits` は offer 全体のいずれかに存在するかだけを見ており、応答の各パラメータを同じ offer に紐付けて検証していない。応答が `server_max_window_bits` を省略した場合も、offer の値との比較を行わない。
- 1 番目の offer が `smwb=8`、2 番目が `smwb=15` の場合、`select_deflate` は 1 番目をスキップして 2 番目を選ぶが、`negotiate_deflate` は 1 番目の 8 と比較するため `HandshakeRejected` になる。

## 設計方針

`negotiate_deflate` を、パラメータごとの集約ではなく offer ごとの全条件判定に変更する。各 permessage-deflate offer について次を調べ、1 つでも満たす offer があれば受理する。

- offer のパースには 0060 で追加する共通の厳密パーサ (`parse_window_bits`) または `from_extension_for_server_request` を使う。パースに失敗した offer は候補から除外する。
- 応答の実効 `server_max_window_bits` (応答に無ければ 15) が、offer の実効値 (offer に無ければ 15) 以下であること。
- 応答に `client_max_window_bits` がある場合、その offer にも `client_max_window_bits` があること。offer の値はヒントであり、応答の値がそれを超えても RFC 7692 Section 7.1.2.2 の MUST には反しない。
- 応答の `server_max_window_bits` は noflate の制約で 15 のみとする (既存)。

どの offer も満たさない場合は `HandshakeRejected` とする。有効な offer が 0 件の状態で応答に `permessage-deflate` が含まれる場合も `HandshakeRejected` とする (0060 の完了条件と同じ)。0065 は 0060 の後に積み、0060 の変更を前提とする。

`select_deflate` が選んだ offer から組み立てた応答は、この判定を満たす。手動 `accept_handshake` の応答も同じ判定を通る。

## CHANGES.md 登録内容

```markdown
- [FIX] 複数の permessage-deflate offer から正しく選択する
  - @voluntas
```

## 完了条件

- `tests/test_websocket_server_connection.rs`（既存）に次のテストがある。
  - offers `permessage-deflate; server_max_window_bits=8, permessage-deflate; server_max_window_bits=15` と `ServerConnectionOptions::new().deflate(...)` で `accept_handshake_auto` が `Connected` になり、応答に `server_max_window_bits=15` を含む。
  - 同じ offers に対する手動 `accept_handshake` で応答 `permessage-deflate; server_max_window_bits=15` が `Connected` になる。
  - offer 1 が `server_max_window_bits=8; client_max_window_bits`、offer 2 が `server_max_window_bits=15` のとき、応答 `permessage-deflate; server_max_window_bits=15; client_max_window_bits=15` が `HandshakeRejected` になる (どの単一 offer も満たさない)。
  - 単一 offer `server_max_window_bits=15` は `Connected` になり、応答に `server_max_window_bits=15` を含む。
  - 単一 offer `server_max_window_bits=8` は拡張不採用のまま `Connected` になり、応答に permessage-deflate を含まない。
  - 単一 offer `permessage-deflate` (smwb なし) は `Connected` になり、応答に `server_max_window_bits` を含まない。
  - `cmwb` を含む offer に自動応答で `client_max_window_bits=15` を返して `Connected` になる。
  - `cmwb` を含む offer に `cmwb` なしの手動応答を返して `Connected` になる。
  - 応答 `server_max_window_bits=8` はサポート外として `HandshakeRejected` になる。
  - offer `server_max_window_bits=+8` と応答 `permessage-deflate` は `HandshakeRejected` になる (既存テストで確認)。
- `make test`（実体は `cargo test --workspace --exclude websocket_client`）が通る。nginx e2e テストは Docker が必要なため GitHub Actions で実行する。

## 解決方法

`src/websocket_server_connection.rs` の `negotiate_deflate` を、offer 全体の集約ではなく offer ごとの判定に変更した。各 permessage-deflate offer について、応答の実効 `server_max_window_bits` (無ければ 15) が offer の実効値以下であり、応答に `client_max_window_bits` がある場合は同じ offer にもあることを確認し、1 つでも満たす offer があれば受理する。どの offer も満たさない場合は `HandshakeRejected` とする。

`tests/test_websocket_server_connection.rs` に、8 → 15 のフォールバック offer の自動応答・手動応答、cmwb を含む混合 offer の拒否、cmwb を含む offer の受理 (自動応答・手動応答)、単一 15 / 8 / smwb なしの offer、応答 `server_max_window_bits=8` の拒否のテストを追加した。符号付き `+8` の回帰は既存テストで確認した。

`make test` が通る (27 個のテストターゲットすべて ok)。
