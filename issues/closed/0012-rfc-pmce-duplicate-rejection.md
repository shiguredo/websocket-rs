# 0012-rfc-pmce-duplicate-rejection

## 概要

クライアントがサーバーレスポンスに複数の `permessage-deflate` 要素が含まれている
場合に接続を失敗させず、最後の設定で上書きして継続してしまう。

## RFC 根拠

RFC 7692 (line 427-429):

> If a server gives an invalid response, such as accepting a PMCE that
> the client did not offer, the client MUST _Fail the WebSocket Connection_.

RFC 7692 (line 431-436):

> If a server responds with a valid PMCE element in the "Sec-WebSocket-
> Extensions" header and _the WebSocket Connection is established_,
> both endpoints MUST use the algorithm described in Section 6 and the
> message payload transformation procedure of the PMCE configured with
> the "agreed parameters" returned by the server to exchange messages.

「valid PMCE element」は単数で記述されており、サーバーは offer のリストから
1 つを選んで受諾する (RFC 7692 line 1015: "picking any supported **one**")。

RFC 7692 Section 6 (line 526-528):

> This document allocates the RSV1 bit of the WebSocket header for PMCEs

RSV1 ビットは PMCE に専用割り当てされており、複数の PMCE が有効な状態は
ビットの意味が曖昧になるため機能的に不正。

## 問題箇所

`src/websocket_client_connection.rs:739-814`

サーバーレスポンスの `Sec-WebSocket-Extensions` に複数の `permessage-deflate`
要素が含まれていた場合、ループで最後の設定が `self.deflate` に上書きされる。
不正なレスポンスとして Fail せず継続してしまう。

## 対応方針

クライアント側のサーバーレスポンス検証ループで `permessage-deflate` の
出現回数をカウントし、2 回以上出現した場合は `Error::handshake_rejected` を返す。

## 解決方法

`src/websocket_client_connection.rs` の `process_handshake_response()` 内、
既存の deflate 設定ループの手前に事前カウント処理を追加した。

`response.extensions` を走査して `permessage-deflate` の出現回数をカウントし、
2 回以上なら即座に `Error::handshake_rejected` を返す。
エラーの場合は無視して正常要素のみカウントする (`into_iter().flatten()` の利用)。
