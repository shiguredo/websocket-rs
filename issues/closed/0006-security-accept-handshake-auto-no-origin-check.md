# 0006 accept_handshake_auto() が Origin / path を検証しない

## 分類

security / P2

## 概要

`WebSocketServerConnection::accept_handshake_auto()` は `ServerHandshakeRequest` が保持する
`origin` および `path` を評価せず、無条件にハンドシェイクを受諾する。

## 影響範囲

- `src/websocket_server_connection.rs:253` (`accept_handshake_auto`)
- `src/websocket_handshake.rs` の `ServerHandshakeRequest` が保持する `path` / `origin`

## 問題の詳細

ブラウザ + Cookie 認証の運用において `accept_handshake_auto()` を使用すると、
CSWSH (Cross-Site WebSocket Hijacking) の踏み台になる可能性がある。

RFC 6455 Section 10.2 では、サーバーは `Origin` ヘッダーを検証することを推奨している。

## 再現条件

1. ブラウザクライアントが Cookie 付きで WebSocket 接続を試みる
2. サーバーが `accept_handshake_auto()` を使用して認証なしに受諾する
3. 攻撃者が悪意のあるページから接続を誘導できる

## 対応方針

コード変更は最小限とし、ドキュメントコメントに以下を明記する。

- この関数は `origin` と `path` の検証を行わない
- ブラウザ + Cookie 認証環境では `accept_handshake()` を使い、
  `ServerHandshakeRequest` の `origin` / `path` を自前で検証すること
- 信頼できない接続元に対してこの関数を使用してはならない

## 備考

Sans I/O ライブラリとして認可ロジックをライブラリ内に持つことは設計の越権であるため、
ライブラリ側での強制検証は行わない。

## 解決方法

`accept_handshake_auto()` のドキュメントコメントに以下を追記した。

- `origin` / `path` を検証しない旨
- ブラウザ + Cookie 認証環境での CSWSH リスク
- 信頼できないクライアントには `pending_request()` + `accept_handshake()` を使うよう案内
