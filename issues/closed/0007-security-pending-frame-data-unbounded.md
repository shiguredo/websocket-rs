# 0007 ハンドシェイク待機中バッファが無制限でメモリ DoS が可能

## 分類

security / P1

## 概要

ハンドシェイク要求を受理待ちの間、`pending_frame_data` に上限なくデータが蓄積される。
アプリケーションが `accept_handshake()` / `reject_handshake()` を遅延させるだけで、
接続確立前にサーバーのメモリを枯渇させられる。

## 影響範囲

- `src/websocket_server_connection.rs:706` (`process_handshake` の `pending_frame_data.extend_from_slice`)
- `src/websocket_server_connection.rs:719` (同上)

## 再現手順

1. TCP 接続後に有効な HTTP Upgrade リクエストを送信してハンドシェイクを完了させる
2. アプリケーションが `accept_handshake()` を呼ばない（または遅延させる）間、
   大量のデータを送り続ける
3. `pending_frame_data` が無制限に増大してメモリを枯渇させる

## 対応方針

`pending_frame_data` への蓄積に上限を設ける。

- 固定定数（例: `MAX_PENDING_FRAME_DATA: usize = 1 * 1024 * 1024`、1MB）を定義する
- `extend_from_slice` 前にサイズをチェックし、超過時はプロトコルエラーで接続を切断する
- エラーコードは `CloseCode::POLICY_VIOLATION` または `1009 Message Too Big` が適切

## 備考

`ServerConnectionOptions` に設定可能フィールドとして公開することも検討できるが、
まず定数による固定上限で対応し、必要に応じてオプション化する。

## 解決方法

`MAX_PENDING_FRAME_DATA: usize = 1024 * 1024`（1MB）の非公開定数を追加。

- `process_handshake()` の `pending_request.is_some()` ブランチで、
  `extend_from_slice` 前に `pending_frame_data.len() + buf.len() > MAX_PENDING_FRAME_DATA` をチェック
- 超過時は `Error::protocol_violation` を返して接続を切断する
