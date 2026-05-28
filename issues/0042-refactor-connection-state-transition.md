# ConnectionState の状態遷移を明示的に定義する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/refactor-connection-state-transition

## 目的

`ConnectionState` の許可遷移を表として定義し、`set_state` で不正遷移を `Error::invalid_state` 等で拒否する。

## 優先度根拠

Medium。現状は任意の `ConnectionState` へ代入可能（`0041` 完了後も遷移ルールは別途必要）。

## 現状

- `src/websocket_connection_types.rs`: `ConnectionState` 定義
- `src/websocket_connection_shared.rs`: `set_state` が妥当性を検証していない

## 許可遷移表

実装から導出した許可遷移（実装前に `rg 'set_state' src/` で再確認すること）:

| 現在の状態 | 遷移先 | トリガー |
|---|---|---|
| `Disconnected` | `Connecting` | `connect()` / `accept_handshake()` |
| `Connecting` | `Connected` | ハンドシェイク完了 |
| `Connected` | `Closing` | `close_internal`（Close フレーム送信） |
| `Closing` | `Closed` | 相手 Close フレーム受信 / Close タイムアウト |
| `Disconnected` | `Closed` | サーバーがハンドシェイク拒否 |
| `Connected` | `Closed` | サーバー側の異常切断処理 |

上記以外の遷移は `Err(Error::invalid_state(...))` で拒否する。

## 設計方針

1. 上記遷移表を `websocket_connection_types.rs` のコメントに記載し、コードの真実の源とする
2. `set_state(&mut self, next: ConnectionState) -> Result<(), Error>` に変更し、遷移表外は `Err`
3. `set_state` 呼び出し元を `?` または `expect` で処理（パニックにしない）
4. PBT: 合法遷移列は成功、故意の非法遷移はエラー（`pbt/tests/prop_client_connection.rs` または新規 `prop_connection_state.rs`）

## 依存

- `0041` 推奨（フィールド private 化後に `set_state` を唯一の書き込み経路にする）

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] ConnectionState の許可遷移を set_state で検証する
  - @<GitHub ハンドル>
```

## 完了条件

- 遷移表がコードまたは issue コメントで明示されている
- 不正遷移がエラーになる
- PBT で少なくとも 1 件非法遷移が拒否されることを検証
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある
