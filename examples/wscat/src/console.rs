//! 対話モードでの端末出力 (プロンプト・受信表示・ANSI カラー)

use std::io::IsTerminal;

use tokio::io::{AsyncWriteExt, Stdout};

/// ANSI エスケープシーケンス。`--no-color` または非 tty では使用しない。
const ANSI_RESET: &str = "\u{001b}[39m";
const ANSI_BLUE: &str = "\u{001b}[34m";
const ANSI_GREEN: &str = "\u{001b}[32m";
const ANSI_YELLOW: &str = "\u{001b}[33m";

/// 行頭への移動 + 行クリア + 3 カラム左 (wscat 本家 `Console.clear()` と同じ)
const ANSI_CLEAR_LINE: &str = "\r\u{001b}[2K\u{001b}[3D";

/// プロンプト文字列
pub const PROMPT: &str = "> ";

/// 端末コンソール
pub struct Console {
    stdout: Stdout,
    is_tty: bool,
    color: bool,
    interactive: bool,
}

impl Console {
    pub fn new(color: bool, interactive: bool) -> Self {
        let is_tty = std::io::stdout().is_terminal();
        Self {
            stdout: tokio::io::stdout(),
            is_tty,
            color: color && is_tty,
            interactive,
        }
    }

    /// 通常メッセージ (色なし)。`--execute` モードでは抑制する (wscat 本家準拠)。
    pub async fn print_status(&mut self, msg: &str) -> std::io::Result<()> {
        if !self.interactive {
            return Ok(());
        }
        self.clear_if_tty().await?;
        let line = format!("{}\n", msg);
        self.stdout.write_all(line.as_bytes()).await?;
        self.write_prompt_if_interactive().await?;
        Ok(())
    }

    /// `< message` (受信、青)
    pub async fn print_incoming(&mut self, payload: &str) -> std::io::Result<()> {
        self.clear_if_tty().await?;
        let line = if self.color {
            format!("{}< {}{}\n", ANSI_BLUE, payload, ANSI_RESET)
        } else {
            format!("< {}\n", payload)
        };
        self.stdout.write_all(line.as_bytes()).await?;
        self.write_prompt_if_interactive().await?;
        Ok(())
    }

    /// 「ping/pong を受信した」表示 (緑)
    pub async fn print_event(&mut self, msg: &str) -> std::io::Result<()> {
        self.clear_if_tty().await?;
        let line = if self.color {
            format!("{}{}{}\n", ANSI_GREEN, msg, ANSI_RESET)
        } else {
            format!("{}\n", msg)
        };
        self.stdout.write_all(line.as_bytes()).await?;
        self.write_prompt_if_interactive().await?;
        Ok(())
    }

    /// 警告系 (黄)
    pub async fn print_warning(&mut self, msg: &str) -> std::io::Result<()> {
        self.clear_if_tty().await?;
        let line = if self.color {
            format!("{}{}{}\n", ANSI_YELLOW, msg, ANSI_RESET)
        } else {
            format!("{}\n", msg)
        };
        self.stdout.write_all(line.as_bytes()).await?;
        self.write_prompt_if_interactive().await?;
        Ok(())
    }

    /// プロンプトを書き出す (対話モード以外では何もしない)
    pub async fn write_prompt_if_interactive(&mut self) -> std::io::Result<()> {
        if self.interactive && self.is_tty {
            self.stdout.write_all(PROMPT.as_bytes()).await?;
            self.stdout.flush().await?;
        }
        Ok(())
    }

    async fn clear_if_tty(&mut self) -> std::io::Result<()> {
        if self.interactive && self.is_tty {
            self.stdout.write_all(ANSI_CLEAR_LINE.as_bytes()).await?;
        }
        Ok(())
    }
}
