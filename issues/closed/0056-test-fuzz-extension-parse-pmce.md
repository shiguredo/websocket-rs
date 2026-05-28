# fuzz_extension_parse.rs に PerMessageDeflateConfig のパースを追加する

- Priority: High
- Created: 2026-05-27
- Completed: 2026-05-28
- Polished: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/test-fuzz-extension-parse-pmce

## 目的

`fuzz_extension_parse.rs` で `PerMessageDeflateConfig` の server/client パース経路を任意入力に曝露し、パニック安全性を検証する。

## 優先度根拠

High。先頭ゼロ・範囲外・重複パラメータ等の分岐が多く、現状 fuzz 未カバー。`0039` の PMCE 項目と統合実装してよい。

## 現状

- `fuzz/fuzz_targets/fuzz_extension_parse.rs`: `Extension::parse` / `parse_strict` のみ
- `from_extension_for_server_request` / `from_extension_for_client_response` は未呼び出し

## 設計方針

1. 入力 `s` から `parse_strict(s)` を試行
2. 得られた `Extension` のうち `name == "permessage-deflate"` に対し両 `from_extension_*` を呼ぶ（`Err` は無視、パニックのみ禁止）
3. `parse` 失敗時も生文字列から `Extension::new("permessage-deflate").param(...)` 等の経路は **追加しない**（現実的な経路のみ）

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] fuzz_extension_parse で PerMessageDeflateConfig パースをカバーする
  - @<GitHub ハンドル>
```

## 完了条件

- `cargo fuzz build fuzz_extension_parse`（または該当ターゲット名）が成功
- `cargo test --workspace` が全件パスする

## 解決方法

0039（fuzzing ターゲット拡充）と完全に重複するため close し、0039 に統合する。両 issue の本文で相互統合を許容済み（0039 L39, 0056 L15）。実装は 0039 の 1 タスクとして行う。
