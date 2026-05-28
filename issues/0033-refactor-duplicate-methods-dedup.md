# send_text 等の重複メソッドを共通化する

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/refactor-duplicate-methods-dedup

## 目的

`send_text` / `send_binary` / `send_ping` / `handle_timer` / `poll_event` / `poll_output` がクライアントとサーバーで `SharedConnectionState` への同一委譲を繰り返している。`FramePolicy` のデフォルト実装で共通化する。

## 優先度根拠

Low。挙動変更ではなく重複削減。`ClientFramePolicy<R>` と `ServerFramePolicy` の型差は trait で吸収する。

## 現状

- `src/websocket_client_connection.rs`: 各メソッドが `self.shared.*` へ委譲
- `src/websocket_server_connection.rs`: 同上

`policy` の型が異なるため、接続型ごとに同一メソッド定義が重複している。

## 設計方針

`FramePolicy` trait への追加は**採用しない**。`FramePolicy` の責務はフレームマスキング方向の検証に限定されており（`verify_frame_masking` / `encode_and_send`）、接続操作メソッドを追加すると Single Responsibility Principle に反する。

macro も**採用しない**（プロジェクト内で `macro_rules!` は未使用であり、新規導入するほどの重複ではない）。

代わりに `SharedConnectionState` に `check_connected()` を含む利便メソッドを追加し、クライアント / サーバー接続型の公開メソッドを単一行の委譲にする:

```rust
// src/websocket_connection_shared.rs に追加
impl SharedConnectionState {
    pub(crate) fn send_text(
        &mut self, text: &str, policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        self.check_connected()?;
        self.send_data_frame(Opcode::Text, text.as_bytes().to_vec(), policy)
    }
    // send_binary, send_ping も同様
}
// poll_event, poll_output, handle_timer は既に単一行委譲のため変更不要
```

- クライアント / サーバーの公開 API シグネチャ・戻り値は変更しない
- `close` は本 issue のスコープ外（`0032` で個別対応。検証ロジックが異なるため）
- poll_event / poll_output / handle_timer の委譲は既に単一行であり、重複解消の対象外とする

## テスト戦略

- 新規テスト不要
- `cargo test --workspace` で回帰確認
- 公開 API シグネチャの不変はコンパイル時に保証される（既存テスト・PBT が同じ呼び出しを行うため）

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] send_text 等の重複ロジックを SharedConnectionState に集約する
  - @<GitHub ハンドル>
```

## 完了条件

- send_text / send_binary / send_ping の `check_connected()` + 委譲ロジックが `SharedConnectionState` に集約されている
- クライアント / サーバーの公開 API に破壊的変更がない
- `cargo clippy --workspace --all-targets -- -D warnings` が通過する
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある
