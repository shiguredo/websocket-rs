# accept_handshake を分割する

- Priority: Low
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: opencode mimo-v2.5-pro
- Branch: feature/refactor-split-accept-handshake

## 目的

`WebSocketServerConnection::accept_handshake`（約 237 行）を責務ごとの private メソッドに分割する。

## 優先度根拠

Low。挙動変更なし。拡張検証・deflate ネゴシエーション・レスポンス構築の可読性向上。

## 現状

`src/websocket_server_connection.rs:239-476` に上記 3 責務がインライン。

## 設計方針

次の private メソッドへ抽出（名称は実装時に調整可）:

- `validate_extensions(...)` — protocol / extensions ABNF / 未サポート拒否
- `negotiate_deflate(...)` — permessage-deflate ネゴシエーションと `PerMessageDeflate` 構築
- `build_handshake_response(...)` — 101 レスポンス組み立て

`accept_handshake` はオーケストレーションのみ。公開 API シグネチャは不変。

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] accept_handshake を責務ごとの private メソッドに分割する
  - @<GitHub ハンドル>
```

## 完了条件

- `accept_handshake` が薄いオーケストレーションになっている
- 既存サーバー接続 PBT / テストが全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある
