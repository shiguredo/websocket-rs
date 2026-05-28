# 0009 フレーム受信バッファが無制限でメモリ DoS が可能

## 分類

security / P1

## 概要

`FrameDecoder::feed()` が無制限に `Vec` へデータを追加し、
攻撃者が制御する巨大なペイロード長を `decode_with_info()` が受け入れる。
フレームが完成するまでバッファは増え続けるため、メモリ DoS が可能。

## 影響範囲

- `src/websocket_frame.rs:177` (`FrameDecoder::feed`)
- `src/websocket_frame.rs:209` (ペイロード長の受け入れ)
- `src/websocket_frame.rs:282` (payload の `to_vec()` による複製)
- `src/websocket_client_connection.rs:807` (`process_frames` から到達)
- `src/websocket_server_connection.rs:734` (同上)

## 再現手順

1. WebSocket 接続を確立する
2. ペイロード長フィールドを最大値 (2^63-1) に設定したフレームヘッダーのみを送信する
3. `FrameDecoder` はフレームが完成するまでバッファへの追加を続けるため、
   大量データを送り続けるとメモリを枯渇させる

## 対応方針

`ClientConnectionOptions` / `ServerConnectionOptions` に `max_frame_size: usize` を追加する。

- デフォルト値: 64MB (`64 * 1024 * 1024`)
- `FrameDecoder` 自体は変更せず、接続層 (`process_frames`) でフレームデコード後に
  ペイロードサイズをチェックする
- 超過時は `CloseCode::MESSAGE_TOO_BIG` (1009) で Close を送信してエラーを返す
- クライアント側・サーバー側の両 `handle_frame()` で対応する

## 備考

スキルドキュメントには「フレーム/メッセージサイズ制限（デフォルト 64MB）」と記載があるが、
実装には `max_decompressed_size` のみ存在し、フレームサイズ制限は未実装。
ドキュメントと実装の乖離を解消する必要もある。

RFC 6455 Section 10.4 より「実装はフレームサイズおよび再構成後のメッセージサイズに上限を課すべき (SHOULD)」。

## 解決方法

`ClientConnectionOptions` と `ServerConnectionOptions` に `max_frame_size: usize`（デフォルト 64MB）を追加。

- `DEFAULT_MAX_FRAME_SIZE` 定数を両ファイルに定義
- 各オプションに `max_frame_size` フィールドと `max_frame_size()` ビルダーメソッドを追加
- `handle_frame()` の先頭でコントロールフレーム以外に対してペイロードサイズをチェック
- 超過時は `CloseCode::MESSAGE_TOO_BIG` (1009) で Close を送信し `Error` を返す

注: バッファ成長の早期抑止（ヘッダー解析時点での拒否）は未対応。
真の早期拒否には `FrameDecoder` 自体へのサイズ制限追加が必要だが、
現時点ではフレーム受信後の検証で対応している。
