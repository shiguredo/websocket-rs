//! WebSocket echo サーバー (e2e テスト用)

use shiguredo_websocket::{
    ConnectionEvent, ConnectionOutput, ConnectionState, ServerConnectionOptions,
    WebSocketServerConnection,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// echo サーバーを起動し、バインドしたポート番号を返す
///
/// `TcpListener` を全インターフェース (`0.0.0.0:0`) でバインドして
/// ランダムポートを取得し、`tokio::spawn` で echo ループを起動する。
/// Docker コンテナから `host.docker.internal` 経由で接続するため、
/// ループバック (`127.0.0.1`) ではなく全インターフェースにバインドする。
pub async fn spawn_echo_server() -> u16 {
    let listener = TcpListener::bind("0.0.0.0:0")
        .await
        .expect("failed to bind echo server");
    let port = listener
        .local_addr()
        .expect("failed to get local addr")
        .port();

    tokio::spawn(async move {
        loop {
            let (stream, _addr) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => break,
            };
            tokio::spawn(handle_connection(stream));
        }
    });

    port
}

async fn handle_connection(mut stream: tokio::net::TcpStream) {
    let options = ServerConnectionOptions::new().ping_interval(0);
    let mut ws = WebSocketServerConnection::new(options);

    let mut buf = vec![0u8; 8192];
    let mut handshake_done = false;

    loop {
        let n = match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };

        if ws.feed_recv_buf(&buf[..n]).is_err() {
            // エラー時でも出力キューに応答が溜まっている場合がある
            while let Some(output) = ws.poll_output() {
                match output {
                    ConnectionOutput::SendData(data) if stream.write_all(&data).await.is_err() => {
                        return;
                    }
                    ConnectionOutput::CloseConnection => return,
                    _ => {}
                }
            }
            break;
        }

        // ハンドシェイクの自動受諾
        if !handshake_done
            && ws.state() == ConnectionState::Connecting
            && ws.handshake_request().is_some()
        {
            if ws.accept_handshake_auto().is_err() {
                break;
            }
            handshake_done = true;
        }

        while let Some(event) = ws.poll_event() {
            match event {
                ConnectionEvent::TextMessage(text) if ws.send_text(&text).is_err() => {
                    return;
                }
                ConnectionEvent::BinaryMessage(data) if ws.send_binary(&data).is_err() => {
                    return;
                }
                _ => {}
            }
        }

        while let Some(output) = ws.poll_output() {
            match output {
                ConnectionOutput::SendData(data) if stream.write_all(&data).await.is_err() => {
                    return;
                }
                ConnectionOutput::CloseConnection => {
                    return;
                }
                _ => {}
            }
        }

        if ws.state() == ConnectionState::Closed {
            break;
        }
    }
}
