//! wscat (Node.js 製の WebSocket CLI) 互換の Rust 実装
//!
//! AGENTS.md の方針に従い RFC 準拠と堅牢性を優先する。
//! 一部の機能 (`--passphrase`、`-p/--protocol`) は非対応。

mod cli;
mod client;
mod common;
mod console;
mod proxy;
mod redirect;
mod server;
mod slash;
mod stream;
mod tls;

use crate::cli::Mode;
use crate::common::AnyError;

#[tokio::main]
async fn main() -> Result<(), AnyError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let parsed = match cli::parse() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(2);
        }
    };

    let result = match parsed.mode.clone() {
        Mode::Connect { url } => client::run(&parsed, &url).await,
        Mode::Listen { port } => server::run(&parsed, port).await,
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
    Ok(())
}
