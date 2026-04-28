//! コマンドライン引数のパース。Node.js 製 wscat と互換のオプション体系を提供する。

use std::path::PathBuf;

use crate::common::AnyError;

/// 動作モード (`-c/--connect` か `-l/--listen` のどちらかで決まる)
#[derive(Debug, Clone)]
pub enum Mode {
    /// クライアントとして接続する
    Connect { url: String },
    /// 単一接続のサーバーとして待ち受ける
    Listen { port: u16 },
}

/// `--wait` の値
#[derive(Debug, Clone, Copy)]
pub enum WaitSeconds {
    /// 既定値 (2 秒)
    Default,
    /// 永久に保持する (`--wait -1`)
    Forever,
    /// 指定秒数後に close する
    Secs(u64),
}

impl WaitSeconds {
    pub fn duration(self) -> Option<std::time::Duration> {
        match self {
            WaitSeconds::Default => Some(std::time::Duration::from_secs(2)),
            WaitSeconds::Forever => None,
            WaitSeconds::Secs(n) => Some(std::time::Duration::from_secs(n)),
        }
    }
}

/// パース済みのコマンドライン
#[derive(Debug, Clone)]
pub struct Cli {
    pub mode: Mode,
    pub wait: WaitSeconds,
    pub execute: Vec<String>,
    pub headers: Vec<(String, String)>,
    pub subprotocols: Vec<String>,
    pub origin: Option<String>,
    pub no_check: bool,
    pub ca: Option<PathBuf>,
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
    pub host_header: Option<String>,
    pub show_ping_pong: bool,
    pub proxy: Option<String>,
    pub max_redirects: u32,
    pub auth: Option<String>,
    pub slash: bool,
    pub follow_redirects: bool,
    pub color: bool,
}

/// 標準入力から読んだ引数をパースする。`--help` は print して終了。`--version` も同様。
pub fn parse() -> Result<Cli, AnyError> {
    let mut args = noargs::raw_args();
    args.metadata_mut().app_name = env!("CARGO_PKG_NAME");
    args.metadata_mut().app_description =
        "wscat-compatible WebSocket CLI written in Rust (client + listen).";

    if noargs::VERSION_FLAG.take(&mut args).is_present() {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    noargs::HELP_FLAG.take_help(&mut args);

    let connect_url: Option<String> = noargs::opt("connect")
        .short('c')
        .ty("URL")
        .doc("Connect to a WebSocket server (ws:// or wss://; scheme defaults to ws://)")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(o.value().to_string()))
        .map_err(stringify_err)?;

    let listen_port: Option<u16> = noargs::opt("listen")
        .short('l')
        .ty("PORT")
        .doc("Listen on the specified port (single-connection echo server, plain WS only)")
        .take(&mut args)
        .present_and_then(|o| o.value().parse())
        .map_err(stringify_err)?;

    // wait は値が "-1" のときは Forever。それ以外は u64
    let wait_raw: Option<String> = noargs::opt("wait")
        .short('w')
        .ty("SECS")
        .doc("Wait this many seconds after sending --execute messages before closing (default: 2, -1 keeps the connection open)")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(o.value().to_string()))
        .map_err(stringify_err)?;
    let wait = match wait_raw.as_deref() {
        None => WaitSeconds::Default,
        Some("-1") => WaitSeconds::Forever,
        Some(s) => WaitSeconds::Secs(
            s.parse::<u64>()
                .map_err(|e| format!("invalid --wait value: {}", e))?,
        ),
    };

    let execute = collect_repeated(
        &mut args,
        "execute",
        Some('x'),
        "MESSAGE",
        "Send the message after connecting (may be repeated; disables interactive mode)",
    );
    let headers_raw = collect_repeated(
        &mut args,
        "header",
        Some('H'),
        "KEY:VALUE",
        "Send an additional HTTP header (may be repeated)",
    );
    let subprotocols = collect_repeated(
        &mut args,
        "subprotocol",
        Some('s'),
        "PROTOCOL",
        "Add a Sec-WebSocket-Protocol value (may be repeated)",
    );

    let origin: Option<String> = noargs::opt("origin")
        .short('o')
        .ty("ORIGIN")
        .doc("Set the Origin header")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(o.value().to_string()))
        .map_err(stringify_err)?;

    let no_check: bool = noargs::flag("no-check")
        .short('n')
        .doc("Skip TLS certificate verification (wss:// only)")
        .take(&mut args)
        .is_present();

    let ca: Option<PathBuf> = noargs::opt("ca")
        .ty("PATH")
        .doc("Path to a CA certificate file (PEM)")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(PathBuf::from(o.value())))
        .map_err(stringify_err)?;
    let cert: Option<PathBuf> = noargs::opt("cert")
        .ty("PATH")
        .doc("Path to a client certificate file (PEM)")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(PathBuf::from(o.value())))
        .map_err(stringify_err)?;
    let key: Option<PathBuf> = noargs::opt("key")
        .ty("PATH")
        .doc("Path to a client private key file (PEM)")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(PathBuf::from(o.value())))
        .map_err(stringify_err)?;
    let passphrase_present: bool = noargs::opt("passphrase")
        .ty("VALUE")
        .doc("(unsupported) Passphrase for the encrypted private key. Specifying this aborts.")
        .take(&mut args)
        .is_present();

    let host_header: Option<String> = noargs::opt("host")
        .ty("HOST")
        .doc("Override the Host header")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(o.value().to_string()))
        .map_err(stringify_err)?;

    let protocol_version: Option<String> = noargs::opt("protocol")
        .short('p')
        .ty("VERSION")
        .doc("(ignored) WebSocket protocol version. RFC 6455 fixes it to 13.")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(o.value().to_string()))
        .map_err(stringify_err)?;

    let show_ping_pong: bool = noargs::flag("show-ping-pong")
        .short('P')
        .doc("Print received Ping and Pong frames")
        .take(&mut args)
        .is_present();

    let proxy: Option<String> = noargs::opt("proxy")
        .ty("URL")
        .doc("Connect through an HTTP CONNECT proxy (http:// or https://)")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(o.value().to_string()))
        .map_err(stringify_err)?;

    let max_redirects: u32 = noargs::opt("max-redirects")
        .ty("N")
        .default("10")
        .doc("Maximum number of redirects to follow when --location is set (default: 10)")
        .take(&mut args)
        .then(|o| o.value().parse())
        .map_err(stringify_err)?;

    let auth: Option<String> = noargs::opt("auth")
        .ty("USER:PASS")
        .doc("Add an HTTP Basic Authorization header")
        .take(&mut args)
        .present_and_then(|o| Ok::<_, &str>(o.value().to_string()))
        .map_err(stringify_err)?;

    let slash: bool = noargs::flag("slash")
        .doc("Enable slash commands (/ping, /pong, /close) in interactive mode")
        .take(&mut args)
        .is_present();

    let follow_redirects: bool = noargs::flag("location")
        .short('L')
        .doc("Follow HTTP redirects (3xx) returned by the WebSocket server")
        .take(&mut args)
        .is_present();

    let no_color: bool = noargs::flag("no-color")
        .doc("Disable colored output")
        .take(&mut args)
        .is_present();

    if let Some(help) = args.finish().map_err(stringify_err)? {
        print!("{}", help);
        std::process::exit(0);
    }

    if passphrase_present {
        return Err("passphrase-protected keys are not supported. \
             Decrypt the key file (e.g. with `openssl pkcs8 -in enc.pem -out plain.pem`) \
             and pass it via --key."
            .into());
    }
    if let Some(version) = protocol_version {
        tracing::warn!(
            requested = %version,
            "ignoring -p/--protocol; RFC 6455 only defines version 13"
        );
    }

    // Mode の組み立て (排他)
    let mode = match (connect_url, listen_port) {
        (Some(_), Some(_)) => return Err("--connect and --listen are mutually exclusive".into()),
        (Some(url), None) => Mode::Connect { url },
        (None, Some(port)) => Mode::Listen { port },
        (None, None) => {
            return Err("either --connect <URL> or --listen <PORT> must be specified".into());
        }
    };

    // header の `name:value` 分割
    let mut headers = Vec::with_capacity(headers_raw.len());
    for raw in headers_raw {
        let (name, value) = raw
            .split_once(':')
            .ok_or_else(|| format!("invalid header format (expected NAME:VALUE): {}", raw))?;
        headers.push((name.trim().to_string(), value.trim_start().to_string()));
    }

    // listen + 一部の -c 専用オプションが指定されている場合は警告だけ出す (本家挙動)
    if let Mode::Listen { .. } = mode {
        if !no_check {
            // ノイズ抑制のため通知不要
        }
        if ca.is_some() || cert.is_some() || key.is_some() {
            tracing::warn!(
                "--ca/--cert/--key are ignored in --listen mode (TLS server is not supported)"
            );
        }
        if proxy.is_some() {
            tracing::warn!("--proxy is ignored in --listen mode");
        }
        if follow_redirects {
            tracing::warn!("--location is ignored in --listen mode");
        }
    }

    Ok(Cli {
        mode,
        wait,
        execute,
        headers,
        subprotocols,
        origin,
        no_check,
        ca,
        cert,
        key,
        host_header,
        show_ping_pong,
        proxy,
        max_redirects,
        auth,
        slash,
        follow_redirects,
        color: !no_color,
    })
}

/// 同じ名前の opt を繰り返し取り出して値を集める
fn collect_repeated(
    args: &mut noargs::RawArgs,
    long: &'static str,
    short: Option<char>,
    ty: &'static str,
    doc: &'static str,
) -> Vec<String> {
    let mut values = Vec::new();
    loop {
        let mut spec = noargs::opt(long).ty(ty).doc(doc);
        if let Some(s) = short {
            spec = spec.short(s);
        }
        let opt = spec.take(args);
        if !opt.is_present() {
            break;
        }
        values.push(opt.value().to_string());
    }
    values
}

fn stringify_err<E: std::fmt::Debug>(e: E) -> AnyError {
    format!("{:?}", e).into()
}
