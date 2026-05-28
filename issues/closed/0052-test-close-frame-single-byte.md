# Close フレームのペイロード長 1 バイトのエラーパスを接続層 PBT で検証する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/add-close-frame-single-byte-pbt

## 目的

RFC 6455 Section 5.5.1 に基づき、Close ペイロード長 1 バイトを **接続処理**（`handle_close`）で `ProtocolViolation` 等として検出する PBT を追加する。

## 優先度根拠

Medium。`prop_close_frame_single_byte_payload` は `FrameDecoder` のみで「デコード成功」を見ており、アプリケーション層の拒否を検証していない。

## 現状

- `src/websocket_connection_shared.rs:455-464`: ペイロード長 1 でエラー
- `pbt/tests/prop_violations.rs:216-240`: デコーダ層のみ（意図的に `result.is_ok()`）

## 設計方針

`pbt/tests/prop_violations.rs`（または `prop_client_connection.rs`）に、ハンドシェイク完了済み接続へ 1 バイト Close フレームを送り、`poll_event` でエラーまたは Close 処理結果を検証するテストを追加。既存 `prop_close_frame_single_byte_payload` はデコーダ層のまま残すか、コメントで役割分担を明記。

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] Close フレーム 1 バイトペイロードの接続層エラーを PBT で検証する
  - @<GitHub ハンドル>
```

## 完了条件

- 接続層で 1 バイト Close がエラーになる PBT がある
- `cargo test --workspace` が全件パスする

## 解決方法

`pbt/tests/prop_server_connection.rs` の末尾に `prop_close_frame_single_byte_payload_at_application_layer` を新規追加した。

ハンドシェイク完了済みサーバー接続 (`setup_connected_server`) に対して、任意の `payload_byte` と `mask_key` で構築した 1 バイト Close フレームを `feed_recv_buf` に渡し、以下を proptest で検証する:

- `feed_recv_buf` が `Err(ErrorKind::ProtocolViolation)` を返すこと
- 返送される応答 Close フレームの code が `CloseCode::PROTOCOL_ERROR (1002)` であること

既存 `pbt/tests/prop_violations.rs:202` の `prop_close_frame_single_byte_payload` はデコーダ層 (`FrameDecoder`) でのデコード成功を検証する役割でそのまま残し、接続層 (`handle_close`) のエラー検証を新規 PBT で補完する形になった。`cargo test --workspace` 全件パス。
