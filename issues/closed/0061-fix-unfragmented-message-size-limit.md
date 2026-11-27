# 未分割メッセージにも max_message_size を適用する

- Priority: Medium
- Created: 2026-11-27
- Polished: 2026-11-27
- Completed: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-unfragmented-message-size-limit

## 目的

`ClientConnectionOptions::max_message_size` / `ServerConnectionOptions::max_message_size` は、公開 rustdoc では「フラグメント累積サイズ制限」と説明されているが、README と利用者向け SKILL では「最大メッセージサイズ」と説明されており、意味が揺れている。実装は分割メッセージの累積サイズだけを検査し、単一フレームで送られたメッセージには `max_message_size` を適用しない。そのため、`max_message_size < max_frame_size` の設定では、同じサイズのメッセージが分割の有無で受理されたり拒否されたりする。

RFC 6455 Section 10.4 は、実装固有の制限を持つ実装がその制限を超えないよう保護することを MUST とし、フレームサイズと再構成後のメッセージサイズに上限を課すことを SHOULD としている。本 issue は `max_message_size` を「分割の有無にかかわらず、受信ペイロード（圧縮時は圧縮後）の累積サイズ上限」と定義し、未分割メッセージにも一貫して適用する。

## 優先度根拠

Medium。利用者が設定したメッセージサイズ上限が、分割の有無という相手側の都合で無効化される。

## 現状

- `src/websocket_connection_shared.rs` の `handle_data_frame` は、`frame.fin` が真の経路で `max_message_size` を検査せずに `emit_message` を呼ぶ。
- 同じ関数の `fin` が偽の経路と、`handle_continuation` の累積検査だけが `max_message_size` を検査する。
- `ClientConnectionOptions::max_message_size` / `ServerConnectionOptions::max_message_size` のフィールド doc とビルダー doc は「フラグメント累積サイズ制限」と説明している。
- 圧縮メッセージでは、累積検査は圧縮後ペイロードの長さに掛かる。解凍後のサイズは `max_decompressed_size` が別に制限する。

## 設計方針

`max_message_size` の意味を「分割の有無にかかわらず、受信ペイロード（圧縮時は圧縮後）の累積サイズ上限」と定義する。解凍後のサイズ上限は既存の `max_decompressed_size` が担う。

- `handle_data_frame` の `frame.fin` 経路でも、解凍前のペイロード長を `max_message_size` と比較する。超過時は `CloseCode::MESSAGE_TOO_BIG` (1009) を送り、`ErrorKind::ProtocolViolation` を返す。
- 分割経路の累積検査と同じ比較 (`>`) を使い、上限ちょうどは受理する。
- `ClientConnectionOptions` / `ServerConnectionOptions` の `max_message_size` のフィールド doc とビルダー doc を新しい意味に更新する。
- 分割メッセージの挙動は変えない。

圧縮メッセージの扱いは、分割経路と同じく圧縮後ペイロードの長さで判定する。

## CHANGES.md 登録内容

```markdown
- [FIX] 未分割メッセージにも max_message_size を適用する
  - @voluntas
```

## 完了条件

- `max_message_size = 10`、`max_frame_size = 1000` のとき、未分割の Text と Binary について、10 バイトは受理され、11 バイトは `ErrorKind::ProtocolViolation` かつ Close 1009 で拒否される。テストは `tests/test_websocket_connection_shared.rs` の境界・エラーパスに追加する。
- permessage-deflate 有効時、未分割メッセージは圧縮後ペイロードのサイズで判定する。上限 20 バイトで 1000 バイトの `a` の圧縮後 18 バイトを受理し、上限 10 バイトで `hi` の圧縮後 14 バイトを Close 1009 で拒否する。解凍後サイズの上限は `max_decompressed_size` で別途確認する。
- 分割メッセージの既存 PBT `prop_fragment_flood_exceeds_max_message_size` が引き続き通る。
- `ClientConnectionOptions` / `ServerConnectionOptions` の `max_message_size` のフィールド doc とビルダー doc が新しい意味に更新されている。
- `make test`（実体は `cargo test --workspace --exclude websocket_client`）が通る。nginx e2e テストは Docker が必要なため GitHub Actions で実行する。

## 解決方法

`src/websocket_connection_shared.rs` の `handle_data_frame` の `fin` 経路で、解凍前の `frame.payload.len()` を `max_message_size` と比較し、超過時は `close_internal(CloseCode::MESSAGE_TOO_BIG, ...)` と `Error::protocol_violation` を返すようにした。分割経路と同じ `>` 比較で、上限ちょうどは受理する。

`ClientConnectionOptions` / `ServerConnectionOptions` の `max_message_size` のフィールド doc とビルダー doc を「受信メッセージの解凍前ペイロードサイズの上限（未分割・分割の両方に適用）」に更新した。

`tests/test_websocket_connection_shared.rs` に、Text / Binary の上限ちょうど受理、上限 +1 の拒否 (Close 1009)、圧縮メッセージの圧縮後サイズ判定のテストを追加した。圧縮メッセージは、上限 20 バイトで 1000 バイトの `a` の圧縮後 18 バイトを受理し (解凍後は上限超過)、上限 10 バイトで `hi` の圧縮後 14 バイトを拒否する (解凍後は上限以下)。これにより解凍前のサイズで判定していることを固定している。

`make test` が通る (25 個のテストターゲットすべて ok)。
