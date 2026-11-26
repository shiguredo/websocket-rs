# 未分割メッセージにも max_message_size を適用する

- Priority: Medium
- Created: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-unfragmented-message-size-limit

## 目的

`ClientConnectionOptions::max_message_size` / `ServerConnectionOptions::max_message_size` はメッセージサイズの上限として公開されているが、単一フレームで送られたメッセージには適用されない。上限を小さく設定しても、`max_frame_size` までの大きさのメッセージが 1 フレームで通ってしまう。RFC 6455 Section 10.4 は、実装がフレームサイズと再構成後のメッセージサイズに上限を課すことを SHOULD としている。

## 優先度根拠

Medium。利用者が設定したメッセージサイズ上限が、分割の有無という相手側の都合で無効化される。

## 現状

- `src/websocket_connection_shared.rs` の `handle_data_frame` は、`frame.fin` が真の経路で `max_message_size` を検査せずに `emit_message` を呼ぶ。
- 同じ関数の `fin` が偽の経路と、`handle_continuation` の累積検査だけが `max_message_size` を検査する。

## 設計方針

`frame.fin` の経路でも、解凍前のペイロード長を `max_message_size` と比較し、超過時は `CloseCode::MESSAGE_TOO_BIG` を送ってエラーにする。解凍後のサイズは既存の `max_decompressed_size` が守る。分割メッセージの挙動は変えない。

## CHANGES.md 登録内容

```markdown
- [FIX] 未分割メッセージにも max_message_size を適用する
  - @voluntas
```

## 完了条件

- `max_message_size = 10`、`max_frame_size = 1000` で 100 バイトの未分割 Text / Binary が拒否される。
- 10 バイト以下の未分割メッセージは受理される。
- 分割メッセージの上限検査が従来どおり動作する。
- `cargo test --workspace` が通る。
