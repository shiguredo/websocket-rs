//! WebSocket クライアントサンプル (WS / WSS)
//!
//! rustls + tokio を使用した WebSocket クライアントの実装例
//!
//! 使い方:
//!   # WSS
//!   cargo run -p websocket_client -- wss://localhost:8443/
//!
//!   # WS
//!   cargo run -p websocket_client -- ws://localhost:8080/
//!
//!   # 自己署名証明書を許可
//!   cargo run -p websocket_client -- wss://localhost:8443/ --insecure

use std::io;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rustls::ClientConfig;
use rustls::client::danger::ServerCertVerified;
use rustls::client::danger::ServerCertVerifier;
use rustls::pki_types::ServerName;
use rustls_platform_verifier::ConfigVerifierExt;
use shiguredo_http11::uri::Uri;
use shiguredo_websocket::{
    ClientConnectionOptions, CloseCode, ConnectionEvent, ConnectionOutput, ConnectionState,
    PerMessageDeflateConfig, RandomSource, Timestamp, WebSocketClientConnection,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{self, Duration};
use tokio_rustls::TlsConnector;

struct ClientOptions {
    host: String,
    port: u16,
    path: String,
    insecure: bool,
    tls: bool,
}

const DEFAULT_TLS_PORT: u16 = 443;
const DEFAULT_PLAIN_PORT: u16 = 80;

/// 自己署名証明書を許可するためのダミー検証器
#[derive(Debug)]
struct InsecureVerifier;

impl ServerCertVerifier for InsecureVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer,
        _intermediates: &[rustls::pki_types::CertificateDer],
        _server_name: &ServerName,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

/// 現在のタイムスタンプを取得
fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    Timestamp::from_millis(millis)
}

/// 暗号学的に安全な乱数ソース
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

fn parse_args() -> Result<ClientOptions, Box<dyn std::error::Error>> {
    let mut args = noargs::raw_args();
    args.metadata_mut().app_name = env!("CARGO_PKG_NAME");
    args.metadata_mut().app_description = "WebSocket クライアント例";

    // --version フラグ
    if noargs::VERSION_FLAG.take(&mut args).is_present() {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    // --help フラグ
    noargs::HELP_FLAG.take_help(&mut args);

    // 位置引数: URL
    let url: String = noargs::arg("<URL>")
        .doc("接続先 URL (例: wss://localhost:8443/ または ws://localhost:8080/)")
        .take(&mut args)
        .then(|a| Ok::<_, &str>(a.value().to_string()))
        .map_err(|e| format!("{:?}", e))?;

    // --insecure フラグ
    let insecure: bool = noargs::flag("insecure")
        .doc("自己署名証明書を許可する (TLS 有効時のみ)")
        .take(&mut args)
        .is_present();

    // 未知の引数があればエラー
    if let Some(help) = args.finish().map_err(|e| format!("{:?}", e))? {
        print!("{}", help);
        std::process::exit(0);
    }

    let (host, port, path, tls) = parse_url(&url)?;
    if insecure && !tls {
        return Err("`--insecure` は ws:// では使えません".into());
    }

    Ok(ClientOptions {
        host,
        port,
        path,
        insecure,
        tls,
    })
}

fn default_port(tls: bool) -> u16 {
    if tls {
        DEFAULT_TLS_PORT
    } else {
        DEFAULT_PLAIN_PORT
    }
}

fn parse_url(url: &str) -> Result<(String, u16, String, bool), Box<dyn std::error::Error>> {
    let uri = Uri::parse(url).map_err(|e| format!("URL の解析に失敗しました : {}", e))?;
    let scheme = uri.scheme().ok_or("URL にスキームがありません")?;
    let tls = if scheme.eq_ignore_ascii_case("wss") {
        true
    } else if scheme.eq_ignore_ascii_case("ws") {
        false
    } else {
        return Err("URL のスキームは ws:// または wss:// のみ対応しています".into());
    };
    if let Some(authority) = uri.authority()
        && authority.contains('@')
    {
        return Err("URL の userinfo は未対応です".into());
    }
    if uri.fragment().is_some() {
        return Err("URL のフラグメントは指定できません".into());
    }
    let host = uri.host().ok_or("URL にホストがありません")?;
    let host = normalize_host(host)?;
    let port = uri.port().unwrap_or_else(|| default_port(tls));
    let path = uri.origin_form();
    Ok((host, port, path, tls))
}

fn normalize_host(host: &str) -> Result<String, Box<dyn std::error::Error>> {
    let host = host.trim();
    if host.is_empty() {
        return Err("ホストが空です".into());
    }
    if let Some(stripped) = host.strip_prefix('[') {
        let stripped = stripped.strip_suffix(']').ok_or("ホストの指定が不正です")?;
        if stripped.is_empty() {
            return Err("ホストが空です".into());
        }
        return Ok(stripped.to_string());
    }
    Ok(host.to_string())
}

fn format_bracketed_host(host: &str) -> String {
    if host.contains(':') {
        format!("[{}]", host)
    } else {
        host.to_string()
    }
}

fn format_host_header(host: &str, port: u16, tls: bool) -> String {
    let host = format_bracketed_host(host);
    if port == default_port(tls) {
        host
    } else {
        format!("{}:{}", host, port)
    }
}

enum ClientStream {
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
    Plain(TcpStream),
}

impl ClientStream {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            ClientStream::Tls(stream) => stream.read(buf).await,
            ClientStream::Plain(stream) => stream.read(buf).await,
        }
    }

    async fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match self {
            ClientStream::Tls(stream) => stream.write_all(buf).await,
            ClientStream::Plain(stream) => stream.write_all(buf).await,
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    // コマンドライン引数を解析
    let options = parse_args().map_err(|e| io::Error::other(e.to_string()))?;

    let host = options.host.clone();
    let port = options.port;
    let path = options.path.as_str();

    let display_host = format_bracketed_host(&host);
    let scheme = if options.tls { "wss" } else { "ws" };
    println!(
        "Connecting to {}://{}:{}{}",
        scheme, display_host, port, path
    );

    // TCP 接続
    let tcp_stream = TcpStream::connect((host.as_str(), port)).await?;
    println!("TCP connected");

    let mut stream = if options.tls {
        // TLS 設定
        let tls_config = if options.insecure {
            // 自己署名証明書を許可
            rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(InsecureVerifier))
                .with_no_client_auth()
        } else {
            // 標準的な CA 検証
            ClientConfig::with_platform_verifier().map_err(|e| io::Error::other(e.to_string()))?
        };

        let connector = TlsConnector::from(Arc::new(tls_config));
        let server_name = ServerName::try_from(host.clone()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("ホストの指定が不正です : {}", host),
            )
        })?;

        // TLS ハンドシェイク
        let tls_stream = connector.connect(server_name, tcp_stream).await?;
        println!("TLS handshake completed");
        ClientStream::Tls(Box::new(tls_stream))
    } else {
        ClientStream::Plain(tcp_stream)
    };

    // WebSocket 接続オプション
    let host_header = format_host_header(&host, port, options.tls);
    let options = ClientConnectionOptions::new(&host_header, path)
        .ping_interval(10_000) // 10秒ごとに Ping
        .deflate(PerMessageDeflateConfig::new());

    // WebSocket 接続を作成
    let mut ws = WebSocketClientConnection::new(options, SecureRandom);

    // ハンドシェイク開始
    ws.connect().unwrap();

    // 出力を送信
    while let Some(output) = ws.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            stream.write_all(&data).await?;
        }
    }

    // ハンドシェイクレスポンスを受信
    let mut buf = vec![0u8; 4096];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            println!("Connection closed during handshake");
            return Ok(());
        }

        ws.feed_recv_buf(&buf[..n], now()).unwrap();

        // イベントを処理
        while let Some(event) = ws.poll_event() {
            match event {
                ConnectionEvent::Connected {
                    protocol,
                    extensions,
                } => {
                    println!("WebSocket connected!");
                    println!("  Protocol: {:?}", protocol);
                    println!("  Extensions: {:?}", extensions);
                }
                ConnectionEvent::StateChanged(state) => {
                    println!("State changed: {:?}", state);
                }
                _ => {}
            }
        }

        if ws.state() == ConnectionState::Connected {
            break;
        }
    }

    // 出力を送信
    while let Some(output) = ws.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            stream.write_all(&data).await?;
        }
    }

    // テストメッセージを送信
    let test_message = "Hello, WebSocket!";
    println!("\nSending: {}", test_message);
    ws.send_text(test_message).unwrap();

    while let Some(output) = ws.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            stream.write_all(&data).await?;
        }
    }

    // エコーを受信（タイムアウト付き）
    let timeout = time::timeout(Duration::from_secs(10), async {
        let mut echo_received = false;
        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                break;
            }

            ws.feed_recv_buf(&buf[..n], now()).unwrap();

            while let Some(event) = ws.poll_event() {
                match event {
                    ConnectionEvent::TextMessage(text) => {
                        println!("Received: {}", text);
                        if text == test_message {
                            println!("Echo confirmed!");
                            echo_received = true;
                        }
                    }
                    ConnectionEvent::BinaryMessage(data) => {
                        println!("Received binary: {} bytes", data.len());
                    }
                    ConnectionEvent::Ping(data) => {
                        println!("Received Ping: {:?}", data);
                    }
                    ConnectionEvent::Pong(data) => {
                        println!("Received Pong: {:?}", data);
                    }
                    ConnectionEvent::Close { code, reason } => {
                        println!("Received Close: {:?} - {}", code, reason);
                        return Ok::<_, io::Error>(());
                    }
                    ConnectionEvent::Error(err) => {
                        println!("Error: {}", err);
                    }
                    _ => {}
                }
            }

            // 出力を送信（Pong 応答など）
            while let Some(output) = ws.poll_output() {
                if let ConnectionOutput::SendData(data) = output {
                    stream.write_all(&data).await?;
                }
            }

            if echo_received {
                return Ok(());
            }
        }

        Ok(())
    })
    .await;

    match timeout {
        Ok(Ok(())) => {}
        Ok(Err(e)) => println!("IO error: {}", e),
        Err(_) => println!("Timeout waiting for echo"),
    }

    // クローズ
    println!("\nClosing connection...");
    ws.close(CloseCode::NORMAL, "goodbye").unwrap();

    while let Some(output) = ws.poll_output() {
        match output {
            ConnectionOutput::SendData(data) => {
                stream.write_all(&data).await?;
            }
            ConnectionOutput::CloseConnection => {
                println!("Connection closed");
                break;
            }
            _ => {}
        }
    }

    // クローズ応答を待つ
    let _ = time::timeout(Duration::from_secs(2), async {
        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                break;
            }

            ws.feed_recv_buf(&buf[..n], now()).unwrap();

            while let Some(event) = ws.poll_event() {
                if let ConnectionEvent::Close { code, reason } = event {
                    println!("Server closed: {:?} - {}", code, reason);
                    return Ok::<_, io::Error>(());
                }
            }

            if ws.state() == ConnectionState::Closed {
                break;
            }
        }
        Ok(())
    })
    .await;

    println!("Done!");
    Ok(())
}
