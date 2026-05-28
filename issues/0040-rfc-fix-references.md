# RFC 引用コメントの節番号を修正する

- Priority: Medium
- Created: 2026-05-27
- Polished: 2026-05-28
- Model: mimo-v2.5-pro
- Branch: feature/refactor-rfc-fix-references

## 目的

ソース内の RFC 引用コメント 5 箇所の節番号・文脈を、`refs/` の一次資料と一致させる。実装挙動は変えない。

## 優先度根拠

Medium。誤った節番号は将来の仕様解釈ミスにつながる。`c90410e` 以降も未修正の箇所が残っている。

## 現状（修正対象）

| # | ファイル:行 | 現コメントの問題 | 修正方針 |
|---|---|---|---|
| 1 | `websocket_handshake.rs:9` | GUID が Section 1.3 | RFC 6455 Section 4.2.2 step 4（`refs/rfc6455.txt` で確認） |
| 2 | `websocket_client_connection.rs:390` | 拡張検証が 4.2.2 | クライアントの拡張検証は Section 4.1 step 5 |
| 3 | `websocket_client_connection.rs:417` | 7692 Section 7.1.2 が主 | 一般 ABNF は RFC 6455 Section 9.1。PMCE 固有は 7692 を併記 |
| 4 | `websocket_client_connection.rs:481-482` | 7.2.1 | window bits 制約は RFC 7692 Section 7.1.2.1 |
| 5 | `websocket_server_connection.rs:259` | 9.1 が送信側検証のように読める | 受信側義務である旨をコメントで明示 |

`websocket_frame.rs:231-237` の最小表現チェックは既に `// RFC 6455 Section 5.2: 最小表現チェック` と正しい節番号で引用されているため、本 issue では対象外とする。

実装前に **必ず** `refs/rfc6455.txt` / `refs/rfc7692.txt`（存在する場合）で該当行を開き、節番号と MUST/SHOULD を照合すること。

## 設計方針

- コメントと doc コメントのみ変更（ロジック変更なし）
- 根拠資料名・節番号をコメントに明記（AGENTS.md: 資料由来機能のコメント規約）

## CHANGES.md 登録内容

`## develop` > `### misc`:

```markdown
- [UPDATE] RFC 引用コメントの節番号を一次資料に合わせて修正する
  - @<GitHub ハンドル>
```

## 完了条件

- 上記 5 箇所が `refs/` と整合する節番号・文脈になっている
- `cargo test --workspace` が全件パスする
- `CHANGES.md` に上記 `[UPDATE]` と担当者行がある
