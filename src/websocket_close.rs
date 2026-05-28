/// reason が max_bytes を超える場合、UTF-8 文字境界で切り詰める
///
/// 内部向け API。`lib.rs` で `#[doc(hidden)] pub use` 経由で PBT に公開する。
/// 通常の `pub use` で公開しないこと。
pub fn truncate_reason(reason: &str, max_bytes: usize) -> &str {
    if reason.len() > max_bytes {
        let mut end = max_bytes;
        while !reason.is_char_boundary(end) {
            end -= 1;
        }
        &reason[..end]
    } else {
        reason
    }
}

/// WebSocket クローズコード (RFC 6455 Section 7.4.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CloseCode(pub u16);

impl CloseCode {
    /// 正常終了
    pub const NORMAL: Self = Self(1000);
    /// エンドポイントが離脱（サーバーダウン、ブラウザ移動など）
    pub const GOING_AWAY: Self = Self(1001);
    /// プロトコルエラー
    pub const PROTOCOL_ERROR: Self = Self(1002);
    /// 受け入れられないデータ型
    pub const UNSUPPORTED_DATA: Self = Self(1003);
    /// 予約済み（将来の定義用）
    pub const RESERVED: Self = Self(1004);
    /// ステータスコードなし（アプリケーション用、送信禁止）
    pub const NO_STATUS_RECEIVED: Self = Self(1005);
    /// 異常切断（アプリケーション用、送信禁止）
    pub const ABNORMAL_CLOSURE: Self = Self(1006);
    /// 無効なペイロードデータ（例: テキストメッセージ内の非 UTF-8）
    pub const INVALID_PAYLOAD: Self = Self(1007);
    /// ポリシー違反
    pub const POLICY_VIOLATION: Self = Self(1008);
    /// メッセージが大きすぎる
    pub const MESSAGE_TOO_BIG: Self = Self(1009);
    /// 必須の拡張機能がない
    pub const MANDATORY_EXTENSION: Self = Self(1010);
    /// 内部サーバーエラー
    pub const INTERNAL_ERROR: Self = Self(1011);
    /// TLS ハンドシェイク失敗（アプリケーション用、送信禁止）
    pub const TLS_HANDSHAKE: Self = Self(1015);

    /// クローズコードを生成する
    pub fn new(code: u16) -> Self {
        Self(code)
    }

    /// クローズコードを u16 として取得する
    pub fn as_u16(self) -> u16 {
        self.0
    }

    /// 有効なクローズコードかどうか（受信時の検証用）
    ///
    /// RFC 6455 Section 7.4.1/7.4.2:
    /// - 0-999: 使用禁止
    /// - 1005, 1006, 1015: 送信禁止 (MUST NOT be set as a status code)
    ///   → 受信時もプロトコルエラーとして扱う
    /// - 5000 以上: RFC で定義されていない
    ///
    /// 1004, 1012-2999 は「予約済み」だが、受信時は許容する。
    pub fn is_valid(self) -> bool {
        matches!(self.0, 1000..=1004 | 1007..=1014 | 1016..=4999)
    }

    /// このコードを送信可能かどうか
    ///
    /// RFC 6455 Section 7.4.1/7.4.2:
    /// - 0-999: 使用禁止
    /// - 1004: 予約済み（未定義）
    /// - 1005, 1006, 1015: 送信禁止 (MUST NOT be set as a status code)
    /// - 2000-2999: 予約済み（RFC 6455 および将来の拡張用）
    /// - 5000以上: RFC で定義されていない範囲
    ///
    /// 1012-1014 は IANA に登録されているため許容する。
    pub fn is_sendable(self) -> bool {
        !matches!(self.0, 0..=999 | 1004 | 1005 | 1006 | 1015 | 2000..=2999 | 5000..)
    }
}

impl From<u16> for CloseCode {
    fn from(code: u16) -> Self {
        Self(code)
    }
}

impl From<CloseCode> for u16 {
    fn from(code: CloseCode) -> Self {
        code.0
    }
}

impl std::fmt::Display for CloseCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let description = match self.0 {
            1000 => "Normal Closure",
            1001 => "Going Away",
            1002 => "Protocol Error",
            1003 => "Unsupported Data",
            1004 => "Reserved",
            1005 => "No Status Received",
            1006 => "Abnormal Closure",
            1007 => "Invalid Payload Data",
            1008 => "Policy Violation",
            1009 => "Message Too Big",
            1010 => "Mandatory Extension",
            1011 => "Internal Error",
            1015 => "TLS Handshake",
            3000..=3999 => "Library/Framework",
            4000..=4999 => "Application",
            _ => "Unknown",
        };
        write!(f, "{} ({})", self.0, description)
    }
}
