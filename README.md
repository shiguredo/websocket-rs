# websocket-rs

[![shiguredo_websocket](https://img.shields.io/crates/v/shiguredo_websocket.svg)](https://crates.io/crates/shiguredo_websocket)
[![Documentation](https://docs.rs/shiguredo_websocket/badge.svg)](https://docs.rs/shiguredo_websocket)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

## About Shiguredo's open source software

We will not respond to PRs or issues that have not been discussed on Discord. Also, Discord is only available in Japanese.

Please read <https://github.com/shiguredo/oss> before use.

## 時雨堂のオープンソースソフトウェアについて

利用前に <https://github.com/shiguredo/oss> をお読みください。

## 概要

Rust で実装された Sans I/O な WebSocket ライブラリです。

## 特徴

- Sans I/O
  - <https://sans-io.readthedocs.io/index.html>
- HTTP/1.1 対応

## 使い方

### クライアント (WebSocket 接続)

```rust
use std::io::{Read, Write};
use std::net::TcpStream;
use shiguredo_websocket::{
    ClientConnectionOptions, ConnectionEvent, ConnectionOutput,
    RandomSource, WebSocketClientConnection, Timestamp,
};

// 乱数ソースの実装
struct SecureRandom;

impl RandomSource for SecureRandom {
    fn masking_key(&mut self) -> [u8; 4] {
        let mut key = [0u8; 4];
        getrandom::fill(&mut key).expect("failed to generate masking key");
        key
    }

    fn nonce(&mut self) -> [u8; 16] {
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).expect("failed to generate nonce");
        nonce
    }
}

// TCP ソケット接続
let mut socket = TcpStream::connect("echo.websocket.org:80")?;

// WebSocket 接続オプション
let options = ClientConnectionOptions::new("echo.websocket.org", "/");

// WebSocket 接続作成・開始
let mut ws = WebSocketClientConnection::new(options, SecureRandom);
ws.connect()?;

// HTTP Upgrade リクエスト送信
while let Some(output) = ws.poll_output() {
    if let ConnectionOutput::SendData(data) = output {
        socket.write_all(&data)?;
    }
}

// 現在時刻を取得するヘルパー関数
fn now() -> Timestamp {
    use std::time::{SystemTime, UNIX_EPOCH};
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    Timestamp::from_millis(millis)
}

// ハンドシェイクレスポンス受信
let mut buf = [0u8; 4096];
loop {
    let n = socket.read(&mut buf)?;
    ws.feed_recv_buf(&buf[..n], now())?;

    while let Some(event) = ws.poll_event() {
        match event {
            ConnectionEvent::Connected { protocol, extensions } => {
                println!("Connected! protocol={:?}", protocol);
            }
            ConnectionEvent::TextMessage(text) => {
                println!("Received: {}", text);
            }
            ConnectionEvent::BinaryMessage(data) => {
                println!("Received: {} bytes", data.len());
            }
            ConnectionEvent::Close { code, reason } => {
                println!("Closed: {:?} {}", code, reason);
                break;
            }
            _ => {}
        }
    }

    // 送信データがあれば送信
    while let Some(output) = ws.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            socket.write_all(&data)?;
        }
    }
}
```

### サーバー (WebSocket 接続受付)

```rust
use shiguredo_websocket::{
    ConnectionEvent, ConnectionOutput, ConnectionState,
    ServerConnectionOptions, WebSocketServerConnection,
};

// WebSocketServerConnection の初期化
let options = ServerConnectionOptions::new();
let mut ws = WebSocketServerConnection::new(options);

// 受信データをフィード
// ws.feed_recv_buf(&received_data)?;

// ハンドシェイクの自動受諾
// if ws.state() == ConnectionState::Connecting {
//     ws.accept_handshake_auto()?;
// }

// イベント処理
// while let Some(event) = ws.poll_event() { ... }

// 出力処理
// while let Some(output) = ws.poll_output() { ... }
```

### メッセージ送信 (クライアント)

```rust
use shiguredo_websocket::CloseCode;

// テキストメッセージ送信
ws.send_text("Hello, WebSocket!").unwrap();

// バイナリメッセージ送信
ws.send_binary(&[0x01, 0x02, 0x03]).unwrap();

// Ping 送信
ws.send_ping(&[]).unwrap();

// 接続を閉じる
ws.close(CloseCode::NORMAL, "Goodbye").unwrap();
```

### メッセージ送信 (サーバー)

```rust
use shiguredo_websocket::CloseCode;

// テキストメッセージ送信
ws.send_text("Hello, WebSocket!").unwrap();

// バイナリメッセージ送信
ws.send_binary(&[0x01, 0x02, 0x03]).unwrap();

// Ping 送信
ws.send_ping(&[]).unwrap();

// 接続を閉じる
ws.close(CloseCode::NORMAL, "Goodbye").unwrap();
```

### フレームの直接操作 (低レベル API)

```rust
use shiguredo_websocket::{Frame, FrameDecoder, Opcode};

// フレーム作成
let frame = Frame::text("Hello");
let masking_key = [0x12, 0x34, 0x56, 0x78];
let encoded = frame.encode(masking_key);

// フレームデコード
let mut decoder = FrameDecoder::new();
decoder.feed(&encoded);

while let Some(frame) = decoder.decode().unwrap() {
    match frame.opcode {
        Opcode::Text => {
            let text = String::from_utf8_lossy(&frame.payload);
            println!("Text: {}", text);
        }
        Opcode::Binary => {
            println!("Binary: {} bytes", frame.payload.len());
        }
        Opcode::Ping => {
            // Pong を返す
        }
        Opcode::Close => {
            // 接続を閉じる
        }
        _ => {}
    }
}
```

## WebSocket

このライブラリが対応している WebSocket の仕組みです。

### フレーム

- テキストフレーム / バイナリフレーム
- 制御フレーム (Ping, Pong, Close)
- フラグメンテーション (継続フレーム)
- マスキング (クライアント→サーバー)

### ハンドシェイク

- HTTP/1.1 Upgrade リクエスト/レスポンス
- Sec-WebSocket-Key / Sec-WebSocket-Accept の検証
- サブプロトコルネゴシエーション (Sec-WebSocket-Protocol)
- 拡張ネゴシエーション (Sec-WebSocket-Extensions)

### 拡張

- permessage-deflate (RFC 7692)
  - server_no_context_takeover
  - client_no_context_takeover
  - server_max_window_bits
  - client_max_window_bits

### 接続管理

- 自動 Ping/Pong 応答
- 定期的な Ping 送信 (設定可能)
- Close ハンドシェイク
- 状態管理 (Connecting, Connected, Closing, Closed)

### セキュリティ

- マスキングキーの検証
- フレームサイズ制限
- UTF-8 検証 (テキストメッセージ)

### 制限 (DoS 対策)

デフォルト値:

- 最大解凍サイズ (Zip Bomb 対策): 16MB

`ClientConnectionOptions` / `ServerConnectionOptions` で `max_decompressed_size` をカスタマイズ可能です。

## サンプル

サンプルは [Tokio](https://github.com/tokio-rs/tokio) と [Rustls](https://github.com/rustls/rustls) を利用しています。引数のライブラリには [noargs](https://github.com/sile/noargs) を利用しています。

### websocket_client

WS/WSS クライアントの例です。

```bash
cargo run -p websocket_client -- wss://localhost:8443/
cargo run -p websocket_client -- ws://localhost:8080/
cargo run -p websocket_client -- wss://localhost:8443/ --insecure
```

**オプション:**

- `<URL>`: 接続先 URL (ws:// または wss://)
- `--insecure`: 自己署名証明書を許可 (WSS のみ)

**機能:**

- WS/WSS リクエスト送信
- エコーメッセージ受信
- permessage-deflate 対応
- rustls-platform-verifier による TLS 検証

### websocket_server

WS/WSS エコーサーバーの例です。

```bash
cargo run -p websocket_server
cargo run -p websocket_server -- --tls --cert cert.pem --key key.pem
```

**オプション:**

- `-p, --port <PORT>`: リッスンポート (デフォルト: 8080, TLS 有効時: 8443)
- `--tls`: WSS 有効化
- `--cert <PATH>`: 証明書ファイル (PEM 形式)
- `--key <PATH>`: 秘密鍵ファイル (PEM 形式)

**機能:**

- WS/WSS 接続受付
- テキスト/バイナリメッセージのエコーバック
- 自動 Pong 応答

### websocket_reverse_proxy

WS/WSS リバースプロキシの例です。

```bash
# WS -> WS
cargo run -p websocket_reverse_proxy -- --upstream ws://localhost:8080/

# WS -> WSS
cargo run -p websocket_reverse_proxy -- --upstream wss://echo.websocket.org/

# WSS -> WSS (TLS 終端)
cargo run -p websocket_reverse_proxy -- --tls --cert cert.pem --key key.pem --upstream wss://echo.websocket.org/
```

**オプション:**

- `-p, --port <PORT>`: リッスンポート (デフォルト: 8080, TLS 有効時: 8443)
- `-u, --upstream <URL>`: 転送先 URL (ws:// または wss://)
- `--tls`: フロントエンドで WSS 有効化
- `--cert <PATH>`: 証明書ファイル (PEM 形式)
- `--key <PATH>`: 秘密鍵ファイル (PEM 形式)
- `--insecure`: アップストリームの自己署名証明書を許可
- `--debug`: デバッグログを有効化

**機能:**

- WS/WSS 接続の双方向プロキシ
- テキスト/バイナリ/Ping/Close メッセージの中継
- permessage-deflate 対応

## 規格書

このライブラリが準拠している RFC 一覧です。

- RFC 6455 - The WebSocket Protocol
  - <https://datatracker.ietf.org/doc/html/rfc6455>
- RFC 7692 - Compression Extensions for WebSocket
  - <https://datatracker.ietf.org/doc/html/rfc7692>

## ライセンス

Apache License 2.0

```text
Copyright 2026-2026, Shiguredo Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
