//! WebSocket エコーサーバーの例 (tokio + tokio-rustls)
//!
//! 使い方:
//!   # WS サーバー (ポート 8080)
//!   cargo run -p websocket_server
//!
//!   # WSS サーバー (ポート 8443)
//!   cargo run -p websocket_server -- --tls --cert cert.pem --key key.pem

use std::sync::Arc;

use rustls::ServerConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use shiguredo_websocket::{
    ConnectionEvent, ConnectionOutput, ConnectionState, ServerConnectionOptions,
    WebSocketServerConnection,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

struct ServerOptions {
    port: u16,
    tls: bool,
    cert_path: Option<String>,
    key_path: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_args()?;

    let addr = format!("0.0.0.0:{}", options.port);
    let listener = TcpListener::bind(&addr).await?;

    if options.tls {
        let cert_path = options
            .cert_path
            .as_ref()
            .ok_or("--cert is required for TLS")?;
        let key_path = options
            .key_path
            .as_ref()
            .ok_or("--key is required for TLS")?;

        let config = load_tls_config(cert_path, key_path)?;
        let acceptor = TlsAcceptor::from(Arc::new(config));

        println!("WSS server listening on wss://{}", addr);

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            let acceptor = acceptor.clone();

            tokio::spawn(async move {
                match acceptor.accept(stream).await {
                    Ok(tls_stream) => {
                        if let Err(e) = handle_websocket_connection(tls_stream, peer_addr).await {
                            eprintln!("WebSocket error from {}: {}", peer_addr, e);
                        }
                    }
                    Err(e) => eprintln!("TLS handshake error from {}: {}", peer_addr, e),
                }
            });
        }
    } else {
        println!("WS server listening on ws://{}", addr);

        loop {
            let (stream, peer_addr) = listener.accept().await?;

            tokio::spawn(async move {
                if let Err(e) = handle_websocket_connection(stream, peer_addr).await {
                    eprintln!("WebSocket error from {}: {}", peer_addr, e);
                }
            });
        }
    }
}

fn parse_args() -> Result<ServerOptions, Box<dyn std::error::Error>> {
    let mut args = noargs::raw_args();
    args.metadata_mut().app_name = env!("CARGO_PKG_NAME");
    args.metadata_mut().app_description = "WebSocket echo server";

    // --version フラグ
    if noargs::VERSION_FLAG.take(&mut args).is_present() {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    // --help フラグ
    noargs::HELP_FLAG.take_help(&mut args);

    // --tls フラグ
    let tls: bool = noargs::flag("tls")
        .doc("Enable WSS (WebSocket Secure)")
        .take(&mut args)
        .is_present();

    // --port オプション (デフォルトは TLS の有無で変わる)
    let default_port = if tls { "8443" } else { "8080" };
    let port: u16 = noargs::opt("port")
        .short('p')
        .doc("Port to listen on (default: 8080, or 8443 with --tls)")
        .default(default_port)
        .take(&mut args)
        .then(|o| o.value().parse())
        .map_err(|e| format!("{:?}", e))?;

    // --cert オプション
    let cert_path: Option<String> = noargs::opt("cert")
        .doc("Path to certificate file (PEM)")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(o.value().to_string()))
        .map_err(|e| format!("{:?}", e))?;

    // --key オプション
    let key_path: Option<String> = noargs::opt("key")
        .doc("Path to private key file (PEM)")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(o.value().to_string()))
        .map_err(|e| format!("{:?}", e))?;

    // 未知の引数があればエラー
    if let Some(help) = args.finish().map_err(|e| format!("{:?}", e))? {
        print!("{}", help);
        std::process::exit(0);
    }

    Ok(ServerOptions {
        port,
        tls,
        cert_path,
        key_path,
    })
}

fn load_tls_config(
    cert_path: &str,
    key_path: &str,
) -> Result<ServerConfig, Box<dyn std::error::Error>> {
    let certs: Vec<CertificateDer<'static>> =
        CertificateDer::pem_file_iter(cert_path)?.collect::<Result<Vec<_>, _>>()?;

    if certs.is_empty() {
        return Err("No certificates found in cert file".into());
    }

    let key = PrivateKeyDer::from_pem_file(key_path)?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    Ok(config)
}

async fn handle_websocket_connection<S>(
    mut stream: S,
    peer_addr: std::net::SocketAddr,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    println!("Connection from {}", peer_addr);

    // WebSocketServerConnection の初期化
    let options = ServerConnectionOptions::new().ping_interval(30_000);
    let mut ws = WebSocketServerConnection::new(options);

    let mut buf = vec![0u8; 8192];
    let mut handshake_done = false;

    loop {
        // データ受信
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            println!("Connection closed by {}", peer_addr);
            break;
        }

        // WebSocket にフィード
        ws.feed_recv_buf(&buf[..n])?;

        // ハンドシェイクの自動受諾
        if !handshake_done && ws.state() == ConnectionState::Connecting {
            ws.accept_handshake_auto()?;
            handshake_done = true;
        }

        // イベント処理
        while let Some(event) = ws.poll_event() {
            match event {
                ConnectionEvent::Connected {
                    protocol,
                    extensions,
                } => {
                    println!("WebSocket connected from {}", peer_addr);
                    if let Some(p) = protocol {
                        println!("  Protocol: {}", p);
                    }
                    if !extensions.is_empty() {
                        println!("  Extensions: {:?}", extensions);
                    }
                }
                ConnectionEvent::TextMessage(text) => {
                    println!("Received text from {}: {}", peer_addr, text);
                    // エコーバック
                    ws.send_text(&text)?;
                }
                ConnectionEvent::BinaryMessage(data) => {
                    println!("Received binary from {}: {} bytes", peer_addr, data.len());
                    // エコーバック
                    ws.send_binary(&data)?;
                }
                ConnectionEvent::Ping(_data) => {
                    println!("Ping from {}", peer_addr);
                    // Pong は自動送信される
                }
                ConnectionEvent::Pong(_data) => {
                    println!("Pong from {}", peer_addr);
                }
                ConnectionEvent::Close { code, reason } => {
                    println!("Close from {}: {:?} {}", peer_addr, code, reason);
                }
                ConnectionEvent::Error(err) => {
                    eprintln!("Error from {}: {}", peer_addr, err);
                }
                ConnectionEvent::StateChanged(state) => {
                    println!("State changed to {:?} for {}", state, peer_addr);
                }
            }
        }

        // 出力処理
        while let Some(output) = ws.poll_output() {
            match output {
                ConnectionOutput::SendData(data) => {
                    stream.write_all(&data).await?;
                }
                ConnectionOutput::SetTimer {
                    id: _id,
                    duration_millis: _duration_millis,
                } => {
                    // タイマー管理は簡略化（TODO: 必要に応じて実装）
                }
                ConnectionOutput::ClearTimer { id: _id } => {
                    // タイマー管理は簡略化
                }
                ConnectionOutput::CloseConnection => {
                    println!("Closing connection to {}", peer_addr);
                    return Ok(());
                }
            }
        }

        // 接続終了判定
        if ws.state() == ConnectionState::Closed {
            break;
        }
    }

    Ok(())
}
