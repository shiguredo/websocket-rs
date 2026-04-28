//! `--connect` モードの本体

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::ServerName;
use shiguredo_websocket::{
    ClientConnectionOptions, CloseCode, ConnectionEvent, ConnectionOutput, ConnectionState,
    TimerId, WebSocketClientConnection,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::{Instant, Sleep, sleep_until};
use tokio_rustls::TlsConnector;

use crate::cli::Cli;
use crate::common::{
    AnyError, SecureRandom, basic_auth_value, format_host_header, now, parse_proxy_url,
    parse_ws_url,
};
use crate::common::{ProxyUrl, WsUrl};
use crate::console::Console;
use crate::proxy::connect_through_proxy;
use crate::redirect::next_url;
use crate::slash::{self, SlashCmd};
use crate::stream::DynStream;
use crate::tls::build_client_config;

/// `--connect` モードを実行する。`Cli::mode` が `Connect` であることが前提。
pub async fn run(cli: &Cli, initial_url: &str) -> Result<(), AnyError> {
    let mut console = Console::new(cli.color, cli.execute.is_empty());

    let mut current_url = parse_ws_url(initial_url)?;
    let mut redirects_remaining = if cli.follow_redirects {
        cli.max_redirects
    } else {
        0
    };

    let proxy_url = match cli.proxy.as_deref() {
        Some(url) => Some(parse_proxy_url(url)?),
        None => None,
    };

    loop {
        match connect_once(cli, &mut console, &current_url, proxy_url.as_ref()).await {
            Ok(ConnectOutcome::Finished) => return Ok(()),
            Ok(ConnectOutcome::Redirect(info)) => {
                if redirects_remaining == 0 {
                    return Err(format!(
                        "redirect {} {} received but --location/-L is not set or limit reached",
                        info.status_code, info.reason_phrase
                    )
                    .into());
                }
                let next = next_url(&current_url, &info)?
                    .ok_or("redirect requested but resolver returned no URL")?;
                redirects_remaining -= 1;
                console
                    .print_status(&format!(
                        "Redirecting to ws{}://{}",
                        if next.tls { "s" } else { "" },
                        format_host_header(&next.host, next.port, next.tls)
                    ))
                    .await
                    .ok();
                current_url = next;
            }
            Err(e) => return Err(e),
        }
    }
}

enum ConnectOutcome {
    Finished,
    Redirect(shiguredo_websocket::HttpResponseInfo),
}

async fn connect_once(
    cli: &Cli,
    console: &mut Console,
    url: &WsUrl,
    proxy: Option<&ProxyUrl>,
) -> Result<ConnectOutcome, AnyError> {
    // ベースとなるストリームを準備 (proxy 経由 or 直接 TCP)
    let base_stream: DynStream = match proxy {
        Some(p) => connect_through_proxy(p, &url.host, url.port).await?,
        None => {
            let tcp = TcpStream::connect((url.host.as_str(), url.port))
                .await
                .map_err(|e| format!("TCP connect failed: {}", e))?;
            let _ = tcp.set_nodelay(true);
            Box::new(tcp) as DynStream
        }
    };

    // wss なら TLS で包む
    let mut stream: DynStream = if url.tls {
        let config = build_client_config(cli)?;
        let connector = TlsConnector::from(Arc::new(config));
        let server_name = ServerName::try_from(url.host.clone())
            .map_err(|_| format!("invalid host name: {}", url.host))?;
        let tls = connector
            .connect(server_name, base_stream)
            .await
            .map_err(|e| format!("TLS handshake failed: {}", e))?;
        Box::new(tls) as DynStream
    } else {
        base_stream
    };

    // WebSocket ハンドシェイク用オプションを構築
    let host_header = cli
        .host_header
        .clone()
        .unwrap_or_else(|| format_host_header(&url.host, url.port, url.tls));
    let mut options = ClientConnectionOptions::new(&host_header, &url.path);
    if let Some(origin) = cli.origin.as_deref() {
        options = options.origin(origin);
    }
    for sub in &cli.subprotocols {
        options = options.protocol(sub);
    }
    for (name, value) in &cli.headers {
        options = options.header(name, value);
    }
    if let Some(auth) = cli.auth.as_deref() {
        options = options.header("Authorization", &basic_auth_value(auth));
    }

    let mut ws = WebSocketClientConnection::new(options, SecureRandom);
    ws.connect()
        .map_err(|e| format!("failed to start WebSocket handshake: {}", e))?;

    // 出力 (ハンドシェイクリクエスト) を送信
    drain_outputs(&mut ws, &mut stream, &mut TimerState::default()).await?;

    // ハンドシェイクレスポンス受信ループ
    let mut buf = vec![0u8; 8192];
    let mut timers = TimerState::default();
    while ws.state() == ConnectionState::Connecting {
        let n = stream
            .read(&mut buf)
            .await
            .map_err(|e| format!("failed to read handshake response: {}", e))?;
        if n == 0 {
            return Err("connection closed during handshake".into());
        }
        match ws.feed_recv_buf(&buf[..n], now()) {
            Ok(()) => {}
            Err(e) => {
                if let Some(info) = e.http_response.as_deref() {
                    if (300..400).contains(&info.status_code) {
                        return Ok(ConnectOutcome::Redirect(
                            (**e.http_response.as_ref().unwrap()).clone(),
                        ));
                    }
                    return Err(format!(
                        "WebSocket handshake rejected: HTTP {} {}",
                        info.status_code, info.reason_phrase
                    )
                    .into());
                }
                return Err(format!("handshake error: {}", e).into());
            }
        }
        drain_events_to_console(&mut ws, console, cli.show_ping_pong).await?;
        drain_outputs(&mut ws, &mut stream, &mut timers).await?;
    }

    if ws.state() != ConnectionState::Connected {
        return Err(format!(
            "WebSocket is not in Connected state after handshake (state = {:?})",
            ws.state()
        )
        .into());
    }

    let scheme = if url.tls { "wss" } else { "ws" };
    console
        .print_status(&format!(
            "Connected ({}://{}{})",
            scheme,
            format_host_header(&url.host, url.port, url.tls),
            url.path
        ))
        .await
        .ok();

    // 対話モード or execute モードに分岐
    if cli.execute.is_empty() {
        run_interactive(cli, console, &mut ws, &mut stream, &mut timers).await?;
    } else {
        run_execute(cli, &mut ws, &mut stream, &mut timers).await?;
    }
    Ok(ConnectOutcome::Finished)
}

async fn run_execute(
    cli: &Cli,
    ws: &mut WebSocketClientConnection<SecureRandom>,
    stream: &mut DynStream,
    timers: &mut TimerState,
) -> Result<(), AnyError> {
    for msg in &cli.execute {
        ws.send_text(msg)
            .map_err(|e| format!("failed to send: {}", e))?;
        drain_outputs(ws, stream, timers).await?;
    }

    let close_deadline = cli.wait.duration().map(|d| Instant::now() + d);
    let mut buf = vec![0u8; 8192];
    let mut closed = false;

    let mut timer_box: Option<std::pin::Pin<Box<Sleep>>> = None;

    loop {
        let timer_fut = make_timer_fut(timers, &mut timer_box);
        let close_fut = async {
            match close_deadline {
                Some(d) => sleep_until(d).await,
                None => std::future::pending().await,
            }
        };
        tokio::pin!(close_fut);

        tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => {
                if !closed {
                    let _ = ws.close(CloseCode::NORMAL, "");
                    drain_outputs(ws, stream, timers).await?;
                }
                break;
            }
            res = stream.read(&mut buf) => {
                let n = res.map_err(|e| format!("read error: {}", e))?;
                if n == 0 { break; }
                ws.feed_recv_buf(&buf[..n], now()).map_err(|e| format!("recv error: {}", e))?;
                drain_events_silent(ws).await?;
                drain_outputs(ws, stream, timers).await?;
                if matches!(ws.state(), ConnectionState::Closed) {
                    break;
                }
            }
            id = timer_fut => {
                ws.handle_timer(id).map_err(|e| format!("timer error: {}", e))?;
                drain_outputs(ws, stream, timers).await?;
            }
            _ = close_fut, if !closed && close_deadline.is_some() => {
                let _ = ws.close(CloseCode::NORMAL, "");
                drain_outputs(ws, stream, timers).await?;
                closed = true;
            }
        }
    }
    Ok(())
}

async fn run_interactive(
    cli: &Cli,
    console: &mut Console,
    ws: &mut WebSocketClientConnection<SecureRandom>,
    stream: &mut DynStream,
    timers: &mut TimerState,
) -> Result<(), AnyError> {
    console.write_prompt_if_interactive().await.ok();

    let stdin = tokio::io::stdin();
    let mut stdin = BufReader::new(stdin).lines();

    let mut buf = vec![0u8; 8192];
    let mut closed = false;
    let mut timer_box: Option<std::pin::Pin<Box<Sleep>>> = None;

    loop {
        let timer_fut = make_timer_fut(timers, &mut timer_box);

        tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => {
                if !closed {
                    let _ = ws.close(CloseCode::NORMAL, "");
                    drain_outputs(ws, stream, timers).await?;
                }
                break;
            }
            line = stdin.next_line(), if !closed => {
                let line = line.map_err(|e| format!("stdin error: {}", e))?;
                let line = match line {
                    Some(l) => l,
                    None => {
                        // EOF: close
                        let _ = ws.close(CloseCode::NORMAL, "");
                        drain_outputs(ws, stream, timers).await?;
                        closed = true;
                        continue;
                    }
                };
                if handle_user_input(cli, console, ws, stream, timers, &line).await? {
                    closed = true;
                }
            }
            res = stream.read(&mut buf) => {
                let n = res.map_err(|e| format!("read error: {}", e))?;
                if n == 0 { break; }
                ws.feed_recv_buf(&buf[..n], now()).map_err(|e| format!("recv error: {}", e))?;
                drain_events_to_console(ws, console, cli.show_ping_pong).await?;
                drain_outputs(ws, stream, timers).await?;
                if matches!(ws.state(), ConnectionState::Closed) {
                    break;
                }
            }
            id = timer_fut => {
                ws.handle_timer(id).map_err(|e| format!("timer error: {}", e))?;
                drain_outputs(ws, stream, timers).await?;
            }
        }
    }
    Ok(())
}

/// 戻り値が `true` の場合、ユーザーが close 要求を出したことを意味する
async fn handle_user_input(
    cli: &Cli,
    console: &mut Console,
    ws: &mut WebSocketClientConnection<SecureRandom>,
    stream: &mut DynStream,
    timers: &mut TimerState,
    line: &str,
) -> Result<bool, AnyError> {
    if cli.slash
        && let Some(cmd) = slash::parse(line)
    {
        let mut close_requested = false;
        match cmd {
            SlashCmd::Ping(data) => {
                let payload = data.unwrap_or_default();
                if let Err(e) = ws.send_ping(payload.as_bytes()) {
                    console
                        .print_warning(&format!("send_ping failed: {}", e))
                        .await
                        .ok();
                }
            }
            SlashCmd::Pong(data) => {
                let payload = data.unwrap_or_default();
                if let Err(e) = ws.send_pong(payload.as_bytes()) {
                    console
                        .print_warning(&format!("send_pong failed: {}", e))
                        .await
                        .ok();
                }
            }
            SlashCmd::Close { code, reason } => {
                let close_code = code.map(CloseCode::new).unwrap_or(CloseCode::NORMAL);
                let reason = reason.unwrap_or_default();
                if let Err(e) = ws.close(close_code, &reason) {
                    console
                        .print_warning(&format!("close failed: {}", e))
                        .await
                        .ok();
                }
                close_requested = true;
            }
            SlashCmd::Unknown => {
                console
                    .print_warning("Unrecognized slash command.")
                    .await
                    .ok();
                return Ok(false);
            }
        }
        drain_outputs(ws, stream, timers).await?;
        return Ok(close_requested);
    }

    if let Err(e) = ws.send_text(line) {
        console
            .print_warning(&format!("send failed: {}", e))
            .await
            .ok();
        return Ok(false);
    }
    drain_outputs(ws, stream, timers).await?;
    console.write_prompt_if_interactive().await.ok();
    Ok(false)
}

#[derive(Default)]
struct TimerState {
    deadlines: HashMap<TimerId, Instant>,
}

impl TimerState {
    fn set(&mut self, id: TimerId, duration_millis: u64) {
        self.deadlines
            .insert(id, Instant::now() + Duration::from_millis(duration_millis));
    }
    fn clear(&mut self, id: TimerId) {
        self.deadlines.remove(&id);
    }
    fn next(&self) -> Option<(TimerId, Instant)> {
        self.deadlines
            .iter()
            .min_by_key(|(_, d)| **d)
            .map(|(id, d)| (*id, *d))
    }
}

fn make_timer_fut<'a>(
    timers: &'a TimerState,
    slot: &'a mut Option<std::pin::Pin<Box<Sleep>>>,
) -> impl std::future::Future<Output = TimerId> + 'a {
    let next = timers.next();
    *slot = next.map(|(_, deadline)| Box::pin(sleep_until(deadline)));
    async move {
        match (next, slot.as_mut()) {
            (Some((id, _)), Some(sleep)) => {
                sleep.as_mut().await;
                id
            }
            _ => std::future::pending::<TimerId>().await,
        }
    }
}

async fn drain_outputs(
    ws: &mut WebSocketClientConnection<SecureRandom>,
    stream: &mut DynStream,
    timers: &mut TimerState,
) -> Result<(), AnyError> {
    while let Some(output) = ws.poll_output() {
        match output {
            ConnectionOutput::SendData(data) => {
                stream
                    .write_all(&data)
                    .await
                    .map_err(|e| format!("write error: {}", e))?;
            }
            ConnectionOutput::SetTimer {
                id,
                duration_millis,
            } => {
                timers.set(id, duration_millis);
            }
            ConnectionOutput::ClearTimer { id } => {
                timers.clear(id);
            }
            ConnectionOutput::CloseConnection => {
                let _ = stream.shutdown().await;
            }
        }
    }
    Ok(())
}

async fn drain_events_to_console(
    ws: &mut WebSocketClientConnection<SecureRandom>,
    console: &mut Console,
    show_ping_pong: bool,
) -> Result<(), AnyError> {
    while let Some(event) = ws.poll_event() {
        match event {
            ConnectionEvent::TextMessage(text) => {
                console.print_incoming(&text).await.ok();
            }
            ConnectionEvent::BinaryMessage(data) => {
                let text = String::from_utf8_lossy(&data).into_owned();
                console.print_incoming(&text).await.ok();
            }
            ConnectionEvent::Ping(data) => {
                if show_ping_pong {
                    let body = String::from_utf8_lossy(&data);
                    console
                        .print_event(&format!("Received ping (data: \"{}\")", body))
                        .await
                        .ok();
                }
            }
            ConnectionEvent::Pong(data) => {
                if show_ping_pong {
                    let body = String::from_utf8_lossy(&data);
                    console
                        .print_event(&format!("Received pong (data: \"{}\")", body))
                        .await
                        .ok();
                }
            }
            ConnectionEvent::Close { code, reason } => {
                let code_str = code
                    .map(|c| c.as_u16().to_string())
                    .unwrap_or_else(|| "<no-code>".to_string());
                console
                    .print_status(&format!(
                        "Disconnected (code: {}, reason: \"{}\")",
                        code_str, reason
                    ))
                    .await
                    .ok();
            }
            ConnectionEvent::Error(err) => {
                console.print_warning(&format!("Error: {}", err)).await.ok();
            }
            ConnectionEvent::Connected { .. } | ConnectionEvent::StateChanged(_) => {}
        }
    }
    Ok(())
}

async fn drain_events_silent(
    ws: &mut WebSocketClientConnection<SecureRandom>,
) -> Result<(), AnyError> {
    while let Some(event) = ws.poll_event() {
        if let ConnectionEvent::Error(err) = event {
            tracing::warn!(error = %err, "websocket reported an error");
        }
    }
    Ok(())
}
