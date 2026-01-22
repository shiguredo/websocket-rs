//! WebSocket リバースプロキシの例
//!
//! 使い方:
//!   # WS -> WS
//!   cargo run -p websocket_reverse_proxy -- --port 8888 --upstream ws://localhost:8080/
//!
//!   # WS -> WSS
//!   cargo run -p websocket_reverse_proxy -- --port 8888 --upstream wss://echo.websocket.org/
//!
//!   # WSS -> WSS (TLS 終端)
//!   cargo run -p websocket_reverse_proxy -- --port 8443 --tls --cert cert.pem --key key.pem --upstream wss://echo.websocket.org/

use std::io;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, ServerConfig};
use rustls_platform_verifier::ConfigVerifierExt;
use shiguredo_http11::uri::Uri;
use shiguredo_websocket::{
    ClientConnectionOptions, CloseCode, ConnectionEvent, ConnectionOutput, ConnectionState,
    PerMessageDeflateConfig, ServerConnectionOptions, Timestamp, WebSocketClientConnection,
    WebSocketServerConnection,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

const DEFAULT_TLS_PORT: u16 = 443;
const DEFAULT_PLAIN_PORT: u16 = 80;

struct ProxyOptions {
    port: u16,
    upstream_url: String,
    upstream_host: String,
    upstream_port: u16,
    upstream_path: String,
    upstream_tls: bool,
    tls: bool,
    cert_path: Option<String>,
    key_path: Option<String>,
    insecure: bool,
    debug: bool,
}

/// 現在のタイムスタンプを取得
fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    Timestamp::from_millis(millis)
}

/// ハンドシェイク用の nonce を生成
fn generate_nonce() -> [u8; 16] {
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).expect("failed to generate nonce");
    nonce
}

/// フレームマスキング用のキーを生成
fn generate_masking_key() -> [u8; 4] {
    let mut key = [0u8; 4];
    getrandom::fill(&mut key).expect("failed to generate masking key");
    key
}

fn now_timestamp() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    format!("{}.{:03}", secs, millis)
}

fn log_debug(enabled: bool, message: &str) {
    if enabled {
        eprintln!("[{}] DEBUG: {}", now_timestamp(), message);
    }
}

/// 自己署名証明書を許可するためのダミー検証器
#[derive(Debug)]
struct InsecureVerifier;

impl rustls::client::danger::ServerCertVerifier for InsecureVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer,
        _intermediates: &[CertificateDer],
        _server_name: &ServerName,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer,
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_args()?;

    let addr = format!("0.0.0.0:{}", options.port);
    let listener = TcpListener::bind(&addr).await?;

    let scheme = if options.tls { "wss" } else { "ws" };
    println!("WebSocket リバースプロキシをバインド: {}", addr);
    println!(
        "  {}://localhost:{}/ -> {}",
        scheme, options.port, options.upstream_url
    );

    let options = Arc::new(options);

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

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            let acceptor = acceptor.clone();
            let options = Arc::clone(&options);

            tokio::spawn(async move {
                match acceptor.accept(stream).await {
                    Ok(tls_stream) => {
                        if let Err(e) = handle_client(tls_stream, peer_addr, &options).await {
                            eprintln!("クライアント処理エラー ({}): {}", peer_addr, e);
                        }
                    }
                    Err(e) => eprintln!("TLS ハンドシェイクエラー ({}): {}", peer_addr, e),
                }
            });
        }
    } else {
        loop {
            let (stream, peer_addr) = listener.accept().await?;
            let options = Arc::clone(&options);

            tokio::spawn(async move {
                if let Err(e) = handle_client(stream, peer_addr, &options).await {
                    eprintln!("クライアント処理エラー ({}): {}", peer_addr, e);
                }
            });
        }
    }
}

fn parse_args() -> Result<ProxyOptions, Box<dyn std::error::Error>> {
    let mut args = noargs::raw_args();
    args.metadata_mut().app_name = "websocket_reverse_proxy";
    args.metadata_mut().app_description = "WebSocket リバースプロキシ";

    // --version フラグ
    if noargs::VERSION_FLAG.take(&mut args).is_present() {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    // --help フラグ
    noargs::HELP_FLAG.take_help(&mut args);

    // --debug フラグ
    let debug: bool = noargs::flag("debug")
        .doc("デバッグログを有効化")
        .take(&mut args)
        .is_present();

    // --tls フラグ
    let tls: bool = noargs::flag("tls")
        .doc("フロントエンドで WSS を有効化")
        .take(&mut args)
        .is_present();

    // --port オプション
    let default_port = if tls { "8443" } else { "8080" };
    let port: u16 = noargs::opt("port")
        .short('p')
        .doc("リッスンポート (デフォルト: 8080, TLS 有効時: 8443)")
        .default(default_port)
        .take(&mut args)
        .then(|o| o.value().parse())
        .map_err(|e| format!("{:?}", e))?;

    // --upstream オプション
    let upstream_url: Option<String> = noargs::opt("upstream")
        .short('u')
        .doc("転送先 URL (例: ws://localhost:8080/ または wss://echo.websocket.org/)")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(o.value().to_string()))
        .map_err(|e| format!("{:?}", e))?;

    // --cert オプション
    let cert_path: Option<String> = noargs::opt("cert")
        .doc("証明書ファイル (PEM)")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(o.value().to_string()))
        .map_err(|e| format!("{:?}", e))?;

    // --key オプション
    let key_path: Option<String> = noargs::opt("key")
        .doc("秘密鍵ファイル (PEM)")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(o.value().to_string()))
        .map_err(|e| format!("{:?}", e))?;

    // --insecure フラグ
    let insecure: bool = noargs::flag("insecure")
        .doc("アップストリームの自己署名証明書を許可")
        .take(&mut args)
        .is_present();

    // 未知の引数があればエラー
    if let Some(help) = args.finish().map_err(|e| format!("{:?}", e))? {
        print!("{}", help);
        std::process::exit(0);
    }

    // 必須オプションのチェック
    let upstream_url = upstream_url.ok_or("--upstream オプションは必須です")?;

    let (upstream_host, upstream_port, upstream_path, upstream_tls) = parse_url(&upstream_url)?;

    Ok(ProxyOptions {
        port,
        upstream_url,
        upstream_host,
        upstream_port,
        upstream_path,
        upstream_tls,
        tls,
        cert_path,
        key_path,
        insecure,
        debug,
    })
}

fn parse_url(url: &str) -> Result<(String, u16, String, bool), Box<dyn std::error::Error>> {
    let uri = Uri::parse(url).map_err(|e| format!("URL の解析に失敗しました: {}", e))?;
    let scheme = uri.scheme().ok_or("URL にスキームがありません")?;
    let tls = if scheme.eq_ignore_ascii_case("wss") {
        true
    } else if scheme.eq_ignore_ascii_case("ws") {
        false
    } else {
        return Err("URL のスキームは ws:// または wss:// のみ対応しています".into());
    };
    let host = uri.host().ok_or("URL にホストがありません")?;
    let host = normalize_host(host)?;
    let port = uri.port().unwrap_or_else(|| default_port(tls));
    let path = uri.origin_form();
    Ok((host, port, path, tls))
}

fn default_port(tls: bool) -> u16 {
    if tls {
        DEFAULT_TLS_PORT
    } else {
        DEFAULT_PLAIN_PORT
    }
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

fn load_tls_config(
    cert_path: &str,
    key_path: &str,
) -> Result<ServerConfig, Box<dyn std::error::Error>> {
    let certs: Vec<CertificateDer<'static>> =
        CertificateDer::pem_file_iter(cert_path)?.collect::<Result<Vec<_>, _>>()?;

    if certs.is_empty() {
        return Err("証明書ファイルに証明書がありません".into());
    }

    let key = PrivateKeyDer::from_pem_file(key_path)?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    Ok(config)
}

enum ProxyStream {
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
    Plain(TcpStream),
}

impl ProxyStream {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            ProxyStream::Tls(stream) => stream.read(buf).await,
            ProxyStream::Plain(stream) => stream.read(buf).await,
        }
    }

    async fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match self {
            ProxyStream::Tls(stream) => stream.write_all(buf).await,
            ProxyStream::Plain(stream) => stream.write_all(buf).await,
        }
    }
}

async fn handle_client<S>(
    mut client_stream: S,
    peer_addr: std::net::SocketAddr,
    options: &ProxyOptions,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let debug = options.debug;
    log_debug(debug, &format!("クライアント接続: {}", peer_addr));

    // クライアント側 WebSocket (サーバーとして動作)
    let server_options = ServerConnectionOptions::new();
    let mut client_ws = WebSocketServerConnection::new(server_options);

    // クライアントからのハンドシェイクを受信
    let mut buf = vec![0u8; 8192];
    loop {
        let n = client_stream.read(&mut buf).await?;
        if n == 0 {
            log_debug(debug, "クライアント切断 (ハンドシェイク前)");
            return Ok(());
        }

        client_ws.feed_recv_buf(&buf[..n], now())?;

        if client_ws.state() == ConnectionState::Connecting {
            // ハンドシェイクを受諾
            client_ws.accept_handshake_auto(now())?;
            break;
        }
    }

    // クライアントへハンドシェイクレスポンスを送信
    while let Some(output) = client_ws.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            client_stream.write_all(&data).await?;
        }
    }

    // イベントを処理して Connected を確認
    while let Some(event) = client_ws.poll_event() {
        if let ConnectionEvent::Connected {
            protocol,
            extensions,
        } = event
        {
            log_debug(
                debug,
                &format!(
                    "クライアント WebSocket 接続完了: protocol={:?}, extensions={:?}",
                    protocol, extensions
                ),
            );
        }
    }

    log_debug(debug, "アップストリームへ接続中...");

    // アップストリームへ接続
    let mut upstream_stream = connect_upstream(options).await?;

    // アップストリーム側 WebSocket (クライアントとして動作)
    let host_header = format_host_header(
        &options.upstream_host,
        options.upstream_port,
        options.upstream_tls,
    );
    let client_options = ClientConnectionOptions::new(&host_header, &options.upstream_path)
        .deflate(PerMessageDeflateConfig::new());
    let mut upstream_ws = WebSocketClientConnection::new(client_options);

    // アップストリームへハンドシェイクを送信
    upstream_ws.connect(generate_nonce())?;

    while let Some(output) = upstream_ws.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            upstream_stream.write_all(&data).await?;
        }
    }

    // アップストリームからのハンドシェイクレスポンスを受信
    loop {
        let n = upstream_stream.read(&mut buf).await?;
        if n == 0 {
            return Err("アップストリームが切断されました (ハンドシェイク中)".into());
        }

        upstream_ws.feed_recv_buf(&buf[..n], now())?;

        while let Some(event) = upstream_ws.poll_event() {
            if let ConnectionEvent::Connected {
                protocol,
                extensions,
            } = event
            {
                log_debug(
                    debug,
                    &format!(
                        "アップストリーム WebSocket 接続完了: protocol={:?}, extensions={:?}",
                        protocol, extensions
                    ),
                );
            }
        }

        if upstream_ws.state() == ConnectionState::Connected {
            break;
        }
    }

    log_debug(debug, "双方向プロキシを開始");

    // 双方向プロキシループ
    proxy_loop(
        &mut client_stream,
        &mut client_ws,
        &mut upstream_stream,
        &mut upstream_ws,
        debug,
    )
    .await?;

    log_debug(debug, &format!("接続終了: {}", peer_addr));
    Ok(())
}

async fn connect_upstream(
    options: &ProxyOptions,
) -> Result<ProxyStream, Box<dyn std::error::Error + Send + Sync>> {
    let tcp_stream =
        TcpStream::connect((options.upstream_host.as_str(), options.upstream_port)).await?;

    if options.upstream_tls {
        let tls_config = if options.insecure {
            ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(InsecureVerifier))
                .with_no_client_auth()
        } else {
            ClientConfig::with_platform_verifier().map_err(|e| format!("TLS 設定エラー: {}", e))?
        };

        let connector = TlsConnector::from(Arc::new(tls_config));
        let server_name = ServerName::try_from(options.upstream_host.clone())
            .map_err(|_| format!("無効なホスト名: {}", options.upstream_host))?;

        let tls_stream = connector.connect(server_name, tcp_stream).await?;
        Ok(ProxyStream::Tls(Box::new(tls_stream)))
    } else {
        Ok(ProxyStream::Plain(tcp_stream))
    }
}

async fn proxy_loop<S>(
    client_stream: &mut S,
    client_ws: &mut WebSocketServerConnection,
    upstream_stream: &mut ProxyStream,
    upstream_ws: &mut WebSocketClientConnection,
    debug: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let mut client_buf = vec![0u8; 8192];
    let mut upstream_buf = vec![0u8; 8192];

    loop {
        tokio::select! {
            // クライアントからの受信
            result = client_stream.read(&mut client_buf) => {
                let n = result?;
                if n == 0 {
                    log_debug(debug, "クライアント切断");
                    // クライアントが切断したらアップストリームも閉じる
                    if upstream_ws.state() == ConnectionState::Connected {
                        upstream_ws.close(CloseCode::NORMAL, "", generate_masking_key())?;
                        send_output(upstream_stream, upstream_ws).await?;
                    }
                    break;
                }

                client_ws.feed_recv_buf(&client_buf[..n], now())?;

                // クライアントからのイベントを処理してアップストリームへ転送
                while let Some(event) = client_ws.poll_event() {
                    match event {
                        ConnectionEvent::TextMessage(text) => {
                            log_debug(debug, &format!("クライアント -> アップストリーム (text): {} bytes", text.len()));
                            upstream_ws.send_text(&text, generate_masking_key())?;
                        }
                        ConnectionEvent::BinaryMessage(data) => {
                            log_debug(debug, &format!("クライアント -> アップストリーム (binary): {} bytes", data.len()));
                            upstream_ws.send_binary(&data, generate_masking_key())?;
                        }
                        ConnectionEvent::Ping(data) => {
                            log_debug(debug, "クライアント -> アップストリーム (ping)");
                            upstream_ws.send_ping(&data, now(), generate_masking_key())?;
                        }
                        ConnectionEvent::Close { code, reason } => {
                            log_debug(debug, &format!("クライアントから Close: {:?} {}", code, reason));
                            upstream_ws.close(code.unwrap_or(CloseCode::NORMAL), &reason, generate_masking_key())?;
                        }
                        ConnectionEvent::Error(err) => {
                            log_debug(debug, &format!("クライアントエラー: {}", err));
                        }
                        _ => {}
                    }
                }

                // クライアントへの出力を送信
                send_output_server(client_stream, client_ws).await?;
                // アップストリームへの出力を送信
                send_output(upstream_stream, upstream_ws).await?;

                if client_ws.state() == ConnectionState::Closed {
                    log_debug(debug, "クライアント WebSocket 終了");
                    break;
                }
            }

            // アップストリームからの受信
            result = upstream_stream.read(&mut upstream_buf) => {
                let n = result?;
                if n == 0 {
                    log_debug(debug, "アップストリーム切断");
                    // アップストリームが切断したらクライアントも閉じる
                    if client_ws.state() == ConnectionState::Connected {
                        client_ws.close(CloseCode::NORMAL, "", now())?;
                        send_output_server(client_stream, client_ws).await?;
                    }
                    break;
                }

                // 自動応答（Pong など）用の masking_key を事前に追加
                upstream_ws.push_masking_key(generate_masking_key());
                upstream_ws.feed_recv_buf(&upstream_buf[..n], now())?;

                // アップストリームからのイベントを処理してクライアントへ転送
                while let Some(event) = upstream_ws.poll_event() {
                    match event {
                        ConnectionEvent::TextMessage(text) => {
                            log_debug(debug, &format!("アップストリーム -> クライアント (text): {} bytes", text.len()));
                            client_ws.send_text(&text, now())?;
                        }
                        ConnectionEvent::BinaryMessage(data) => {
                            log_debug(debug, &format!("アップストリーム -> クライアント (binary): {} bytes", data.len()));
                            client_ws.send_binary(&data, now())?;
                        }
                        ConnectionEvent::Ping(data) => {
                            log_debug(debug, "アップストリーム -> クライアント (ping)");
                            client_ws.send_ping(&data, now())?;
                        }
                        ConnectionEvent::Close { code, reason } => {
                            log_debug(debug, &format!("アップストリームから Close: {:?} {}", code, reason));
                            client_ws.close(code.unwrap_or(CloseCode::NORMAL), &reason, now())?;
                        }
                        ConnectionEvent::Error(err) => {
                            log_debug(debug, &format!("アップストリームエラー: {}", err));
                        }
                        _ => {}
                    }
                }

                // アップストリームへの出力を送信
                send_output(upstream_stream, upstream_ws).await?;
                // クライアントへの出力を送信
                send_output_server(client_stream, client_ws).await?;

                if upstream_ws.state() == ConnectionState::Closed {
                    log_debug(debug, "アップストリーム WebSocket 終了");
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn send_output(
    stream: &mut ProxyStream,
    ws: &mut WebSocketClientConnection,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    while let Some(output) = ws.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            stream.write_all(&data).await?;
        }
    }
    Ok(())
}

async fn send_output_server<S>(
    stream: &mut S,
    ws: &mut WebSocketServerConnection,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncWriteExt + Unpin,
{
    while let Some(output) = ws.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            stream.write_all(&data).await?;
        }
    }
    Ok(())
}
