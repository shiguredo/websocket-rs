//! HTTP CONNECT プロキシクライアント

use std::sync::Arc;

use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use rustls_platform_verifier::ConfigVerifierExt;
use shiguredo_http11::ResponseDecoder;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::common::{AnyError, ProxyUrl, format_bracketed_host};
use crate::stream::{DynStream, IoStream};

/// プロキシ越しに `target_host:target_port` への TCP トンネルを確立し、
/// 確立したストリームを返す
pub async fn connect_through_proxy(
    proxy: &ProxyUrl,
    target_host: &str,
    target_port: u16,
) -> Result<DynStream, AnyError> {
    let tcp = TcpStream::connect((proxy.host.as_str(), proxy.port))
        .await
        .map_err(|e| {
            format!(
                "failed to connect to proxy {}:{}: {}",
                proxy.host, proxy.port, e
            )
        })?;
    let _ = tcp.set_nodelay(true);

    if proxy.tls {
        let config = ClientConfig::with_platform_verifier()
            .map_err(|e| format!("failed to load platform verifier for proxy: {}", e))?;
        let connector = TlsConnector::from(Arc::new(config));
        let server_name = ServerName::try_from(proxy.host.clone())
            .map_err(|_| format!("invalid proxy host name: {}", proxy.host))?;
        let mut tls = connector
            .connect(server_name, tcp)
            .await
            .map_err(|e| format!("TLS handshake with proxy failed: {}", e))?;
        send_connect_and_validate(&mut tls, target_host, target_port, proxy.auth.as_deref())
            .await?;
        Ok(Box::new(tls) as DynStream)
    } else {
        let mut tcp = tcp;
        send_connect_and_validate(&mut tcp, target_host, target_port, proxy.auth.as_deref())
            .await?;
        Ok(Box::new(tcp) as DynStream)
    }
}

async fn send_connect_and_validate<S: IoStream + ?Sized>(
    stream: &mut S,
    target_host: &str,
    target_port: u16,
    proxy_auth: Option<&str>,
) -> Result<(), AnyError> {
    let host_header = format!("{}:{}", format_bracketed_host(target_host), target_port);

    let mut request = format!(
        "CONNECT {host} HTTP/1.1\r\nHost: {host}\r\nProxy-Connection: Keep-Alive\r\n",
        host = host_header
    );
    if let Some(auth) = proxy_auth {
        request.push_str(&format!("Proxy-Authorization: {}\r\n", auth));
    }
    request.push_str("\r\n");

    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| format!("failed to send CONNECT to proxy: {}", e))?;
    stream
        .flush()
        .await
        .map_err(|e| format!("failed to flush CONNECT to proxy: {}", e))?;

    let mut decoder = ResponseDecoder::new();
    let mut buf = vec![0u8; 4096];
    let head = loop {
        let n = stream
            .read(&mut buf)
            .await
            .map_err(|e| format!("failed to read CONNECT response: {}", e))?;
        if n == 0 {
            return Err("proxy closed the connection during CONNECT".into());
        }
        decoder
            .feed(&buf[..n])
            .map_err(|e| format!("invalid CONNECT response: {}", e))?;
        if let Some((head, _body)) = decoder
            .decode_headers()
            .map_err(|e| format!("invalid CONNECT response: {}", e))?
        {
            break head;
        }
    };

    if !head.is_success() {
        return Err(format!(
            "proxy CONNECT failed: {} {}",
            head.status_code, head.reason_phrase
        )
        .into());
    }
    Ok(())
}
