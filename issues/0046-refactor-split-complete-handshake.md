# complete_handshake を分割する

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: mimo-v2.5-pro
- Branch: feature/refactor-split-complete-handshake

## 目的

`WebSocketClientConnection::complete_handshake`（約 155 行）から deflate ネゴシエーション検証を抽出する。

## 優先度根拠

Low。`0045`（サーバー側）と対称の整理。

## 現状

`src/websocket_client_connection.rs:375-530` に protocol 検証、拡張 ABNF、deflate 検証 + コーデック作成、状態遷移 + タイマーが混在。

## 設計方針

```rust
fn validate_deflate_negotiation(
    &mut self,
    ext: &Extension,
) -> Result<(), Error>
```

- PMCE パラメータ検証（既存インライン 416-508 行相当）
- 成功時 `self.shared.deflate`（または既存フィールド）へ `PerMessageDeflate` を設定
- `complete_handshake` は protocol / extension ループと状態遷移 + イベントキュー + Ping タイマーを残す

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] complete_handshake から deflate ネゴシエーション検証を抽出する
  - @<GitHub ハンドル>
```

## 完了条件

- `validate_deflate_negotiation` が追加され、インライン重複が無い
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある
