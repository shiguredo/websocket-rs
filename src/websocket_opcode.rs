/// WebSocket フレームのオペコード (RFC 6455 Section 5.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Opcode {
    /// 継続フレーム
    Continuation = 0x0,
    /// テキストフレーム
    Text = 0x1,
    /// バイナリフレーム
    Binary = 0x2,
    /// 接続クローズ
    Close = 0x8,
    /// Ping
    Ping = 0x9,
    /// Pong
    Pong = 0xA,
}

impl Opcode {
    /// u8 から Opcode を生成する
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x0 => Some(Self::Continuation),
            0x1 => Some(Self::Text),
            0x2 => Some(Self::Binary),
            0x8 => Some(Self::Close),
            0x9 => Some(Self::Ping),
            0xA => Some(Self::Pong),
            _ => None,
        }
    }

    /// Opcode を u8 に変換する
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// コントロールフレームかどうか
    pub fn is_control(self) -> bool {
        matches!(self, Self::Close | Self::Ping | Self::Pong)
    }

    /// データフレームかどうか
    pub fn is_data(self) -> bool {
        matches!(self, Self::Continuation | Self::Text | Self::Binary)
    }
}

impl std::fmt::Display for Opcode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Continuation => write!(f, "Continuation"),
            Self::Text => write!(f, "Text"),
            Self::Binary => write!(f, "Binary"),
            Self::Close => write!(f, "Close"),
            Self::Ping => write!(f, "Ping"),
            Self::Pong => write!(f, "Pong"),
        }
    }
}
