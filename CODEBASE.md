# CODEBASE

本リポジトリ固有の指示。`AGENTS.md` と併せて参照すること。

- 良い設計のためには破壊的変更を積極的に行うこと
- 間違いを認め、よりよい修正をすること
- 性能より堅牢性を優先すること
- 依存は最小限にすること

## WebSocket について

- Sans I/O を徹底すること
  - ソケットや時計などの I/O は利用側が担い、ライブラリはバイト列とイベントだけを扱う
  - `RandomSource` のような乱数供給も利用側から注入する
- RFC 準拠を最優先とすること
  - RFC 6455 (The WebSocket Protocol) と RFC 7692 (Compression Extensions for WebSocket) に準拠する
  - RFC を確認する際は `refs/` 以下を利用すること
  - 仕様に由来する実装には、根拠となる RFC のセクション番号をコメントで残すこと
- HTTP/1.1 上の WebSocket のみをサポートすること
- 内部向け API は `#[doc(hidden)]` 付きの再エクスポートに留め、通常の公開 API には含めないこと
  - PBT や fuzz から参照するために公開する場合も同様とする

## サンプルについて

- `examples/` 以下は利用者へのお手本として位置付けること
  - 実在のサーバー・クライアントと通信できる状態を保つこと
  - ログとパニックメッセージは英語で書くこと
- `examples/websocket_client` の nginx e2e テストは Docker を必要とする
  - `make test` や prek の `cargo test` では除外し、GitHub Actions 上でのみ実行する
  - 手元で実行する場合は `make test-e2e` を使う

## 開発について

- ツールチェーンは `rust-toolchain.toml` で MSRV に固定している
  - MSRV を上げるときは `Cargo.toml` の `rust-version` と `rust-toolchain.toml` の `channel` を同時に更新すること
- Git フックは prek で管理している
  - 初回は `prek install --prepare-hooks` を実行すること
- fuzzing は nightly ツールチェーンを利用する
  - `cargo +nightly fuzz run <target>` で実行する
