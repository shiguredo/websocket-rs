//! `--listen` モードの本体
//!
//! wscat 本家と同様、単一接続のみ受け付ける。2 つ目以降の接続は即座に切断する。

use std::collections::HashMap;
use std::time::Duration;

use shiguredo_websocket::{
    CloseCode, ConnectionEvent, ConnectionOutput, ConnectionState, ServerConnectionOptions,
    TimerId, WebSocketServerConnection,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, Sleep, sleep_until};

use crate::cli::Cli;
use crate::common::AnyError;
use crate::console::Console;
use crate::slash::{self, SlashCmd};

/// セッションがメインループへ通知する内部メッセージ
enum SessionEvent {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close { code: Option<u16>, reason: String },
    Disconnected,
}

/// メインループからセッションへ送るコマンド
enum SessionCmd {
    SendText(String),
    SendPing(Vec<u8>),
    SendPong(Vec<u8>),
    Close { code: u16, reason: String },
}

struct Session {
    handle: JoinHandle<()>,
    cmd_tx: mpsc::Sender<SessionCmd>,
    event_rx: mpsc::Receiver<SessionEvent>,
}

impl Session {
    async fn shutdown(self) {
        drop(self.cmd_tx);
        drop(self.event_rx);
        let _ = self.handle.await;
    }
}

pub async fn run(cli: &Cli, port: u16) -> Result<(), AnyError> {
    let mut console = Console::new(cli.color, true);
    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .map_err(|e| format!("failed to bind 0.0.0.0:{}: {}", port, e))?;
    console
        .print_status(&format!(
            "Listening on port {} (press CTRL+C to quit)",
            port
        ))
        .await
        .ok();

    let mut active: Option<Session> = None;
    let mut stdin_done = false;

    let stdin = tokio::io::stdin();
    let mut stdin = BufReader::new(stdin).lines();

    loop {
        if let Some(session) = &mut active {
            tokio::select! {
                biased;
                _ = tokio::signal::ctrl_c() => {
                    let _ = session.cmd_tx.send(SessionCmd::Close { code: 1000, reason: String::new() }).await;
                    if let Some(s) = active.take() {
                        s.shutdown().await;
                    }
                    break;
                }
                accept = listener.accept() => {
                    if let Ok((stream, _)) = accept {
                        // 単一接続ポリシー: 2 つ目以降は drop して切断
                        drop(stream);
                    }
                }
                line = stdin.next_line(), if !stdin_done => {
                    let line = line.map_err(|e| format!("stdin error: {}", e))?;
                    let line = match line {
                        Some(l) => l,
                        None => {
                            stdin_done = true;
                            continue;
                        }
                    };
                    handle_user_input(cli, &mut console, &session.cmd_tx, &line).await;
                }
                ev = session.event_rx.recv() => {
                    match ev {
                        Some(SessionEvent::Text(text)) => {
                            console.print_incoming(&text).await.ok();
                        }
                        Some(SessionEvent::Binary(data)) => {
                            let text = String::from_utf8_lossy(&data).into_owned();
                            console.print_incoming(&text).await.ok();
                        }
                        Some(SessionEvent::Ping(data)) => {
                            if cli.show_ping_pong {
                                let body = String::from_utf8_lossy(&data);
                                console.print_event(&format!("Received ping (data: \"{}\")", body)).await.ok();
                            }
                        }
                        Some(SessionEvent::Pong(data)) => {
                            if cli.show_ping_pong {
                                let body = String::from_utf8_lossy(&data);
                                console.print_event(&format!("Received pong (data: \"{}\")", body)).await.ok();
                            }
                        }
                        Some(SessionEvent::Close { code, reason }) => {
                            let code_str = code.map(|c| c.to_string()).unwrap_or_else(|| "<no-code>".to_string());
                            console.print_status(&format!(
                                "Disconnected (code: {}, reason: \"{}\")",
                                code_str, reason
                            )).await.ok();
                        }
                        Some(SessionEvent::Disconnected) | None => {
                            if let Some(s) = active.take() {
                                s.shutdown().await;
                            }
                        }
                    }
                }
            }
        } else {
            tokio::select! {
                biased;
                _ = tokio::signal::ctrl_c() => break,
                accept = listener.accept() => {
                    let (stream, peer) = accept.map_err(|e| format!("accept error: {}", e))?;
                    let _ = stream.set_nodelay(true);
                    console.print_status(&format!("Client connected from {}", peer)).await.ok();
                    active = Some(spawn_session(stream));
                }
            }
        }
    }
    Ok(())
}

fn spawn_session(stream: TcpStream) -> Session {
    let (cmd_tx, cmd_rx) = mpsc::channel::<SessionCmd>(16);
    let (event_tx, event_rx) = mpsc::channel::<SessionEvent>(64);

    let handle = tokio::spawn(async move {
        if let Err(e) = run_session(stream, cmd_rx, event_tx.clone()).await {
            tracing::warn!(error = %e, "session ended with error");
        }
        let _ = event_tx.send(SessionEvent::Disconnected).await;
    });

    Session {
        handle,
        cmd_tx,
        event_rx,
    }
}

async fn run_session(
    mut stream: TcpStream,
    mut cmd_rx: mpsc::Receiver<SessionCmd>,
    event_tx: mpsc::Sender<SessionEvent>,
) -> Result<(), AnyError> {
    let options = ServerConnectionOptions::new();
    let mut ws = WebSocketServerConnection::new(options);
    let mut buf = vec![0u8; 8192];
    let mut handshake_done = false;
    let mut timers: HashMap<TimerId, Instant> = HashMap::new();
    let mut timer_box: Option<std::pin::Pin<Box<Sleep>>> = None;

    loop {
        let timer_fut = make_timer_fut(&timers, &mut timer_box);

        tokio::select! {
            biased;
            res = stream.read(&mut buf) => {
                let n = res.map_err(|e| format!("session read error: {}", e))?;
                if n == 0 {
                    break;
                }
                ws.feed_recv_buf(&buf[..n]).map_err(|e| format!("session recv error: {}", e))?;
                if !handshake_done && ws.state() == ConnectionState::Connecting {
                    ws.accept_handshake_auto().map_err(|e| format!("handshake reject: {}", e))?;
                    handshake_done = true;
                }
                while let Some(event) = ws.poll_event() {
                    match event {
                        ConnectionEvent::TextMessage(text) => {
                            let _ = event_tx.send(SessionEvent::Text(text)).await;
                        }
                        ConnectionEvent::BinaryMessage(data) => {
                            let _ = event_tx.send(SessionEvent::Binary(data)).await;
                        }
                        ConnectionEvent::Ping(data) => {
                            let _ = event_tx.send(SessionEvent::Ping(data)).await;
                        }
                        ConnectionEvent::Pong(data) => {
                            let _ = event_tx.send(SessionEvent::Pong(data)).await;
                        }
                        ConnectionEvent::Close { code, reason } => {
                            let _ = event_tx.send(SessionEvent::Close {
                                code: code.map(|c| c.as_u16()),
                                reason,
                            }).await;
                        }
                        _ => {}
                    }
                }
                drain_server_outputs(&mut ws, &mut stream, &mut timers).await?;
                if matches!(ws.state(), ConnectionState::Closed) {
                    break;
                }
            }
            cmd = cmd_rx.recv() => {
                let cmd = match cmd {
                    Some(c) => c,
                    None => {
                        // メインループ側が drop したのでセッションも終了する
                        if matches!(ws.state(), ConnectionState::Connected | ConnectionState::Closing) {
                            let _ = ws.close(CloseCode::NORMAL, "");
                            drain_server_outputs(&mut ws, &mut stream, &mut timers).await?;
                        }
                        break;
                    }
                };
                match cmd {
                    SessionCmd::SendText(text) => {
                        if let Err(e) = ws.send_text(&text) {
                            tracing::warn!(error = %e, "send_text failed");
                        }
                    }
                    SessionCmd::SendPing(data) => {
                        if let Err(e) = ws.send_ping(&data) {
                            tracing::warn!(error = %e, "send_ping failed");
                        }
                    }
                    SessionCmd::SendPong(data) => {
                        if let Err(e) = ws.send_pong(&data) {
                            tracing::warn!(error = %e, "send_pong failed");
                        }
                    }
                    SessionCmd::Close { code, reason } => {
                        let close_code = CloseCode::new(code);
                        if let Err(e) = ws.close(close_code, &reason) {
                            tracing::warn!(error = %e, "close failed");
                        }
                    }
                }
                drain_server_outputs(&mut ws, &mut stream, &mut timers).await?;
            }
            id = timer_fut => {
                if let Err(e) = ws.handle_timer(id) {
                    tracing::warn!(error = %e, "handle_timer failed");
                }
                drain_server_outputs(&mut ws, &mut stream, &mut timers).await?;
            }
        }
    }
    Ok(())
}

async fn drain_server_outputs(
    ws: &mut WebSocketServerConnection,
    stream: &mut TcpStream,
    timers: &mut HashMap<TimerId, Instant>,
) -> Result<(), AnyError> {
    while let Some(output) = ws.poll_output() {
        match output {
            ConnectionOutput::SendData(data) => {
                stream
                    .write_all(&data)
                    .await
                    .map_err(|e| format!("session write error: {}", e))?;
            }
            ConnectionOutput::SetTimer {
                id,
                duration_millis,
            } => {
                timers.insert(id, Instant::now() + Duration::from_millis(duration_millis));
            }
            ConnectionOutput::ClearTimer { id } => {
                timers.remove(&id);
            }
            ConnectionOutput::CloseConnection => {
                let _ = stream.shutdown().await;
            }
        }
    }
    Ok(())
}

fn make_timer_fut<'a>(
    timers: &'a HashMap<TimerId, Instant>,
    slot: &'a mut Option<std::pin::Pin<Box<Sleep>>>,
) -> impl std::future::Future<Output = TimerId> + 'a {
    let next = timers
        .iter()
        .min_by_key(|(_, d)| **d)
        .map(|(id, d)| (*id, *d));
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

async fn handle_user_input(
    cli: &Cli,
    console: &mut Console,
    cmd_tx: &mpsc::Sender<SessionCmd>,
    line: &str,
) {
    if cli.slash
        && let Some(cmd) = slash::parse(line)
    {
        match cmd {
            SlashCmd::Ping(data) => {
                let _ = cmd_tx
                    .send(SessionCmd::SendPing(data.unwrap_or_default().into_bytes()))
                    .await;
            }
            SlashCmd::Pong(data) => {
                let _ = cmd_tx
                    .send(SessionCmd::SendPong(data.unwrap_or_default().into_bytes()))
                    .await;
            }
            SlashCmd::Close { code, reason } => {
                let _ = cmd_tx
                    .send(SessionCmd::Close {
                        code: code.unwrap_or(1000),
                        reason: reason.unwrap_or_default(),
                    })
                    .await;
            }
            SlashCmd::Unknown => {
                console
                    .print_warning("Unrecognized slash command.")
                    .await
                    .ok();
            }
        }
        return;
    }
    let _ = cmd_tx.send(SessionCmd::SendText(line.to_string())).await;
    console.write_prompt_if_interactive().await.ok();
}
