# 空または空白だけの Host ヘッダーを拒否する

- Priority: Medium
- Created: 2026-11-27
- Polished: 2026-11-27
- Completed: 2026-11-27
- Model: deepseek-v4.1-flash:1017
- Branch: feature/fix-empty-host-header

## 目的

WebSocket のオープニングハンドシェイクで、Host ヘッダーの値が空または空白だけでも受理される。RFC 6455 Section 4.2.1 item 2 は「サーバーの authority を含む Host ヘッダー」を要求しており、空の値は authority ではない。同節の冒頭も、この要件に反するハンドシェイクに対して 400 Bad Request などの HTTP エラー応答を返すことをサーバーに要求している。

## 優先度根拠

Medium。`ServerHandshakeRequest.host` が空文字列のままアプリケーションへ渡るため、Host によるルーティングや検証を行う利用側が、不正なリクエストを正規のものとして扱う可能性がある。

## 現状

- `src/websocket_handshake_request.rs` の `HandshakeRequestValidator::validate_request` は `request.get_header("Host")` の存在だけを確認し、値の妥当性を検査していない。
- HTTP デコードを担う `shiguredo_http11` の `RequestDecoder` は `Host: ` を値 `""` として通す。空白だけの値も同様に通る。
- 正常な Host と、ポート付き・不正文字を含む Host の検証は `shiguredo_http11` 側で行われている。

## 設計方針

`validate_request` で Host の値を取得した直後に `trim().is_empty()` を検査し、空なら `Error::handshake_rejected` を返す。コードコメントには RFC 6455 Section 4.2.1 item 2 を記載する。`shiguredo_http11` 側の変更は行わない。

## CHANGES.md 登録内容

`## develop` に次のエントリを追加する。

```markdown
- [FIX] 空または空白だけの Host ヘッダーを拒否する
  - @voluntas
```

## 完了条件

- `Host:` と `Host:  ` のリクエストが `ErrorKind::HandshakeRejected` になるテストがある。
- 正常な Host のリクエストは従来どおり受理される。
- `make test`（実体は `cargo test --workspace --exclude websocket_client`）が通る。nginx e2e テストは Docker が必要なため GitHub Actions で実行する。

## 解決方法

`src/websocket_handshake_request.rs` の `HandshakeRequestValidator::validate_request` で Host の値を取得した直後に `trim().is_empty()` を検査し、空なら `Error::handshake_rejected("invalid Host header: empty authority")` を返すようにした。

`pbt/tests/rfc6455.rs` に `prop_section_4_2_1_host_authority_required` と `prop_section_4_2_1_host_authority_accepts_authority` を追加した。空・空白のみの Host が `ErrorKind::HandshakeRejected` になり、`example.com`、`example.com:8080`、`127.0.0.1`、`127.0.0.1:80` は受理されることを確認する。

`make test` が通る (24 個のテストバイナリすべて ok)。
