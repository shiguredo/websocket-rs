# サーバー側が不正なハンドシェイクに対して HTTP エラーレスポンスを返していない

Created: 2026-03-25
Model: Opus 4.6

## 優先度

P1

## 概要

サーバー側の `receive()` 内で、クライアントの不正なハンドシェイクを検出した際に、`VersionNotSupported` (426) だけは HTTP エラーレスポンスを返しているが、それ以外のバリデーション失敗は `Err` を返すのみで HTTP レスポンスを生成しない。

## RFC 根拠

RFC 6455 Section 4.2.1 (refs/rfc6455.txt:1139):

> the server MUST stop processing the client's handshake and return an HTTP response with an appropriate error code (such as 400 Bad Request).

MUST 要件であり、例外規定はない。

## 現在の挙動

`src/websocket_server_connection.rs:770` の `VersionNotSupported` 分岐のみ `reject_handshake(426, ...)` を呼び、`src/websocket_server_connection.rs:775` の一般エラー分岐は `Err(e)` をそのまま返す。

426 を自動送出している時点で、ライブラリ自身がハンドシェイク違反に対する HTTP エラー応答の責務を引き受けている。同じ match 文内で一般エラーだけ Sans I/O を理由に免責する整合性はない。

## 修正方針

`Err(e)` の一般分岐でも `reject_handshake(400, "Bad Request", &[])` を送出してから `Err(e)` を返す。

## 解決方法

`src/websocket_server_connection.rs` の `process_handshake()` 内、`Err(e)` の一般分岐で `reject_handshake(400, "Bad Request", &[])` を呼び出してから `Err(e)` を返すようにした。これにより `VersionNotSupported` (426) と同様に、全てのハンドシェイクバリデーション失敗で HTTP エラーレスポンスが送出される。

Completed: 2026-03-25
