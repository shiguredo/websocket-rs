---
name: shiguredo-websocket
description: 時雨堂の Sans I/O WebSocket ライブラリ shiguredo_websocket の機能・API リファレンス。HTTP/1.1 上の WebSocket クライアント/サーバー接続、ハンドシェイク検証、フレームエンコード/デコード、permessage-deflate (RFC 7692)、自動 Ping/Pong、Close ハンドシェイク、CSWSH 対策に関する質問時に使用。
---

# shiguredo_websocket

Sans I/O 設計に基づく WebSocket (RFC 6455) ライブラリ。HTTP/1.1 上の WebSocket のみを扱う。

## 特徴

- **Sans I/O**: I/O を完全に分離した設計 (Tokio, async-std, 同期 I/O など任意の環境で使用可能)
- **クライアント/サーバー両対応**: `WebSocketClientConnection` / `WebSocketServerConnection`
- **permessage-deflate (RFC 7692)**: サーバー/クライアント双方の context takeover および max_window_bits パラメータをネゴシエート
- **自動 Ping/Pong**: `ping_interval` 設定で定期 Ping 送信、受信 Ping への自動 Pong 応答
- **Close ハンドシェイク**: RFC 6455 Section 7.1 準拠の双方向 Close
- **状態管理**: `Disconnected` / `Connecting` / `Connected` / `Closing` / `Closed`
- **DoS 対策**: 最大フレームサイズ・最大メッセージサイズ・最大解凍サイズの上限設定
- **CSWSH 対策**: ハンドシェイク自動受諾は `accept_handshake_auto`、Origin/path を自前検証する場合は `handshake_request()` + `accept_handshake()`
- **HTTP/1.1 部分は `shiguredo_http11` に委譲**: ハンドシェイクのリクエスト/レスポンス処理は同 crate のデコーダーを利用

## バージョン情報

- crate 名: `shiguredo_websocket`
- バージョン: 2026.2.0
- Rust Edition: 2024
- 最小 Rust バージョン: 1.88
- ライセンス: Apache-2.0

## 依存

- `aws-lc-rs`: Sec-WebSocket-Accept の SHA-1 計算
- `base64ct`: Sec-WebSocket-Key / Sec-WebSocket-Accept の base64 エンコード/デコード
- `noflate`: permessage-deflate の DEFLATE 圧縮/展開
- `shiguredo_http11`: HTTP/1.1 ハンドシェイクのリクエスト/レスポンス処理

## コア API

### クライアント接続

| 型 | 説明 | 主要メソッド |
|----|------|-------------|
| `WebSocketClientConnection<R: RandomSource>` | クライアント側 WebSocket 接続 | `new(options, random)`, `state()`, `protocol()`, `extensions()`, `connect()`, `feed_recv_buf(buf, now)`, `send_text(text)`, `send_binary(data)`, `send_ping(data)`, `close(code, reason)`, `handle_timer(timer_id)`, `poll_event()`, `poll_output()` |
| `ClientConnectionOptions` | クライアント接続オプション | `new(host, path)`, `origin(s)`, `protocol(s)` (追加), `deflate(config)`, `header(name, value)`, `ping_interval(millis)`, `max_frame_size(size)`, `max_message_size(size)`, `max_decompressed_size(size)` |
| `RandomSource` (trait) | マスキングキー/ハンドシェイク nonce 用の乱数ソース | `masking_key() -> [u8; 4]`, `nonce() -> [u8; 16]` |

`ClientConnectionOptions` のデフォルト値:

| フィールド | デフォルト値 | 説明 |
|-----------|-------------|------|
| `path` | `"/"` | リクエストパス |
| `host` | `"localhost"` | Host ヘッダー |
| `origin` | `None` | Origin ヘッダー (オプション) |
| `protocols` | `[]` | サブプロトコル候補 |
| `deflate_config` | `None` | permessage-deflate 設定 |
| `additional_headers` | `[]` | 追加ヘッダー |
| `ping_interval_millis` | 30000 | Ping 送信間隔 (0 で無効) |
| `pong_timeout_millis` | 10000 | Pong タイムアウト |
| `close_timeout_millis` | 5000 | クローズタイムアウト |
| `max_frame_size` | 64 MiB | 最大フレームサイズ (`DEFAULT_MAX_FRAME_SIZE`) |
| `max_message_size` | 64 MiB | 最大メッセージサイズ (`DEFAULT_MAX_MESSAGE_SIZE`) |
| `max_decompressed_size` | 16 MiB | 最大解凍サイズ (`DEFAULT_MAX_DECOMPRESSED_SIZE`) |

### サーバー接続

| 型 | 説明 | 主要メソッド |
|----|------|-------------|
| `WebSocketServerConnection` | サーバー側 WebSocket 接続 | `new(options)`, `state()`, `protocol()`, `extensions()`, `handshake_request()`, `feed_recv_buf(buf)`, `accept_handshake_auto()`, `accept_handshake(response)`, `reject_handshake(status_code, reason, headers)`, `send_text(text)`, `send_binary(data)`, `send_ping(data)`, `close(code, reason)`, `handle_timer(timer_id)`, `poll_event()`, `poll_output()` |
| `ServerConnectionOptions` | サーバー接続オプション | `new()`, `protocol(s)` (追加), `deflate(config)`, `header(name, value)`, `ping_interval(millis)`, `max_frame_size(size)`, `max_message_size(size)`, `max_decompressed_size(size)` |

サーバー側に `feed_recv_buf` のタイムスタンプ引数はない (`ping_interval` は `SetTimer` に基づいて呼び出し側が管理する)。

### 接続状態とイベント

| 型 | バリアント | 説明 |
|----|-----------|------|
| `ConnectionState` | `Disconnected`, `Connecting`, `Connected`, `Closing`, `Closed` | 接続状態 |
| `TimerId` | `Ping`, `PongTimeout`, `CloseTimeout` | タイマー識別子 |
| `ConnectionEvent` | `Connected { protocol, extensions }`, `TextMessage(String)`, `BinaryMessage(Vec<u8>)`, `Ping(Vec<u8>)`, `Pong(Vec<u8>)`, `Close { code, reason }`, `StateChanged(ConnectionState)`, `Error(String)` | アプリケーション層に通知するイベント |
| `ConnectionOutput` | `SendData(Vec<u8>)`, `SetTimer { id, duration_millis }`, `ClearTimer { id }`, `CloseConnection` | I/O 層が実行すべきアクション |

`feed_recv_buf` は RFC 6455 Section 7.1.7 に従い、一度 `Err` を返したら以降の呼び出しも即座に `Err` を返す (内部 `failed` フラグ)。

### 時刻

| 型 | 説明 | 主要メソッド |
|----|------|-------------|
| `Timestamp` | UNIX エポックからのミリ秒 | `from_millis(u64)`, `as_millis()`, `saturating_sub(other)`, `add_millis(millis)`, `Add<u64>`, `Sub<Timestamp>` |

クライアントの `feed_recv_buf` は現在時刻 (`Timestamp`) を引数に取る。Sans I/O のため呼び出し側で時計を渡す。

### フレーム (低レベル API)

| 型 | 説明 | 主要メソッド |
|----|------|-------------|
| `Frame` | WebSocket フレーム (`fin`, `rsv1`-`rsv3`, `opcode`, `payload`) | `new(opcode, payload)`, `text(&str)`, `binary(Vec<u8>)`, `ping(Vec<u8>)` (≤125 バイト, Result), `pong(Vec<u8>)` (≤125 バイト, Result), `close(code, reason)` (理由 ≤123 バイト, Result), `encode(masking_key)`, `encode_unmasked()` |
| `DecodedFrame` | デコード済みフレーム + マスク情報 (`frame`, `masked`) | - |
| `FrameDecoder` | 状態を持つフレームデコーダー | `new()`, `feed(data)`, `decode() -> Result<Option<Frame>>`, `decode_with_info() -> Result<Option<DecodedFrame>>`, `clear()`, `buffer_len()` |
| `Opcode` | フレームオペコード (RFC 6455 §5.2) | `Continuation = 0x0`, `Text = 0x1`, `Binary = 0x2`, `Close = 0x8`, `Ping = 0x9`, `Pong = 0xA`, `from_u8(u8)`, `as_u8()`, `is_control()`, `is_data()` |
| `CloseCode` | クローズコード (RFC 6455 §7.4) | 定数 `NORMAL`(1000), `GOING_AWAY`(1001), `PROTOCOL_ERROR`(1002), `UNSUPPORTED_DATA`(1003), `RESERVED`(1004), `NO_STATUS_RECEIVED`(1005), `ABNORMAL_CLOSURE`(1006), `INVALID_PAYLOAD`(1007), `POLICY_VIOLATION`(1008), `MESSAGE_TOO_BIG`(1009), `MANDATORY_EXTENSION`(1010), `INTERNAL_ERROR`(1011), `TLS_HANDSHAKE`(1015) ほか, `new(u16)`, `as_u16()`, `is_valid()`, `is_sendable()` |

`is_sendable()` は `0..=999` / `1004` / `1005` / `1006` / `1015` / `2000..=2999` / `5000..` を拒否する (RFC 6455 §7.4.1)。`close()` API は内部で `is_sendable()` を検証する。

### ハンドシェイク (低レベル API)

| 型 | 説明 | 主要メソッド |
|----|------|-------------|
| `HandshakeRequest` | クライアント側リクエストビルダー | `new(path, host)`, `origin(s)`, `protocol(s)` (追加), `extension(s)` (追加), `header(name, value)`, `build(nonce) -> Result<Vec<u8>>` |
| `HandshakeValidator` | クライアント側レスポンスバリデーター | `new(nonce)`, `feed(data)`, `remaining() -> &[u8]`, `validate() -> Result<Option<HandshakeResponse>>` |
| `HandshakeResponse` | クライアント側ハンドシェイク結果 | `protocol`, `extensions` |
| `ServerHandshakeRequest` | サーバー側受信リクエスト | `path`, `host`, `origin`, `protocols`, `extensions`, `key` |
| `ServerHandshakeResponse` | サーバー側レスポンスビルダー | `new()`, `protocol(s)`, `extension(s)` (追加), `header(name, value)` |
| `HandshakeRequestValidator` | サーバー側リクエストバリデーター | `new()`, `feed(data)`, `remaining()`, `reset()`, `validate() -> Result<Option<ServerHandshakeRequest>>` |
| 関数 `calculate_accept(nonce: &[u8; 16]) -> String` | nonce から Sec-WebSocket-Accept を計算 | - |
| 関数 `calculate_accept_from_key(key: &str) -> String` | base64 済み Sec-WebSocket-Key から Accept を計算 | - |

`HandshakeValidator::validate` は `decode_headers()` ベースで実装されており、非 101 レスポンスでも `BodyKind::CloseDelimited` のままハンドシェイク判定を返す。101 以外のレスポンスは `Error { kind: ErrorKind::HttpResponse, http_response: Some(...) }` として伝播する (RFC 6455 §4.1)。

### permessage-deflate

| 型 | 説明 | 主要メソッド |
|----|------|-------------|
| `PerMessageDeflateConfig` | 拡張パラメータ (RFC 7692) | `new()`, `server_max_window_bits(u8)` (8-15 にクランプ), `client_max_window_bits(u8)` (8-15), `server_no_context_takeover()`, `client_no_context_takeover()`, `to_extension()`, `from_extension_for_client_response(ext)`, `from_extension_for_server_request(ext)`, `from_extension_validated(...)`, `negotiate(client_request, server_config)` |
| `PerMessageDeflate` | DEFLATE コーデック (送受信両方を保持) | `new_client(config)`, `new_server(config)`, `config()`, `compress(data)`, `decompress(data, max_size)`, `should_compress(data, threshold)` |
| `Compressor` / `Decompressor` | 単方向コーデック | `new(config, is_client)`, `compress(data)` / `decompress(data, max_size)` |
| `Extension` | 拡張ヘッダー要素 (RFC 6455 §9.1) | `new(name)`, `param(name, value)`, `get_param(name)`, `encode()`, `parse(s) -> Vec<Extension>`, `parse_strict(s) -> Result<Vec<Extension>, String>` |
| `ExtensionParam` | 拡張パラメータ (`name`, `value: Option<String>`) | - |

実装上の制約: window_bits は `noflate` の制約により 15 固定。`server_max_window_bits < 15` を offer されると `Error::HandshakeRejected` で接続を拒否する (RFC 7692 §7.2.1)。圧縮は 64 バイト未満のメッセージには適用しない (`COMPRESSION_THRESHOLD`)。

### バッファ拡張トレイト

| トレイト | 実装 | メソッド |
|---------|-----|--------|
| `ByteSliceExt` | `&[u8]` | `read_u8()`, `read_u16()`, `read_u32()`, `read_u64()`, `read_bytes(len)`, `read_utf8(len)` (Big-endian, 不足時 `InsufficientBuffer`) |
| `VecExt` | `Vec<u8>` | `write_u8(v)`, `write_u16(v)`, `write_u32(v)`, `write_u64(v)`, `write_bytes(v)` (Big-endian) |

### エラー型

| バリアント (`ErrorKind`) | 説明 |
|------------------------|------|
| `InvalidInput` | 入力の形式または構造が無効 |
| `InvalidData` | データコンテンツが無効/破損 |
| `InvalidState` | 状態に対して操作を実行不可 |
| `InsufficientBuffer` | 提供バッファが結果を保持できない |
| `Unsupported` | 操作またはデータ形式がサポートされていない |
| `ProtocolViolation` | プロトコル違反 |
| `HandshakeRejected` | ハンドシェイク拒否 |
| `VersionNotSupported` | サポートされていない `Sec-WebSocket-Version` |
| `InvalidUtf8` | 無効な UTF-8 (テキストフレーム/Close 理由) |
| `HttpResponse` | ハンドシェイクで 101 以外のレスポンス (RFC 6455 §4.1)。`error.http_response: Option<Box<HttpResponseInfo>>` に `status_code`, `reason_phrase`, `headers` が入る |

`Error` は `kind`, `reason`, `location` (`#[track_caller]`), `backtrace` (RUST_BACKTRACE 設定時), `close_code`, `http_response` を持つ。

## コード例

### クライアント (同期 I/O)

```rust
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{SystemTime, UNIX_EPOCH};
use shiguredo_websocket::{
    ClientConnectionOptions, ConnectionEvent, ConnectionOutput,
    RandomSource, Timestamp, WebSocketClientConnection,
};

struct AwsLcRandom;

impl RandomSource for AwsLcRandom {
    fn masking_key(&mut self) -> [u8; 4] {
        let mut key = [0u8; 4];
        aws_lc_rs::rand::fill(&mut key).expect("masking_key");
        key
    }
    fn nonce(&mut self) -> [u8; 16] {
        let mut nonce = [0u8; 16];
        aws_lc_rs::rand::fill(&mut nonce).expect("nonce");
        nonce
    }
}

fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    Timestamp::from_millis(millis)
}

let mut socket = TcpStream::connect("echo.websocket.org:80")?;
let options = ClientConnectionOptions::new("echo.websocket.org", "/");
let mut ws = WebSocketClientConnection::new(options, AwsLcRandom);
ws.connect()?;

// Upgrade リクエストを送信
while let Some(output) = ws.poll_output() {
    if let ConnectionOutput::SendData(data) = output {
        socket.write_all(&data)?;
    }
}

// 受信ループ
let mut buf = [0u8; 4096];
loop {
    let n = socket.read(&mut buf)?;
    ws.feed_recv_buf(&buf[..n], now())?;

    while let Some(event) = ws.poll_event() {
        match event {
            ConnectionEvent::Connected { protocol, extensions } => {
                println!("Connected protocol={:?} ext={:?}", protocol, extensions);
            }
            ConnectionEvent::TextMessage(text) => println!("text: {}", text),
            ConnectionEvent::BinaryMessage(data) => println!("bin: {} bytes", data.len()),
            ConnectionEvent::Close { code, reason } => {
                println!("close: {:?} {}", code, reason);
                break;
            }
            _ => {}
        }
    }

    while let Some(output) = ws.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            socket.write_all(&data)?;
        }
    }
}
```

### サーバー (自動受諾)

```rust
use shiguredo_websocket::{
    ConnectionEvent, ConnectionOutput, ConnectionState,
    ServerConnectionOptions, WebSocketServerConnection,
};

let options = ServerConnectionOptions::new().protocol("echo");
let mut ws = WebSocketServerConnection::new(options);

// 受信データをフィード
ws.feed_recv_buf(&received_data)?;

// ハンドシェイクの自動受諾 (CSWSH 対策が不要な閉じた環境のみ)
if ws.state() == ConnectionState::Connecting {
    ws.accept_handshake_auto()?;
}

// イベント処理
while let Some(event) = ws.poll_event() {
    match event {
        ConnectionEvent::TextMessage(text) => {
            ws.send_text(&text)?;
        }
        ConnectionEvent::BinaryMessage(data) => {
            ws.send_binary(&data)?;
        }
        ConnectionEvent::Close { code, reason } => {
            // 必要なら ws.close(...) で応答する
        }
        _ => {}
    }
}

// 出力処理
while let Some(output) = ws.poll_output() {
    match output {
        ConnectionOutput::SendData(data) => { /* 送信 */ }
        ConnectionOutput::SetTimer { id, duration_millis } => { /* タイマー登録 */ }
        ConnectionOutput::ClearTimer { id } => { /* タイマー解除 */ }
        ConnectionOutput::CloseConnection => { /* ソケット close */ }
    }
}
```

### サーバー (CSWSH 対策つき手動受諾)

```rust
use shiguredo_websocket::{
    ConnectionState, ServerConnectionOptions, ServerHandshakeResponse,
    WebSocketServerConnection,
};

let mut ws = WebSocketServerConnection::new(ServerConnectionOptions::new());
ws.feed_recv_buf(&received_data)?;

if ws.state() == ConnectionState::Connecting {
    let request = ws.handshake_request().expect("Connecting なら必ず存在する");

    // CSWSH 対策: Origin と path を自前で検証
    let allowed_origin = request
        .origin
        .as_deref()
        .map(|o| o == "https://example.com")
        .unwrap_or(false);
    if !allowed_origin {
        // RFC 6455 Section 4.4: バージョン不一致以外でも追加ヘッダーは適宜
        ws.reject_handshake(403, "Forbidden", &[])?;
    } else {
        let response = ServerHandshakeResponse::new();
        ws.accept_handshake(response)?;
    }
}
```

`accept_handshake_auto` は Origin/path を検証しないため、ブラウザ + Cookie 認証環境では CSWSH (Cross-Site WebSocket Hijacking) の踏み台になりうる。信頼できないクライアントを受け付ける場合は必ず手動受諾を使うこと。

### メッセージ送信

```rust
use shiguredo_websocket::CloseCode;

// テキスト (UTF-8 として送信)
ws.send_text("hello")?;

// バイナリ
ws.send_binary(&[0x01, 0x02, 0x03])?;

// Ping (≤ 125 バイト)
ws.send_ping(&[])?;

// クローズ。送信禁止コード (1005/1006/1015 等) は `InvalidInput` で拒否される
ws.close(CloseCode::NORMAL, "Goodbye")?;
```

### permessage-deflate を有効にした接続

```rust
use shiguredo_websocket::{
    ClientConnectionOptions, PerMessageDeflateConfig, WebSocketClientConnection,
};

let deflate = PerMessageDeflateConfig::new()
    .client_no_context_takeover()
    .server_no_context_takeover();

let options = ClientConnectionOptions::new("example.com", "/")
    .deflate(deflate);

let mut ws = WebSocketClientConnection::new(options, my_random);
```

サーバー側も `ServerConnectionOptions::deflate(config)` で受諾候補を提示できる。圧縮は 64 バイト未満のメッセージにはかけない。`server_max_window_bits < 15` を要求された場合はハンドシェイク段階で `Error::HandshakeRejected` を返して接続を切る (現実装の `noflate` 制約)。

### タイマー処理

`ConnectionOutput::SetTimer { id, duration_millis }` を受け取ったら I/O 層でタイマーを仕掛け、満了時に `ws.handle_timer(id)` を呼ぶ。`TimerId::Ping` は次回の Ping 送信、`TimerId::PongTimeout` は Pong 不達検出 (`POLICY_VIOLATION` で close)、`TimerId::CloseTimeout` は Close ハンドシェイク打ち切り (`CloseConnection` 出力) に使う。

### フレームの直接操作 (低レベル API)

```rust
use shiguredo_websocket::{Frame, FrameDecoder, Opcode};

// フレーム作成・エンコード (クライアントはマスキング必須)
let frame = Frame::text("hello");
let masking_key = [0x12, 0x34, 0x56, 0x78];
let encoded = frame.encode(masking_key);

// フレームデコード
let mut decoder = FrameDecoder::new();
decoder.feed(&encoded);

while let Some(frame) = decoder.decode()? {
    match frame.opcode {
        Opcode::Text => {
            let text = String::from_utf8_lossy(&frame.payload);
            println!("text: {}", text);
        }
        Opcode::Binary => println!("bin: {} bytes", frame.payload.len()),
        Opcode::Ping | Opcode::Pong | Opcode::Close => { /* 制御フレーム */ }
        Opcode::Continuation => { /* 継続フレーム */ }
    }
}
```

### ハンドシェイクの直接操作 (低レベル API)

```rust
use shiguredo_websocket::{
    HandshakeRequest, HandshakeRequestValidator, HandshakeValidator,
    calculate_accept_from_key,
};

// クライアント側
let request = HandshakeRequest::new("/", "example.com")
    .protocol("chat")
    .extension("permessage-deflate");
let nonce = [0u8; 16];
let bytes = request.build(nonce)?;
// nonce はクライアント側で保持し、レスポンス検証に再利用する

let mut validator = HandshakeValidator::new(nonce);
validator.feed(&received);
if let Some(response) = validator.validate()? {
    let leftover = validator.remaining(); // ハンドシェイク以降のフレームデータ
}

// サーバー側
let mut request_validator = HandshakeRequestValidator::new();
request_validator.feed(&received);
if let Some(request) = request_validator.validate()? {
    let accept = calculate_accept_from_key(&request.key);
    // 任意のレスポンスを構築して送信
}
```

### 非 101 レスポンスの処理

```rust
use shiguredo_websocket::{ErrorKind, WebSocketClientConnection};

match ws.feed_recv_buf(&buf, now) {
    Ok(()) => {}
    Err(err) if err.kind == ErrorKind::HttpResponse => {
        if let Some(info) = err.http_response.as_deref() {
            eprintln!("HTTP {} {}", info.status_code, info.reason_phrase);
            for (name, value) in &info.headers {
                eprintln!("  {}: {}", name, value);
            }
        }
    }
    Err(err) => return Err(err.into()),
}
```

## RFC 準拠

| RFC | 名称 | 対応機能 |
|-----|------|---------|
| RFC 6455 | The WebSocket Protocol | ハンドシェイク、フレーム、マスキング、フラグメンテーション、Close ハンドシェイク、Ping/Pong、UTF-8 検証 |
| RFC 7692 | Compression Extensions for WebSocket | permessage-deflate (server/client_no_context_takeover, server/client_max_window_bits)

実装メモ:

- RFC 6455 §4.1: 101 以外の HTTP レスポンスは `ErrorKind::HttpResponse` で `HttpResponseInfo` を伴って通知される。HTTP procedures はアプリケーション層で処理する。
- RFC 6455 §4.2.1: Sec-WebSocket-Version は `13` 固定。それ以外は `VersionNotSupported`。
- RFC 6455 §7.1.7: `feed_recv_buf` が `Err` を返した後は以降の呼び出しも即座に `Err` を返す (`failed` フラグ)。
- RFC 6455 §7.4.1: 送信禁止コード (1004, 1005, 1006, 1015, 2000-2999, 5000+) は `CloseCode::is_sendable()` で拒否される。
- RFC 6455 §9.1: 不正な `Sec-WebSocket-Extensions` (parse_strict 失敗) はハンドシェイク失敗。
- RFC 7692 §7.1.2.1: クライアントが `server_max_window_bits` を offer しなかった場合、サーバーがレスポンスに含めてはならない。サーバーが offer 値を超える値で応答するのも拒否。
- RFC 7692 §7.1.2.2: クライアントが `client_max_window_bits` を offer していない場合、サーバーがレスポンスに含めてはならない。

## 既知の制限事項

- **window_bits は 15 固定**: `noflate` 制約により `server_max_window_bits < 15` の要求はハンドシェイク段階で拒否する。
- **`Sec-WebSocket-Extensions` の quoted-string**: 現在は `,` で単純分割しており、`extension-param` 値が quoted-string の場合に誤分割する可能性がある。permessage-deflate (RFC 7692) のパラメータはすべて token のため実害はないが、将来の拡張実装時には RFC 7230 §7 の `#rule` に準拠した stateful パーサーへの置き換えが必要。
- **HTTP/1.1 のみ**: HTTP/2 (RFC 8441) や HTTP/3 (RFC 9220) 上の WebSocket には対応していない。
- **`PerMessageDeflateConfig::from_extension` は deprecated**: 検証が不十分なため、`from_extension_for_client_response` / `from_extension_for_server_request` / `from_extension_validated` を使うこと。
