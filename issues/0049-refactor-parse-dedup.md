# Extension::parse と parse_strict の役割重複を解消する

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/refactor-parse-dedup

## 目的

`Extension::parse` と `parse_strict` の重複ロジックを内部関数に集約する。

## 優先度根拠

Low。`parse` は不正要素をスキップ、`parse_strict` は Err — エラー方針のみ異なる。

## 現状

- `src/websocket_extension.rs:88-143`: `parse`
- `src/websocket_extension.rs:150-224`: `parse_strict`（ほぼ同一ループ）

## 設計方針

`fn parse_extensions(s: &str, strict: bool) -> Result<Vec<Extension>, Option<ExtensionParseError>>` のような内部 API を導入:

- `strict == false`: 不正 extension はスキップして `Ok(vec)`
- `strict == true`: 最初のエラーで `Err`

公開 `parse` / `parse_strict` のシグネチャは維持（`0034` 完了後は `parse_strict` は `ExtensionParseError`）。

## 依存

- `0034` 完了後に着手するとエラー型の二重変更を避けられる

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] Extension::parse と parse_strict の共通パースロジックを抽出する
  - @<GitHub ハンドル>
```

## 完了条件

- パース本体が 1 箇所
- 既存 `prop_handshake` の parse / parse_strict テストがパス
- `cargo test --workspace` が全件パスする
