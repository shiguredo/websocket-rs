# 0022: サーバーレスポンスで複数行の Sec-WebSocket-Extensions を誤って拒否する

- Created: 2026-05-14
- Model: deepseek-v4-flash

## 優先度

P1

## 概要

`HandshakeValidator::validate_response()` はサーバーからの HTTP レスポンスに
`Sec-WebSocket-Extensions` ヘッダーが 2 行以上含まれている場合に
エラーとして拒否する。RFC 6455 Section 4.2.2 Step 6 は拡張の複数行分割を
明示的に許容しており、これは RFC 違反である。

リクエスト側 (`HandshakeRequestValidator`) は複数行を正しくマージして処理している。

## 該当コード

`src/websocket_handshake.rs:609-615`:

```rust
// RFC 6455 Section 11.3.2: HTTP レスポンスでは Sec-WebSocket-Extensions は 1 回のみ許容
let extension_values = response.get_headers("Sec-WebSocket-Extensions");
if extension_values.len() > 1 {
    return Err(Error::handshake_rejected(
        "duplicate Sec-WebSocket-Extensions header in HTTP response",
    ));
}
```

### 補足: RFC セクションの解釈

RFC 6455 内部で以下の記述が存在する:

| セクション | 規定 |
|---|---|
| Section 4.2.2 Step 6 | サーバーレスポンスで複数の拡張を複数行に分割してよい |
| Section 11.3.2 | `Sec-WebSocket-Extensions` ヘッダフィールドは HTTP レスポンスに複数回現れてはならない (MUST NOT) |

両者は規範的に矛盾している。この矛盾の解消は RFC 9110 Section 5.2（Field Lines）に
依拠する: RFC 9110 は同名ヘッダーの複数行を単一のフィールド値に統合することを
許容しており、仮に Section 11.3.2 の MUST NOT を厳格に解釈しても、
マージ後の論理的なヘッダー値は 1 つであるため矛盾は生じない。

リクエスト側では既に複数行マージが実装されており、レスポンス側でも同様に扱う。
実装として複数行を拒否するメリットはなく、リクエスト側との不整合を解消する。

## RFC 根拠

RFC 6455 Section 4.2.2 Step 6 (refs/rfc6455.txt:198-202):

> 1. Optionally, the |Server| MAY indicate one or more protocol extensions
>     that the server is using and that the client MUST use when talking to
>     the server.  If multiple extensions are to be used, they can all be
>     listed in a single |Sec-WebSocket-Extensions| header field or split
>     between multiple instances of the |Sec-WebSocket-Extensions| header
>     field.

RFC 9110 Section 5.3 (Field Order):
同名ヘッダーが複数行の場合はリスト値として統合する。

## 現状のリクエスト側実装

`validate_request` (line 410) では正しくマージして処理している:

```rust
let extension_values = request.get_headers("Sec-WebSocket-Extensions");
// RFC 9110 Section 5.5: 同名ヘッダーが複数行の場合はリスト値として統合する
let extensions: Vec<String> = values
    .iter()
    .flat_map(|v| v.split(','))
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .collect();
```

なおリクエスト側のコメントも合わせて修正する:

- line 407: `// RFC 6455 Section 9.1: 複数の Sec-WebSocket-Extensions ヘッダー行は許容される` →
  `// RFC 9110 Section 5.2: 同名ヘッダーが複数行の場合はリスト値として統合する`
  （Section 9.1 は ABNF 定義であり複数行許容の根拠ではない。実装は RFC 9110 のフィールド統合に依拠している）
- line 408: `// RFC 9110 Section 5.3` → `// RFC 9110 Section 5.3`
  （節番号はそのままだが、記述の位置が間違っている場合は確認の上修正）

## 修正方針

レスポンス側 (`websocket_handshake.rs:621-626`) のマージロジックは
リクエスト側と完全に同一であり、変更不要。修正は以下の 4 点:

1. `validate_response` 内の `if extension_values.len() > 1 { return Err(...) }` の拒否チェック
   (lines 609-615) を削除する。削除後のコメントは以下とする:
   `// RFC 6455 Section 4.2.2 Step 6: 複数行の Sec-WebSocket-Extensions を許容。RFC 9110 Section 5.2 により統合する`
2. リクエスト側 line 407-408 の RFC 参照コメントを修正する（前述）

## 安全性の評価

- マージ後も `validate_extension_entry` (line 635) が各エントリの ABNF 適合性を検証する
- `!extension_values.is_empty() && extensions.is_empty()` チェック (line 629) は
  マージ後の extensions が空の場合に正しく発火する（複数行マージ後も動作は変わらない）
- マージ結果は `Vec<String>` であり、下流の各 Connection は個別に検証するため影響を受けない。
  複数行の各々に permessage-deflate が含まれていた場合、マージ後に複数の permessage-deflate エントリが
  生じるが、これは issue 0012（クライアント側）および issue 0015 のサーバー側実装で
  重複検出され正しく拒否される

## 既存テストの修正

`pbt/tests/prop_handshake.rs:1246-1268` に `prop_response_duplicate_extension_headers_rejected`
というテストが存在し、現在の動作（複数行の Sec-WebSocket-Extensions を拒否）を検証している。
修正後は複数行を受理するため、以下の対応が必要:

1. 既存テストを `prop_response_multiple_extension_headers_accepted` に改名し、
   `prop_assert!(result.is_ok())` に変更する
2. マージ後の `extensions` ベクタに両方の拡張が含まれることを確認するアサーションを追加する
3. 以下のエッジケースをカバーするテストを追加:
   - 2 行とも有効な異なる拡張（正常受理）
   - 1 行目が空、2 行目が有効（マージ後は 1 要素）
   - 2 行とも空/無効（`malformed` エラーが発火することの確認）

## CHANGES.md 登録内容

- [FIX] サーバーレスポンスで複数行の Sec-WebSocket-Extensions を誤って拒否する問題を修正する
  - @実装者名
