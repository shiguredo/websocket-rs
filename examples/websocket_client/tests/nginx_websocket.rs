//! nginx WebSocket プロキシ経由の e2e テスト
//!
//! echo サーバーを起動し、nginx コンテナで WebSocket プロキシを構成して、
//! shiguredo_websocket のクライアント・サーバーが実際のプロキシ経由で
//! 正しく動作することを検証する。

// テストバイナリの crate root は tests/ 直下とみなされるため、`#[path]` で
// nginx_websocket/ 配下のヘルパーを明示する。
#[path = "nginx_websocket/helpers.rs"]
mod helpers;

use std::time::{SystemTime, UNIX_EPOCH};

use shiguredo_websocket::{
    ClientConnectionOptions, CloseCode, ConnectionEvent, ConnectionOutput, ConnectionState,
    RandomSource, Timestamp, WebSocketClientConnection,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct SecureRandom;

impl RandomSource for SecureRandom {
    fn masking_key(&mut self) -> [u8; 4] {
        let mut key = [0u8; 4];
        aws_lc_rs::rand::fill(&mut key).expect("masking key の生成に失敗してはならない");
        key
    }

    fn nonce(&mut self) -> [u8; 16] {
        let mut nonce = [0u8; 16];
        aws_lc_rs::rand::fill(&mut nonce).expect("nonce の生成に失敗してはならない");
        nonce
    }
}

fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("システム時刻が UNIX エポックより前になっている")
        .as_millis() as u64;
    Timestamp::from_millis(millis)
}

async fn connect_via_nginx(
    nginx_port: u16,
) -> (TcpStream, WebSocketClientConnection<SecureRandom>) {
    let mut stream = TcpStream::connect(("127.0.0.1", nginx_port))
        .await
        .expect("nginx への接続に失敗してはならない");

    let host = format!("127.0.0.1:{nginx_port}");
    let options = ClientConnectionOptions::new(&host, "/");
    let mut ws = WebSocketClientConnection::new(options, SecureRandom);

    ws.connect()
        .expect("ハンドシェイクの開始に失敗してはならない");

    while let Some(output) = ws.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            stream
                .write_all(&data)
                .await
                .expect("ハンドシェイク要求の送信に失敗してはならない");
        }
    }

    let mut buf = vec![0u8; 4096];
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    loop {
        let n = tokio::time::timeout_at(deadline, stream.read(&mut buf))
            .await
            .expect("ハンドシェイクの応答がタイムアウトした")
            .expect("ハンドシェイク応答の受信に失敗してはならない");
        if n == 0 {
            panic!("connection closed during handshake");
        }

        ws.feed_recv_buf(&buf[..n], now())
            .expect("ハンドシェイク応答の投入に失敗してはならない");

        while let Some(event) = ws.poll_event() {
            match event {
                ConnectionEvent::Error(err) => panic!("error during handshake: {err}"),
                ConnectionEvent::Close { code, reason } => {
                    panic!("unexpected close during handshake: {code:?} {reason}")
                }
                _ => {}
            }
        }

        while let Some(output) = ws.poll_output() {
            if let ConnectionOutput::SendData(data) = output {
                stream
                    .write_all(&data)
                    .await
                    .expect("ハンドシェイク中の送信に失敗してはならない");
            }
        }

        if ws.state() == ConnectionState::Connected {
            break;
        }
    }

    (stream, ws)
}

async fn send_and_recv_text(
    stream: &mut TcpStream,
    ws: &mut WebSocketClientConnection<SecureRandom>,
    text: &str,
) -> String {
    ws.send_text(text)
        .expect("テキストメッセージの送信に失敗してはならない");
    flush_output(stream, ws).await;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    let mut buf = vec![0u8; 8192];
    loop {
        let n = tokio::time::timeout_at(deadline, stream.read(&mut buf))
            .await
            .expect("エコー応答の受信がタイムアウトした")
            .expect("エコー応答の受信に失敗してはならない");
        if n == 0 {
            panic!("connection closed while waiting for echo");
        }

        ws.feed_recv_buf(&buf[..n], now())
            .expect("エコー応答の投入に失敗してはならない");

        while let Some(event) = ws.poll_event() {
            match event {
                ConnectionEvent::TextMessage(received) => return received,
                ConnectionEvent::Error(err) => panic!("error while waiting for echo: {err}"),
                ConnectionEvent::Close { code, reason } => {
                    panic!("unexpected close while waiting for echo: {code:?} {reason}")
                }
                _ => {}
            }
        }

        flush_output(stream, ws).await;
    }
}

async fn send_and_recv_binary(
    stream: &mut TcpStream,
    ws: &mut WebSocketClientConnection<SecureRandom>,
    data: &[u8],
) -> Vec<u8> {
    ws.send_binary(data)
        .expect("バイナリメッセージの送信に失敗してはならない");
    flush_output(stream, ws).await;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    let mut buf = vec![0u8; 8192];
    loop {
        let n = tokio::time::timeout_at(deadline, stream.read(&mut buf))
            .await
            .expect("エコー応答の受信がタイムアウトした")
            .expect("エコー応答の受信に失敗してはならない");
        if n == 0 {
            panic!("connection closed while waiting for echo");
        }

        ws.feed_recv_buf(&buf[..n], now())
            .expect("エコー応答の投入に失敗してはならない");

        while let Some(event) = ws.poll_event() {
            match event {
                ConnectionEvent::BinaryMessage(received) => return received,
                ConnectionEvent::Error(err) => panic!("error while waiting for echo: {err}"),
                ConnectionEvent::Close { code, reason } => {
                    panic!("unexpected close while waiting for echo: {code:?} {reason}")
                }
                _ => {}
            }
        }

        flush_output(stream, ws).await;
    }
}

async fn flush_output(stream: &mut TcpStream, ws: &mut WebSocketClientConnection<SecureRandom>) {
    while let Some(output) = ws.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            stream
                .write_all(&data)
                .await
                .expect("送信データのフラッシュに失敗してはならない");
        }
    }
}

#[tokio::test]
async fn テキストメッセージのエコー() {
    helpers::ensure_docker();
    let echo_port = helpers::echo_server::spawn().await;
    let nginx = helpers::nginx::spawn_ws_proxy(echo_port).await;
    let (mut stream, mut ws) = connect_via_nginx(nginx.port).await;

    let received = send_and_recv_text(&mut stream, &mut ws, "Hello, WebSocket!").await;
    assert_eq!(received, "Hello, WebSocket!");
}

#[tokio::test]
async fn バイナリメッセージのエコー() {
    helpers::ensure_docker();
    let echo_port = helpers::echo_server::spawn().await;
    let nginx = helpers::nginx::spawn_ws_proxy(echo_port).await;
    let (mut stream, mut ws) = connect_via_nginx(nginx.port).await;

    let data = vec![0x00, 0x01, 0x02, 0xFF, 0xFE, 0xFD];
    let received = send_and_recv_binary(&mut stream, &mut ws, &data).await;
    assert_eq!(received, data);
}

#[tokio::test]
async fn 大きなメッセージのエコー() {
    helpers::ensure_docker();
    let echo_port = helpers::echo_server::spawn().await;
    let nginx = helpers::nginx::spawn_ws_proxy(echo_port).await;
    let (mut stream, mut ws) = connect_via_nginx(nginx.port).await;

    // 128 KiB: 16bit 拡張ペイロード長の上限 (65535 バイト) を超え、
    // 64bit 拡張ペイロード長が使われるサイズ
    let data: Vec<u8> = (0..128 * 1024).map(|i| (i % 256) as u8).collect();
    let received = send_and_recv_binary(&mut stream, &mut ws, &data).await;
    assert_eq!(received, data);
}

#[tokio::test]
async fn ping_pong_の往復() {
    helpers::ensure_docker();
    let echo_port = helpers::echo_server::spawn().await;
    let nginx = helpers::nginx::spawn_ws_proxy(echo_port).await;
    let (mut stream, mut ws) = connect_via_nginx(nginx.port).await;

    let ping_data = b"ping-test";
    ws.send_ping(ping_data)
        .expect("Ping の送信に失敗してはならない");
    flush_output(&mut stream, &mut ws).await;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    let mut buf = vec![0u8; 4096];
    loop {
        let n = tokio::time::timeout_at(deadline, stream.read(&mut buf))
            .await
            .expect("Pong の受信がタイムアウトした")
            .expect("Pong の受信に失敗してはならない");
        if n == 0 {
            panic!("connection closed while waiting for pong");
        }

        ws.feed_recv_buf(&buf[..n], now())
            .expect("Pong の投入に失敗してはならない");

        let mut pong_received = false;
        while let Some(event) = ws.poll_event() {
            match event {
                ConnectionEvent::Pong(data) => {
                    assert_eq!(data, ping_data, "Pong ペイロードが Ping と一致しない");
                    pong_received = true;
                }
                ConnectionEvent::Error(err) => {
                    panic!("error while waiting for pong: {err}")
                }
                ConnectionEvent::Close { code, reason } => {
                    panic!("unexpected close while waiting for pong: {code:?} {reason}")
                }
                _ => {}
            }
        }

        flush_output(&mut stream, &mut ws).await;

        if pong_received {
            break;
        }
    }
}

#[tokio::test]
async fn close_ハンドシェイク() {
    helpers::ensure_docker();
    let echo_port = helpers::echo_server::spawn().await;
    let nginx = helpers::nginx::spawn_ws_proxy(echo_port).await;
    let (mut stream, mut ws) = connect_via_nginx(nginx.port).await;

    ws.close(CloseCode::NORMAL, "")
        .expect("Close の送信に失敗してはならない");
    flush_output(&mut stream, &mut ws).await;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    let mut buf = vec![0u8; 4096];
    let mut close_received = false;
    loop {
        let n = tokio::time::timeout_at(deadline, stream.read(&mut buf))
            .await
            .expect("Close 応答の受信がタイムアウトした")
            .expect("Close 応答の受信に失敗してはならない");
        if n == 0 {
            break;
        }

        ws.feed_recv_buf(&buf[..n], now())
            .expect("Close 応答の投入に失敗してはならない");

        while let Some(event) = ws.poll_event() {
            match event {
                ConnectionEvent::Close { code, reason } => {
                    assert_eq!(
                        code,
                        Some(CloseCode::NORMAL),
                        "Close ステータスコードが 1000 でない"
                    );
                    assert_eq!(reason, "", "サーバーの自動応答の reason は空文字");
                    close_received = true;
                }
                ConnectionEvent::Error(err) => {
                    panic!("error while waiting for close: {err}")
                }
                _ => {}
            }
        }

        flush_output(&mut stream, &mut ws).await;

        if ws.state() == ConnectionState::Closed || close_received {
            break;
        }
    }

    assert!(close_received, "Close 応答を受信できなかった");
}
