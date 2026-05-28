# shiguredo_http11 のバージョン指定をマイナーまでに修正する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/refactor-http11-version-patch-level

## 目的

AGENTS.md の規約に違反している `shiguredo_http11` のバージョン指定をマイナーまでに修正する。

AGENTS.md で「バージョン番号はマイナーバージョンまで指定すること」と明記されている。パッチレベルまで固定するとセキュリティ修正やバグ修正を含むパッチリリースの自動取り込みが阻害されるため、マイナーまでの指定が規約とされている。`shiguredo_http11` は CalVer（`YYYY.MINOR.PATCH`）形式だが、Cargo の semver 互換解釈に従い同規約を適用する。

## 優先度根拠

Medium。依存の解決範囲の表記のみでありランタイム挙動は変わらない。規約違反の解消のため High ではなく Medium とする。

## 現状

`Cargo.toml` の `[workspace.dependencies]` で `shiguredo_http11 = "2026.6.1"` とパッチレベルまで指定されている。他の workspace 依存（`aws-lc-rs`, `base64ct` 等）はマイナー指定に準拠済み。

Cargo の semver では `"2026.6.1"` は `>=2026.6.1, <2026.7.0`、`"2026.6"` は `>=2026.6.0, <2026.7.0` を意味する。差異は最小バージョンの起点が `2026.6.0` か `2026.6.1` かのみ。`Cargo.lock` には既に `version = "2026.6.1"` が記載されているため、**変更後も lock の解決結果は変わらない想定**（差分が出た場合は lock をコミットに含める）。

## 設計方針

`Cargo.toml` の `[workspace.dependencies]` にある `shiguredo_http11 = "2026.6.1"` を `shiguredo_http11 = "2026.6"` に変更する。メンバークレートは `shiguredo_http11.workspace = true` のため、個別 `Cargo.toml` の編集は不要。

## 実装手順

1. `Cargo.toml` の `[workspace.dependencies]` にある `shiguredo_http11 = "2026.6.1"` を `shiguredo_http11 = "2026.6"` に変更
2. `cargo build` および `cargo test --workspace` で確認
3. `Cargo.lock` に意図しない差分が無いことを確認（あればコミットに含める）
4. `CHANGES.md` を更新

## テスト戦略

- 新規テスト・PBT・fuzz の追加・変更は不要（依存指定の表記のみ）

## CHANGES.md 登録内容

`## develop` > `### misc` に次を追加する:

```markdown
- [UPDATE] shiguredo_http11 のバージョン指定をマイナーまでに修正する
  - @<GitHub ハンドル>
```

## 完了条件

- `Cargo.toml` の `shiguredo_http11` が `"2026.6"` になっている
- `[workspace.dependencies]` にパッチレベル指定の依存が残っていない（`rg '= "[0-9]+\.[0-9]+\.[0-9]+"' Cargo.toml` の結果がパッケージ自身の `version = "2026.2.0"` のみであること）
- `cargo build` が通過する
- `cargo test --workspace` が全件パスする
- `CHANGES.md` の `## develop` > `### misc` に上記 `[UPDATE]` エントリと担当者行が追加されている

## 解決方法

`Cargo.toml` の `[workspace.dependencies]` にある `shiguredo_http11 = "2026.6.1"` を `shiguredo_http11 = "2026.6"` に変更した。メンバークレートは `shiguredo_http11.workspace = true` を参照しているため個別 `Cargo.toml` の編集は不要だった。

検証:

- `rg '= "[0-9]+\.[0-9]+\.[0-9]+"' Cargo.toml` がパッケージ自身の `version = "2026.2.0"` のみを返す
- `cargo build` 通過
- `cargo test --workspace` 全件パス
- `Cargo.lock` に差分は出ない（既存解決結果と同一）
