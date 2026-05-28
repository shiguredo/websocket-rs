# deprecated な from_extension を削除する

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: mimo-v2.5-pro
- Branch: feature/change-remove-deprecated-from-extension

## 目的

`#[deprecated]` の `PerMessageDeflateConfig::from_extension` を削除する。代替は `from_extension_for_client_response` / `from_extension_for_server_request`。

## 優先度根拠

Low。呼び出し 0。検証が不十分な旧 API の温存をやめる。

## 現状

- `src/websocket_extension.rs:418-471`: `from_extension`（`since = "0.3.0"`）
- `rg 'from_extension\\('` で定義以外 0 件

## 設計方針

関数定義ごと削除。`skills/shiguredo-websocket/SKILL.md` の `from_extension` 言及（制限事項付近）を削除または代替 API へ差し替え。

## テスト戦略

- 新規テスト不要（呼び出し 0 件の deprecated 関数削除のため）
- `cargo test --workspace` で回帰確認

## CHANGES.md 登録内容

`## develop` 直下に追加する（公開 API 削除のため `[CHANGE]`。AGENTS.md の種別順に従い先頭に配置）:

```markdown
- [CHANGE] deprecated な PerMessageDeflateConfig::from_extension を削除する
  - @<GitHub ハンドル>
```

## 完了条件

- `from_extension` がソースから削除されている
- `skills/shiguredo-websocket/SKILL.md` が更新されている
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[CHANGE]` と担当者行がある
