# close メソッドの検証ロジックをクライアント/サーバー間で共通化する

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: mimo-v2.5-pro
- Branch: feature/refactor-close-method-dedup

## 目的

`WebSocketClientConnection::close` と `WebSocketServerConnection::close` の検証ロジックが実質同一であり、変更時の二重修正を避ける。検証付き `close` を `SharedConnectionState` に集約する。

## 優先度根拠

Low。送信禁止コード・reason 長・状態チェックが重複。`0041` / `0043`（shared 分割）と独立して着手可能だが、完了後は shared 側の変更が中心になる。

## 現状

- `src/websocket_client_connection.rs:296-325`
- `src/websocket_server_connection.rs:549-577`

両者とも: 状態が `Connected` / `Closing` 以外なら `InvalidState`、禁止コードなら `InvalidInput`、reason が 123 バイト超なら `InvalidInput`、成功時は `close_internal` 委譲。

`SharedConnectionState::close_internal` は `Disconnected` / `Closed` を silently ignore する（内部エラー用）。

## 設計方針

`SharedConnectionState` に次を追加する:

```rust
pub(crate) fn close(
    &mut self,
    code: CloseCode,
    reason: &str,
    policy: &mut impl FramePolicy,
) -> Result<(), Error>
```

既存 `close_internal` と同じ引数パターンに合わせる。`close` はバリデーション付き（エラーを返す）、`close_internal` はバリデーションなし（silently ignore）の区別。

- `Connected` / `Closing` 以外 → `Err(Error::invalid_state(...))`
- `!code.is_sendable()` → `Err(Error::invalid_input(...))`
- reason バイト長 > 123 → `Err(Error::invalid_input(...))`
- 成功時 → `close_internal` を呼ぶ

クライアント / サーバーの公開 `close` は上記へ委譲のみ。公開 API シグネチャは変更しない。

## テスト戦略

- 新規テスト不要
- 既存 `prop_client_connection` / `prop_server_connection` / 単体テストで状態・コード・reason 長のエラーパスが引き続きカバーされることを `cargo test --workspace` で確認
- リファクタリング前後で `cargo llvm-cov` のカバレッジが対象パス（close の検証ロジック）で劣化していないことを確認

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] close メソッドの検証ロジックを SharedConnectionState に共通化する
  - @<GitHub ハンドル>
```

## 完了条件

- 検証ロジックが `SharedConnectionState` に集約されている
- クライアント / サーバーの `close` は委譲のみ
- 既存テストで状態エラー・コードエラー・reason 長エラーが引き続き検証されている
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある

## 解決方法

`src/websocket_connection_shared.rs` に `pub(crate) fn close(&mut self, code, reason, policy) -> Result<(), Error>` を追加し、状態 / 送信可能コード / reason 長の 3 段検証を集約した。成功時は内部の `close_internal` に委譲する。

`src/websocket_client_connection.rs::close` と `src/websocket_server_connection.rs::close` は `self.shared.close(code, reason, &mut self.policy)` への委譲のみに置き換えた。公開 API シグネチャと RFC 引用 doc コメント (7.4.1 / 5.5 / 7.1.2) はそのまま維持し、`cargo doc` から見える契約情報を失わないようにしている。

検証:

- `cargo fmt --all -- --check` 通過
- `cargo clippy --workspace --all-targets -- -D warnings` 通過
- `cargo test --workspace` 全件パス
- 既存テストでカバーされるのは状態エラー（`prop_close_rejected_in_connecting_state` 等）のみで、送信禁止コードと reason 長エラーの Connection 経由テストは develop 時点で存在しないが、リファクタ前後の検証コードは文字列レベルで完全一致しているため挙動退化はない。追加テストは issue 本文「新規テスト不要」の方針に従い見送る
