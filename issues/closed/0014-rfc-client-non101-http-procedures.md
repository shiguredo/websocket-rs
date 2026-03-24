# クライアント側が 101 以外の応答を HTTP procedures として扱わず即失敗にしている

Created: 2026-03-25
Model: Opus 4.6

## 優先度

P2

## 概要

クライアント側の `validate_response()` で、101 以外のステータスコードを一律 `handshake_rejected` エラーにしている。RFC が要求する「HTTP procedures に従った処理」に必要な情報 (ヘッダー、ボディ等) が破壊され、上位層が HTTP 的な処理を行うことが不可能になっている。

## RFC 根拠

RFC 6455 Section 4.1 (refs/rfc6455.txt:1030):

> If the status code received from the server is not 101, the client handles the response per HTTP [RFC2616] procedures. In particular, the client might perform authentication if it receives a 401 status code; the server might redirect the client using a 3xx status code (but clients are not required to follow them), etc.

## 現在の挙動

`src/websocket_handshake.rs:520` でステータスコードと reason phrase をフォーマット文字列に埋め込んで `handshake_rejected` エラーを返す。`401` の `WWW-Authenticate` ヘッダー、`3xx` の `Location` ヘッダー、レスポンスボディ等は全て失われる。

ステータスコード文字列をエラー文言に埋め込むだけでは、HTTP procedures を扱える API とは言えない。

## 修正方針

101 以外のレスポンスを受けた場合に、ステータスコード・ヘッダー・ボディを構造化した形で返す専用のエラーバリアントを設ける。上位層がレスポンス情報に基づいてリダイレクト・認証等の HTTP 的処理を実行できるようにする。

## 解決方法

1. `ErrorKind::HttpResponse` バリアントを追加
2. `HttpResponseInfo` 構造体を追加（`status_code`, `reason_phrase`, `headers` を保持）
3. `Error` に `http_response: Option<HttpResponseInfo>` フィールドを追加
4. `validate_response()` で 101 以外のレスポンスを受けた場合に `Error::http_response()` を使い、HTTP レスポンスの構造化情報を保持したエラーを返すようにした

これにより上位層は `error.kind == ErrorKind::HttpResponse` で判別し、`error.http_response` から `Location` ヘッダー (3xx) や `WWW-Authenticate` ヘッダー (401) 等を取得して HTTP procedures を実行できる。

Completed: 2026-03-25
