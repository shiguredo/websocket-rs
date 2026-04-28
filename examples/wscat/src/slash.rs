//! スラッシュコマンドのパース。`--slash` 有効時に行頭 `/` で起動する。
//!
//! 対応コマンド:
//! - `/ping [data]`
//! - `/pong [data]`  (RFC 6455 §5.5.3)
//! - `/close [code] [reason...]`

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCmd {
    Ping(Option<String>),
    Pong(Option<String>),
    Close {
        code: Option<u16>,
        reason: Option<String>,
    },
    Unknown,
}

/// 行頭の `/` を取り除いてコマンドをパースする
pub fn parse(line: &str) -> Option<SlashCmd> {
    let rest = line.strip_prefix('/')?;
    let mut iter = rest.splitn(2, char::is_whitespace);
    let head = iter.next()?;
    let tail = iter.next();
    match head {
        "ping" => Some(SlashCmd::Ping(tail.map(|s| s.to_string()))),
        "pong" => Some(SlashCmd::Pong(tail.map(|s| s.to_string()))),
        "close" => {
            // close は最大 2 トークン (code, reason)。reason は残り全部を join する
            let mut tokens = tail.unwrap_or("").splitn(2, char::is_whitespace);
            let code = tokens
                .next()
                .filter(|s| !s.is_empty())
                .and_then(|s| s.parse::<u16>().ok());
            let reason = tokens
                .next()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty());
            Some(SlashCmd::Close { code, reason })
        }
        _ => Some(SlashCmd::Unknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_no_slash() {
        assert_eq!(parse("hello"), None);
    }

    #[test]
    fn parse_ping_no_data() {
        assert_eq!(parse("/ping"), Some(SlashCmd::Ping(None)));
    }

    #[test]
    fn parse_ping_with_data() {
        assert_eq!(
            parse("/ping hello"),
            Some(SlashCmd::Ping(Some("hello".to_string())))
        );
    }

    #[test]
    fn parse_pong_with_data() {
        assert_eq!(
            parse("/pong world"),
            Some(SlashCmd::Pong(Some("world".to_string())))
        );
    }

    #[test]
    fn parse_close_no_args() {
        assert_eq!(
            parse("/close"),
            Some(SlashCmd::Close {
                code: None,
                reason: None,
            })
        );
    }

    #[test]
    fn parse_close_code_only() {
        assert_eq!(
            parse("/close 1001"),
            Some(SlashCmd::Close {
                code: Some(1001),
                reason: None,
            })
        );
    }

    #[test]
    fn parse_close_with_reason() {
        assert_eq!(
            parse("/close 1000 going away"),
            Some(SlashCmd::Close {
                code: Some(1000),
                reason: Some("going away".to_string()),
            })
        );
    }

    #[test]
    fn parse_unknown() {
        assert_eq!(parse("/foo bar"), Some(SlashCmd::Unknown));
    }
}
