# 0022: サーバーレスポンスで複数行の Sec-WebSocket-Extensions を誤って拒否する

- Priority: High
- Created: 2026-05-14
- Completed: 2026-05-26
- Polished: 2026-05-26
- Model: DeepSeek V4 Flash
- Branch: feature/fix-server-extensions-multiple-header

## 目的

`HandshakeValidator::validate_response()` はサーバーからの HTTP レスポンスに
`Sec-WebSocket-Extensions` ヘッダーが 2 行以上含まれている場合にエラーとして拒否する。
リクエスト側 (`HandshakeRequestValidator`) は複数行を正しくマージして処理しており、
レスポンス側のみ拒否するのは一貫性がない。

nginx 等のリバースプロキシが同名ヘッダーを複数行に分割して送出するケースがあり、
この実装だと相互運用性が損なわれる。

## 優先度根拠

High。相互運用性に直接影響するバグであり、実サーバーとの接続で発現する可能性がある。

## 現状

### RFC 6455 内部の矛盾

RFC 6455 には以下の矛盾する記述が存在する:

- Section 4.2.2 項目 6 (refs/rfc6455.txt line 1351-1356): サーバーレスポンスで
  複数の拡張を複数行に分割してよいと明記している

  > If multiple extensions are to be used, they can all be listed in a
  > single |Sec-WebSocket-Extensions| header field or split between
  > multiple instances of the |Sec-WebSocket-Extensions| header field.

- Section 11.3.2 (refs/rfc6455.txt line 3228-3232): HTTP レスポンスでは
  `Sec-WebSocket-Extensions` は 1 回のみ出現してよいと規定している

  > However, the |Sec-WebSocket-Extensions| header field MUST NOT appear
  > more than once in an HTTP response.

本 issue では以下の判断により Section 4.2.2 を優先する:

1. Section 4.2.2 はプロトコル動作の規範的記述であり、サーバーがハンドシェイク
   レスポンスをどう構成するかの直接的な仕様である
2. Section 11.3.2 は IANA ヘッダーフィールド登録のメタデータ記述であり、
   プロトコル動作の規範としては Section 4.2.2 が優先される
3. HTTP の一般原則として、同名ヘッダーの複数行はリスト値として統合される
   (RFC 9110 Section 5.3, refs/rfc9110.txt line 1517-1522)
4. nginx 等の実サーバーが複数行を送出する現実がある
5. リクエスト側では既に複数行マージが実装されており、レスポンス側も同様に
   扱うことで実装の一貫性が保たれる

### 該当コード

`src/websocket_handshake.rs` の `validate_response` 内:

```rust
// RFC 6455 Section 11.3.2: HTTP レスポンスでは Sec-WebSocket-Extensions は 1 回のみ許容
let extension_values = response.get_headers("Sec-WebSocket-Extensions");
if extension_values.len() > 1 {
    return Err(Error::handshake_rejected(
        "duplicate Sec-WebSocket-Extensions header in HTTP response",
    ));
}
```

この拒否チェックの直後に、マージロジック (`extension_values.iter().flat_map(...)`) が
既に実装されているが、拒否チェックにより到達不能になっている。

### リクエスト側の実装

`validate_request` では複数行を正しくマージして処理している:

```rust
let values = request.get_headers("Sec-WebSocket-Extensions");
let extensions: Vec<String> = values
    .iter()
    .flat_map(|v| v.split(','))
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .collect();
```

## 設計方針

1. `validate_response` 内の `if extension_values.len() > 1 { return Err(...) }` の
   拒否チェックを削除する。削除後のコメントは以下とする:
   `// RFC 6455 Section 4.2.2 項目 6: 複数行の Sec-WebSocket-Extensions を許容し統合する`

2. リクエスト側 `validate_request` の拡張ヘッダー処理コメントは変更しない。
   リクエスト側の複数行許容の根拠は RFC 6455 Section 11.3.2
   (「MAY appear multiple times in an HTTP request」) であり、
   Section 4.2.2 はサーバーレスポンスの仕様であるためリクエスト側には適用されない

### 安全性の評価

- マージ後も `validate_extension_entry` が各エントリの ABNF 適合性を検証する
- `!extension_values.is_empty() && extensions.is_empty()` チェックは
  マージ後の extensions が空の場合に正しく発火する
- マージ結果は `Vec<String>` であり、下流の各 Connection は個別に検証するため影響を受けない。
  複数行の各々に permessage-deflate が含まれていた場合、マージ後に複数の
  permessage-deflate エントリが生じるが、issue 0012（クライアント側、closed）および
  issue 0015（サーバー側、closed）の実装で重複検出され正しく拒否される

## 完了条件

1. `validate_response` 内の複数行拒否チェックが削除されている
2. コメントが設計方針の通りに更新されている
3. `pbt/tests/prop_handshake.rs` の既存テストが修正され新規テストが追加されている
4. `cargo check --workspace` がエラーなしで通る
5. `cargo clippy --workspace --all-targets -- -D warnings` が警告なしで通る
6. `cargo test --workspace` が全件パスする
7. `CHANGES.md` の `## develop` に変更履歴エントリが追加されている

## 解決方法

### `src/websocket_handshake.rs` — `validate_response` を修正

`if extension_values.len() > 1 { return Err(...) }` の拒否チェックを削除し、
コメントを更新する。マージロジックは既に実装済みのため追加不要。

### `src/websocket_handshake.rs` — `validate_request` のコメント

リクエスト側のコメントは現状で正しいため変更不要。

### `pbt/tests/prop_handshake.rs` — テストを修正・追加

1. 既存テスト `prop_response_duplicate_extension_headers_rejected` (line 1246-1268) を
   `prop_response_multiple_extension_headers_accepted` に改名し、
   `prop_assert!(result.is_ok())` に変更する。
   マージ後の `extensions` ベクタに両方の拡張が含まれることを確認するアサーションを追加する
2. 以下のエッジケースをカバーするテストを追加する:
   - 1 行目が空、2 行目が有効（マージ後は 1 要素）
   - 2 行とも空（`malformed` エラーが発火することの確認）
   - 2 行とも ABNF 違反（`validate_extension_entry` のエラーが発火することの確認）

## CHANGES.md 登録内容

- [FIX] サーバーレスポンスで複数行の Sec-WebSocket-Extensions を誤って拒否する問題を修正する
  - @実装者名
