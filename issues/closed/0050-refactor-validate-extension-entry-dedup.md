# validate_extension_entry と parse_strict の役割重複を解消する

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/refactor-validate-extension-entry-dedup

## 目的

`websocket_handshake.rs` の `validate_extension_entry` を廃止し、`Extension::parse_strict` に検証を一本化する。

## 優先度根拠

Low。同一 ABNF 検証が `Error` と `String`（→ `ExtensionParseError`）で二重管理されている。

## 現状

- `src/websocket_handshake.rs:691-793`: `validate_extension_entry` → `Error`
- `src/websocket_extension.rs:150-224`: `parse_strict`

## 設計方針

1. **前提**: `0034` で `parse_strict` が `ExtensionParseError` を返すこと
2. `validate_extension_entry` の呼び出しを `Extension::parse_strict` + `map_err`（`Error::handshake_rejected` 等）に置換
3. `validate_extension_entry` 関数を削除
4. `0044` でハンドシェイク分割する場合、置換後に削除すること（順序は `0034` → 本 issue → `0044` 推奨）

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] validate_extension_entry を Extension::parse_strict に統一する
  - @<GitHub ハンドル>
```

## 完了条件

- `validate_extension_entry` が無い
- ハンドシェイク経路の拡張検証が `parse_strict` のみ
- `cargo test --workspace` が全件パスする

## 解決方法

`websocket_handshake.rs` の `validate_extension_entry` (約 100 行、Sec-WebSocket-Extensions の ABNF + quoted-string + token 検証) を削除し、呼び出し側 (`websocket_handshake_request.rs:402`、`websocket_handshake_response.rs:225`) を `Extension::parse_strict(ext).map_err(|e| Error::handshake_rejected(format!("invalid Sec-WebSocket-Extensions value: {e}")))?` に置換した。

`Extension::parse_strict` は issue 0049 で内部関数 `parse_one(ext_str, strict=true)` に統一済みで、`ExtensionParseError::InvalidValue` を返す。caller 側で `Error::handshake_rejected` に変換する経路は `websocket_client_connection.rs` の既存実装と同じパターンで、エラーメッセージのフォーマットも統一される。

`websocket_handshake.rs` のモジュール doc コメントから「Sec-WebSocket-Extensions の ABNF 検証 (`validate_extension_entry`)」を削除し、検証経路が `Extension::parse_strict` に集約されたことを明記。`cargo test --workspace` 全件パス。
