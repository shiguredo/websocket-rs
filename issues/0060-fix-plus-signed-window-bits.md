# window bits パラメータの符号付き表記を拒否する

- Priority: Medium
- Created: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-window-bits-sign

## 目的

permessage-deflate の `server_max_window_bits` / `client_max_window_bits` に `+8` や `+08` を指定しても受理される。RFC 7692 Section 7.1.2.1 / 7.1.2.2 の ABNF は `1*DIGIT` であり、符号や先頭ゼロを含む値は不正である。不正値を含む offer はサーバーが拒否し、不正値を含む応答はクライアントが接続を失敗させなければならない (RFC 7692 Section 7)。

## 優先度根拠

Medium。実装間で受理集合が食い違い、ABNF に厳密な相手との相互接続に失敗する。

## 現状

- `src/websocket_extension.rs` の `PerMessageDeflateConfig::from_extension_validated` は、先頭ゼロ検査の後に `value.parse::<u8>()` を使う。
- Rust の `str::parse::<u8>()` は先頭の `+` を受け付けるため `+8` は 8 になる。
- 先頭ゼロ検査は `starts_with('0')` のみのため `+08` を検出できない。

## 設計方針

パース前に、値が空でなく ASCII 数字だけで構成され、先頭が `0` で 2 文字以上でないことを検査する。検査は `server_max_window_bits` と `client_max_window_bits` の両方に適用する。

## CHANGES.md 登録内容

```markdown
- [FIX] window bits パラメータの符号付き表記を拒否する
  - @voluntas
```

## 完了条件

- `+8` と `+08` を含む offer / response が `ExtensionParseError::InvalidValue` になる。
- `8` と `15` は従来どおり受理される。
- 先頭ゼロの拒否と値域の検査が PBT で維持されている。
- `cargo test --workspace` が通る。
