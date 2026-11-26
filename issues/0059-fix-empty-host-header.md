# 空または空白だけの Host ヘッダーを拒否する

- Priority: Medium
- Created: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-empty-host-header

## 目的

WebSocket のオープニングハンドシェイクで、Host ヘッダーの値が空または空白だけでも受理される。RFC 6455 Section 4.2.1 item 4 は「サーバーの authority を含む Host ヘッダー」を要求しており、空の値は authority ではない。RFC 9112 Section 3.2 も、不正な Host を持つリクエストに 400 を返すことをサーバーに要求する。

## 優先度根拠

Medium。`ServerHandshakeRequest.host` が空文字列のままアプリケーションへ渡るため、Host によるルーティングや検証を行う利用側が、不正なリクエストを正規のものとして扱う可能性がある。

## 現状

- `src/websocket_handshake_request.rs` の `HandshakeRequestValidator::validate_request` は `request.get_header("Host")` の存在だけを確認し、値の妥当性を検査していない。
- HTTP デコードを担う `shiguredo_http11` の `RequestDecoder` は `Host: ` を値 `""` として通す。空白だけの値も同様に通る。
- 正常な Host と、ポート付き・不正文字を含む Host の検証は `shiguredo_http11` 側で行われている。

## 設計方針

`validate_request` で Host の値を取得した直後に `trim().is_empty()` を検査し、空なら `Error::handshake_rejected` を返す。`shiguredo_http11` 側の変更は行わない。

## CHANGES.md 登録内容

`## develop` の FIX セクションに次を追加する。

```markdown
- [FIX] 空または空白だけの Host ヘッダーを拒否する
  - @voluntas
```

## 完了条件

- `Host:` と `Host:  ` のリクエストが `ErrorKind::HandshakeRejected` になるテストがある。
- 正常な Host のリクエストは従来どおり受理される。
- `cargo test --workspace` が通る。
