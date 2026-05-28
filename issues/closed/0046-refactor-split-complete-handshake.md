# complete_handshake を分割する

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Completed: 2026-05-28
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

## 解決方法

`WebSocketClientConnection::complete_handshake` (約 155 行) から permessage-deflate (RFC 7692) のネゴシエーション検証を `validate_deflate_negotiation(&mut self, ext: &Extension) -> Result<(), Error>` に抽出した。

`complete_handshake` 本体は次のオーケストレーションに縮小:

- RFC 6455 Section 4.2.2: protocol が要求に含まれているかの検証
- RFC 6455 Section 9.1: 拡張の ABNF / unsolicited 拡張の拒否
- RFC 7692 Section 7.1.3: 複数の permessage-deflate 要素を含むレスポンスの拒否
- 拡張ループから `validate_deflate_negotiation` を呼び出す
- 状態遷移 (Connecting → Connected) と Connected イベント / Ping タイマー登録

`validate_deflate_negotiation` 内では RFC 7692 の各パラメータ検証 (`client_max_window_bits` / `server_max_window_bits` の上限・unsolicited 検出) を行い、合意成立時に `PerMessageDeflate::new_client(config)` を `SharedConnectionState::enable_deflate` 経由で登録する。挙動変化なし、`cargo test --workspace` 全件パス。
