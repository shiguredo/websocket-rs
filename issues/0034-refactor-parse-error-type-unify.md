# パース関数のエラー型を統一する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/change-parse-error-type-unify

## 目的

`Extension::parse_strict` の `Result<_, String>` を `Result<_, ExtensionParseError>` に統一し、拡張パースまわりのエラー処理を一貫させる。

**前提**: `0029`（`ExtensionParseError` の Display / Error / 公開）を完了してから着手する。

## 優先度根拠

Medium。同一モジュールで `String` と `ExtensionParseError` が混在し、呼び出し側が `map_err(|e| Error::handshake_rejected(e))` のように文字列経由で変換している。

## 現状

- `src/websocket_extension.rs:150-224`: `parse_strict` → `Result<Vec<Extension>, String>`
- `src/websocket_extension.rs:477-601`: `from_extension_validated` → `ExtensionParseError`
- 呼び出し: `websocket_client_connection.rs`, `websocket_server_connection.rs`（`map_err` で `String` を handshake エラーへ変換）
- PBT: `pbt/tests/prop_handshake.rs` の `parse_strict` 関連（`Result` のエラー型変更でコンパイル修正が必要）

## 設計方針

1. `parse_strict` の戻り値を `Result<Vec<Extension>, ExtensionParseError>` に変更
2. 既存 `format!(...)` エラーは次の方針でマッピングする:
   - 拡張名が permessage-deflate 以外の文脈に相当 → 該当なし（`parse_strict` は名前検証のみ）
   - token / パラメータ名不正 → `UnknownParameter` または `InvalidValue`（メッセージは Display 用 `detail` に既存英文を流用）
   - 空名前・trailing `;`・quoted-string 構文エラー → **`InvalidValue(String)`** に統一（`detail` に既存 `format!` 文言をそのまま入れる）
3. 呼び出し側の `map_err(|e| ... e.to_string())` を `map_err(Error::handshake_rejected)` 等、`Display` 経由または `?` + `From` 化に整理
4. `prop_handshake.rs` の `parse_strict` テストを `ExtensionParseError` 前提に更新

新規バリアントの追加は **行わない**（`0029` で定義済みの列挙子のみ使用）。

## 実装手順

1. `parse_strict` 本体の `Err(format!(...))` を `Err(ExtensionParseError::InvalidValue(...))` 等へ置換
2. クライアント / サーバー接続の `map_err` を更新（`Display` 経由で `Error::handshake_rejected(e.to_string())` に変換）
3. `pbt/tests/prop_handshake.rs` の `parse_strict` テストをエラー型変更に合わせ修正
4. `skills/shiguredo-websocket/SKILL.md` の `parse_strict` 戻り値型記述を `Result<Vec<Extension>, ExtensionParseError>` に更新

## テスト戦略

- PBT: 既存 `prop_handshake` の `parse_strict` 群を型変更後も維持
- 新規ファイル不要

## CHANGES.md 登録内容

`## develop` 直下に次を追加する（公開 API の戻り値型変更であり後方互換のない変更のため `[CHANGE]`。AGENTS.md の種別順に従い `[ADD]` より前に配置する）:

```markdown
- [CHANGE] Extension::parse_strict のエラー型を String から ExtensionParseError に変更する
  - @<GitHub ハンドル>
```

## 完了条件

- `Extension::parse_strict` が `Result<Vec<Extension>, ExtensionParseError>` を返す
- 呼び出し側・PBT・SKILL の型記述が更新されている
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[CHANGE]` と担当者行がある
