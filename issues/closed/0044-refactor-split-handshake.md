# websocket_handshake.rs を分割する

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: mimo-v2.5-pro
- Branch: feature/refactor-split-handshake

## 目的

`websocket_handshake.rs`（約 803 行）をリクエスト側 / レスポンス側に分割する。

## 優先度根拠

Low。挙動変更なし。`prop_handshake.rs` はファイル分割のみでテストロジックは維持。

## 現状

- `src/websocket_handshake.rs`: リクエストビルダー、バリデーター、レスポンス処理、共有ヘルパが混在
- `pbt/tests/prop_handshake.rs`: 1453 行（変更不要）

## 設計方針

1. `src/websocket_handshake_request.rs` — リクエストビルダー / リクエストバリデーション
2. `src/websocket_handshake_response.rs` — レスポンスバリデーション
3. `src/websocket_handshake.rs` — 共有: `WEBSOCKET_GUID`, `validate_extension_entry`, `validate_key`, `calculate_accept`, `calculate_accept_from_key`, `sha1_digest` 等

`0031` 完了後は `is_valid_token` は `crate::token::is_valid_token` を参照（ハンドシェイク内の重複定義は置かない）。

`calculate_accept_from_key` は `pub fn` のまま、`lib.rs` から再エクスポートを維持。

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] websocket_handshake.rs をリクエスト / レスポンスモジュールに分割する
  - @<GitHub ハンドル>
```

## 完了条件

- 分割が完了し、`cargo test --workspace` が全件パスする
- 公開 API に破壊的変更がない
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある

## 解決方法

`src/websocket_handshake.rs` (約 783 行) を 3 ファイルに分割した:

- `src/websocket_handshake_request.rs` (新規): `HandshakeRequest` / `ServerHandshakeRequest` / `HandshakeRequestValidator`
- `src/websocket_handshake_response.rs` (新規): `ServerHandshakeResponse` / `HandshakeResponse` / `HandshakeValidator`
- `src/websocket_handshake.rs` (縮小): `WEBSOCKET_GUID` 定数、`calculate_accept` / `calculate_accept_from_key` (pub) / `sha1_digest` (private) / `validate_extension_entry` / `validate_key` (pub(crate))

`src/lib.rs` で `mod websocket_handshake_request;` と `mod websocket_handshake_response;` を追加し、`pub use` 経路を新ファイルから再エクスポートするよう更新。`calculate_accept_from_key` は `pub fn` のまま `websocket_handshake.rs` に残し、`websocket_server_connection.rs` から参照経路を保つ。

`websocket_client_connection.rs` / `websocket_server_connection.rs` の use を新パスに更新。挙動変化なし、`cargo test --workspace` 全件パス。
