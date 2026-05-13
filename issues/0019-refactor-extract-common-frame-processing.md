# 0019: クライアント/サーバー間のフレーム処理ロジックの重複を解消する

Created: 2026-05-14
Model: deepseek-v4-flash

## 優先度

P1

## 概要

`WebSocketClientConnection` と `WebSocketServerConnection` の間でフレーム処理ロジックが
コピーペーストされており、400 行以上が完全に重複している。
バグ修正時に両方の修正が必要で、見落としが恒常的に発生する。

## 重複箇所一覧

以下のメソッド/構造体がほぼ完全に同一:

| メソッド/構造体 | client 行 | server 行 |
|---|---|---|
| `FragmentBuffer` | 248-294 | 123-169 |
| `handle_frame` | 881-934 | 847-900 |
| `handle_data_frame` | 936-962 | 902-928 |
| `handle_continuation` | 964-990 | 930-956 |
| `decompress_if_needed` | 993-1013 | 959-979 |
| `emit_message` | 1015-1044 | 981-1005 |
| `handle_close` | 1046-1130 | 1007-1091 |
| `handle_ping` | 1132-1145 | 1093-1106 |
| `handle_pong` | 1147-1160 | 1108-1121 |
| `close_internal` | 634-661 | 684-709 |
| `close` | 526-557 | 647-678 |
| `send_ping` | 504-519 | 625-640 |
| `send_data_frame` | 476-483 | 596-604 |
| `compress_if_enabled` | 486-499 | 607-620 |
| `set_state` | 607-613 | 760-766 |
| `check_connected` | 615-620 | 768-773 |
| `process_frames` | 847-866 | 818-837 |
| `handle_timer` | 560-593 | 712-746 |
| `DEFAULT_MAX_FRAME_SIZE` | 131 | 25 |
| `DEFAULT_MAX_MESSAGE_SIZE` | 134 | 28 |
| `DEFAULT_MAX_DECOMPRESSED_SIZE` | 137 | 31 |

client/server 間の差異は以下に限られる:
- `handle_decoded_frame`: マスク検証の方向 (client: `if decoded.masked`, server: `if !decoded.masked`)
- `send_frame`: client は encode + masking、server は encode_unmasked
- `close_internal` の戻り値型 (client: `Result<(), Error>`, server: `()`)
- `handle_frame` の `now` 引数の有無 (client のみ持つが未使用)

## 修正方針

共通フレーム処理を `trait WebSocketConnection` または共有モジュールに抽出する。

1. `FragmentBuffer` を独立モジュール (`src/fragment_buffer.rs`) に切り出す
2. `DEFAULT_MAX_*` 定数を `lib.rs` または `src/fragment_buffer.rs` の 1 箇所に集約する
3. 共通のフレーム処理メソッド郡を `trait WebSocketConnection` のデフォルト実装として提供する
4. client/server 固有の処理 (マスク検証方向、`send_frame` の実装) はトレイトメソッドで抽象化する
5. `close_internal` の戻り値型をサーバー側に合わせて `()` に統一する
