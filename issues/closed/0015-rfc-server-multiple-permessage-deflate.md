# サーバー側の accept_handshake() が permessage-deflate の複数要素を拒否しない

Created: 2026-03-25
Model: Opus 4.6

## 優先度

P2

## 概要

`accept_handshake()` は `response.extensions` に含まれる複数の `permessage-deflate` 要素を検証せず、そのまま `Sec-WebSocket-Extensions` ヘッダーに連結して送信できる。さらに内部では最後に見た設定で `deflate` を上書きするため、送信したレスポンスと実際の圧縮設定が不一致になる可能性がある。

## RFC 根拠

RFC 7692 Section 5:

> To accept the use of an offered PMCE, a server MUST include the extension name of the PMCE in the "Sec-WebSocket-Extensions" header field

RFC 7692 Section 7.1.3:

> The server can accept "permessage-deflate" by picking any supported one from the listed offers.

サーバーは offer の中から 1 つを選んで返す設計であり、複数の permessage-deflate 要素を含むレスポンスは不正。

RFC 7692 Section 7:

> A client MUST _Fail the WebSocket Connection_ if the peer server accepted an extension negotiation offer for this extension with an extension negotiation response meeting any of the following conditions

クライアント側実装 (`src/websocket_client_connection.rs:741`) はこの MUST に従い複数 permessage-deflate を正しく拒否しているため、サーバー側から不正なレスポンスを生成できること自体が問題。

## 現在の挙動

- `src/websocket_server_connection.rs:337`: `response.extensions` を順に処理するが permessage-deflate の重複チェックがない
- `src/websocket_server_connection.rs:401`: 複数要素をそのまま連結して `Sec-WebSocket-Extensions` ヘッダーに設定
- `src/websocket_server_connection.rs:425`: ループ内で `deflate` 変数を上書きし、最後の設定だけが有効になる

## 修正方針

`accept_handshake()` 内で `response.extensions` をパースした時点で `permessage-deflate` が 2 つ以上含まれている場合は `Err` を返す。クライアント側 (line 741-751) と同等のバリデーションをサーバー側にも追加する。

## 解決方法

`accept_handshake()` 内の拡張バリデーションループ直後に、`response.extensions` をパースして `permessage-deflate` 要素が 2 つ以上含まれる場合は `handshake_rejected` エラーを返すチェックを追加した。クライアント側と同等のバリデーション。

Completed: 2026-03-25
