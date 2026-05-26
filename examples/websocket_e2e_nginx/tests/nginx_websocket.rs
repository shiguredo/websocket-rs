//! nginx WebSocket プロキシ経由の e2e テスト
//!
//! echo サーバーを起動し、nginx コンテナで WebSocket プロキシを構成して、
//! shiguredo_websocket のクライアント・サーバーが実際のプロキシ経由で
//! 正しく動作することを検証する。

mod helpers;

use std::time::{SystemTime, UNIX_EPOCH};

use shiguredo_websocket::{
    ClientConnectionOptions, CloseCode, ConnectionEvent, ConnectionOutput, ConnectionState,
    RandomSource, Timestamp, WebSocketClientConnection,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use websocket_e2e_nginx::spawn_echo_server;

struct SecureRandom;

impl RandomSource for SecureRandom {
    fn masking_key(&mut self) -> [u8; 4] {
        let mut key = [0u8; 4];
        aws_lc_rs::rand::fill(&mut key).expect("failed to generate masking key");
        key
    }

    fn nonce(&mut self) -> [u8; 16] {
        let mut nonce = [0u8; 16];
        aws_lc_rs::rand::fill(&mut nonce).expect("failed to generate nonce");
        nonce
    }
}

fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before UNIX epoch")
        .as_millis() as u64;
    Timestamp::from_millis(millis)
}

async fn connect_via_nginx(
    nginx_port: u16,
) -> (TcpStream, WebSocketClientConnection<SecureRandom>) {
    let mut stream = TcpStream::connect(("127.0.0.1", nginx_port))
        .await
        .expect("failed to connect to nginx");

    let host = format!("127.0.0.1:{nginx_port}");
    let options = ClientConnectionOptions::new(&host, "/");
    let mut ws = WebSocketClientConnection::new(options, SecureRandom);

    ws.connect().expect("failed to initiate handshake");

    while let Some(output) = ws.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            stream
                .write_all(&data)
                .await
                .expect("failed to send handshake request");
        }
    }

    let mut buf = vec![0u8; 4096];
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    loop {
        let n = tokio::time::timeout_at(deadline, stream.read(&mut buf))
            .await
            .expect("handshake timed out")
            .expect("failed to read handshake response");
        if n == 0 {
            panic!("connection closed during handshake");
        }

        ws.feed_recv_buf(&buf[..n], now())
            .expect("failed to feed handshake response");

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
                    .expect("failed to send during handshake");
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
    ws.send_text(text).expect("failed to send text");
    flush_output(stream, ws).await;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    let mut buf = vec![0u8; 8192];
    loop {
        let n = tokio::time::timeout_at(deadline, stream.read(&mut buf))
            .await
            .expect("echo reception timed out")
            .expect("failed to read echo");
        if n == 0 {
            panic!("connection closed while waiting for echo");
        }

        ws.feed_recv_buf(&buf[..n], now())
            .expect("failed to feed echo data");

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
    ws.send_binary(data).expect("failed to send binary");
    flush_output(stream, ws).await;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    let mut buf = vec![0u8; 8192];
    loop {
        let n = tokio::time::timeout_at(deadline, stream.read(&mut buf))
            .await
            .expect("echo reception timed out")
            .expect("failed to read echo");
        if n == 0 {
            panic!("connection closed while waiting for echo");
        }

        ws.feed_recv_buf(&buf[..n], now())
            .expect("failed to feed echo data");

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
                .expect("failed to flush output");
        }
    }
}

#[tokio::test]
async fn テキストメッセージのエコー() {
    helpers::ensure_docker();
    let echo_port = spawn_echo_server().await;
    let nginx = helpers::spawn_nginx_ws_proxy(echo_port).await;
    let (mut stream, mut ws) = connect_via_nginx(nginx.port).await;

    let received = send_and_recv_text(&mut stream, &mut ws, "Hello, WebSocket!").await;
    assert_eq!(received, "Hello, WebSocket!");
}

#[tokio::test]
async fn バイナリメッセージのエコー() {
    helpers::ensure_docker();
    let echo_port = spawn_echo_server().await;
    let nginx = helpers::spawn_nginx_ws_proxy(echo_port).await;
    let (mut stream, mut ws) = connect_via_nginx(nginx.port).await;

    let data = vec![0x00, 0x01, 0x02, 0xFF, 0xFE, 0xFD];
    let received = send_and_recv_binary(&mut stream, &mut ws, &data).await;
    assert_eq!(received, data);
}

#[tokio::test]
async fn 大きなメッセージのエコー() {
    helpers::ensure_docker();
    let echo_port = spawn_echo_server().await;
    let nginx = helpers::spawn_nginx_ws_proxy(echo_port).await;
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
    let echo_port = spawn_echo_server().await;
    let nginx = helpers::spawn_nginx_ws_proxy(echo_port).await;
    let (mut stream, mut ws) = connect_via_nginx(nginx.port).await;

    let ping_data = b"ping-test";
    ws.send_ping(ping_data).expect("failed to send ping");
    flush_output(&mut stream, &mut ws).await;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    let mut buf = vec![0u8; 4096];
    loop {
        let n = tokio::time::timeout_at(deadline, stream.read(&mut buf))
            .await
            .expect("pong reception timed out")
            .expect("failed to read pong");
        if n == 0 {
            panic!("connection closed while waiting for pong");
        }

        ws.feed_recv_buf(&buf[..n], now())
            .expect("failed to feed pong data");

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
    let echo_port = spawn_echo_server().await;
    let nginx = helpers::spawn_nginx_ws_proxy(echo_port).await;
    let (mut stream, mut ws) = connect_via_nginx(nginx.port).await;

    ws.close(CloseCode::NORMAL, "")
        .expect("failed to send close");
    flush_output(&mut stream, &mut ws).await;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    let mut buf = vec![0u8; 4096];
    let mut close_received = false;
    loop {
        let n = tokio::time::timeout_at(deadline, stream.read(&mut buf))
            .await
            .expect("close response timed out")
            .expect("failed to read close response");
        if n == 0 {
            break;
        }

        ws.feed_recv_buf(&buf[..n], now())
            .expect("failed to feed close data");

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
