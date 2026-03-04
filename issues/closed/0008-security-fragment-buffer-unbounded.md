# 0008 フラグメント再構成バッファが無制限でメモリ DoS が可能

## 分類

security / P1

## 概要

`FragmentBuffer::append()` に上限がなく、継続フレームを無制限に蓄積できる。
`max_decompressed_size` は解凍後サイズの制限であり、フラグメント蓄積自体を抑止しない。

## 影響範囲

- `src/websocket_client_connection.rs:249` (`FragmentBuffer::append`)
- `src/websocket_server_connection.rs:121` (同上)
- `src/websocket_client_connection.rs:925` (継続フレーム処理)
- `src/websocket_server_connection.rs:847` (同上)

## 再現手順

1. WebSocket 接続を確立する
2. 大きなペイロードを細切れの継続フレーム (FIN=0) として送り続ける
3. `FragmentBuffer` の `payload` が無制限に増大してメモリを枯渇させる

## 対応方針

`ClientConnectionOptions` / `ServerConnectionOptions` に `max_message_size: usize` を追加する。

- デフォルト値: 64MB (`64 * 1024 * 1024`)
- `FragmentBuffer` に最大サイズを持たせ、`start()` / `append()` でチェックする
- 超過時は `CloseCode::MESSAGE_TOO_BIG` (1009) で Close を送信してエラーを返す
- クライアント側・サーバー側の両 `handle_continuation()` で対応する

## 備考

RFC 6455 Section 10.7 より「実装はメッセージサイズ上限を課すべき (SHOULD)」。
`1009 Message Too Big` クローズコードが存在することからも、これはプロトコル設計の想定範囲内。

## 解決方法

`ClientConnectionOptions` と `ServerConnectionOptions` に `max_message_size: usize`（デフォルト 64MB）を追加。

- `DEFAULT_MAX_MESSAGE_SIZE` 定数を両ファイルに定義
- 各オプションに `max_message_size` フィールドと `max_message_size()` ビルダーメソッドを追加
- `FragmentBuffer` に `len()` メソッドを追加
- `handle_data_frame()` の非 FIN ケースで最初のフラグメントサイズをチェック
- `handle_continuation()` で `append()` 後に累積サイズをチェック
- 超過時は `CloseCode::MESSAGE_TOO_BIG` (1009) で Close を送信し `Error` を返す
