# SharedConnectionState のフィールドをカプセル化する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/refactor-shared-state-encapsulation

## 目的

`SharedConnectionState` の `pub(crate)` フィールド直アクセスをやめ、`state` 変更は `set_state` 経由に限定する。

## 優先度根拠

Medium。`state` の直接代入は不正遷移の温床。`0042`（遷移表）の前提となる。

## 現状

`src/websocket_connection_shared.rs:81-98`: 全フィールドが `pub(crate)`。`websocket_client_connection.rs` / `websocket_server_connection.rs` / 同一ファイル内から直接読み書き。

## 設計方針

1. フィールドの `pub(crate)` を削除
2. 読み取り用 `fn state(&self) -> ConnectionState` 等、既存呼び出しに必要な最小限のアクセサを追加
3. `state` の書き込みは既存 `set_state`（または `0042` で強化後の API）のみ
4. 同一モジュール外からの `shared.state = ...` をコンパイルエラーにし、呼び出し側を置換

## 依存

- `0042` と並行可能だが、先に本 issue でフィールドを private 化してから遷移検証を入れると差分が追いやすい

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] SharedConnectionState のフィールドをカプセル化する
  - @<GitHub ハンドル>
```

## 完了条件

- `SharedConnectionState` のフィールドが外部から直接アクセス不可
- 必要なアクセサが追加され、クライアント / サーバー / shared 内の参照がコンパイルできる
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある

## 解決方法

`src/websocket_connection_shared.rs` の `SharedConnectionState` 17 フィールドから `pub(crate)` 修飾を全て除去し、モジュール内 private にした。代わりに必要最小限のアクセサ `state` / `is_failed` / `mark_failed` / `enable_deflate` / `emit_event` / `enqueue_output` を追加し、書き込みは既存の `set_state` 経由に統一して不正な状態遷移を防ぐようにした。`enable_deflate` は当初 `set_deflate(Option<PerMessageDeflate>)` だったが、レビューを受けて `Option` の包みを取り除き「ハンドシェイク合意成立時に 1 回だけ呼ぶ」用途に絞った。

カプセル化を機に以下の付帯整理も実施した:

- 同モジュール内の `ClientFramePolicy::encode_and_send` / `ServerFramePolicy::encode_and_send` が `output_queue.push_back(...)` を直接触っていたのを `shared.enqueue_output(...)` 経由に置換し、アクセサ追加の意義を一貫させた
- `ClientFramePolicy::random` フィールドの `pub(crate)` を外し、構造体内 private とした
- `set_state` に「`state` への書き込みは本メソッドに集約する」旨の doc コメントを追加した

呼び出し側 `src/websocket_client_connection.rs` / `src/websocket_server_connection.rs` の `self.shared.state` / `self.shared.failed` / `self.shared.deflate` / `self.shared.event_queue.push_back` / `self.shared.output_queue.push_back` を全て新アクセサに置換した。公開 API・テストの修正は不要で、`cargo test --workspace` が全件パス。
