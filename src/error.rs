use std::backtrace::{Backtrace, BacktraceStatus};
use std::panic::Location;

/// エラーの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// 入力データの形式または構造が無効である
    InvalidInput,

    /// データコンテンツが無効または破損している
    InvalidData,

    /// 構造体などの内部状態が不正だったり、依頼された操作を実行可能ではない
    InvalidState,

    /// 提供されたバッファがエンコード/デコード結果を保持するのに小さすぎる
    InsufficientBuffer,

    /// 操作またはデータ形式がサポートされていない
    Unsupported,

    /// プロトコル違反が検出された
    ProtocolViolation,

    /// ハンドシェイクが拒否された
    HandshakeRejected,

    /// サポートされていない WebSocket バージョンを要求された
    VersionNotSupported,

    /// 無効な UTF-8 データ
    InvalidUtf8,

    /// WebSocket ハンドシェイクで 101 以外の HTTP レスポンスを受信した
    ///
    /// RFC 6455 Section 4.1: クライアントは 101 以外のレスポンスを
    /// HTTP procedures に従って処理する必要がある
    HttpResponse,
}

/// HTTP レスポンス情報
///
/// WebSocket ハンドシェイクで 101 以外のレスポンスを受信した場合に、
/// HTTP procedures を実行するために必要な情報を保持する
#[derive(Debug, Clone)]
pub struct HttpResponseInfo {
    /// HTTP ステータスコード
    pub status_code: u16,
    /// HTTP reason phrase
    pub reason_phrase: String,
    /// HTTP レスポンスヘッダー
    pub headers: Vec<(String, String)>,
}

/// エラー型
pub struct Error {
    /// 発生したエラーの種類
    pub kind: ErrorKind,

    /// エラーが発生した理由
    pub reason: String,

    /// エラーが作成されたソースコードの場所
    pub location: &'static Location<'static>,

    /// エラー発生箇所を示すバックトレース
    ///
    /// バックトレースは `RUST_BACKTRACE` 環境変数が設定されていない場合には取得されない
    pub backtrace: Backtrace,

    /// WebSocket クローズコード（該当する場合）
    pub close_code: Option<u16>,

    /// HTTP レスポンス情報（101 以外のレスポンスを受信した場合）
    pub http_response: Option<Box<HttpResponseInfo>>,
}

impl Error {
    /// [`Error`] インスタンスを生成する
    #[track_caller]
    pub fn new(kind: ErrorKind) -> Self {
        Self::with_reason(kind, String::new())
    }

    /// エラー理由つきで [`Error`] インスタンスを生成する
    #[track_caller]
    pub fn with_reason<T: Into<String>>(kind: ErrorKind, reason: T) -> Self {
        Self {
            kind,
            reason: reason.into(),
            location: Location::caller(),
            backtrace: Backtrace::capture(),
            close_code: None,
            http_response: None,
        }
    }

    #[track_caller]
    pub(crate) fn invalid_input<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(ErrorKind::InvalidInput, reason)
    }

    #[track_caller]
    pub(crate) fn invalid_data<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(ErrorKind::InvalidData, reason)
    }

    #[track_caller]
    pub(crate) fn invalid_state<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(ErrorKind::InvalidState, reason)
    }

    // 他の ErrorKind コンストラクタとの一貫性のため温存する。
    // 使用が開始されると expect(dead_code) が unfulfilled となり、属性除去のタイミングを検知できる。
    #[track_caller]
    #[expect(dead_code)]
    pub(crate) fn unsupported<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(ErrorKind::Unsupported, reason)
    }

    #[track_caller]
    pub(crate) fn insufficient_buffer() -> Self {
        Self::new(ErrorKind::InsufficientBuffer)
    }

    #[track_caller]
    pub(crate) fn protocol_violation<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(ErrorKind::ProtocolViolation, reason)
    }

    #[track_caller]
    pub(crate) fn handshake_rejected<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(ErrorKind::HandshakeRejected, reason)
    }

    #[track_caller]
    pub(crate) fn version_not_supported<T: Into<String>>(received: T) -> Self {
        Self::with_reason(ErrorKind::VersionNotSupported, received)
    }

    #[track_caller]
    pub(crate) fn invalid_utf8<T: Into<String>>(reason: T) -> Self {
        Self::with_reason(ErrorKind::InvalidUtf8, reason)
    }

    #[track_caller]
    pub(crate) fn check_buffer_size(required_size: usize, buf: &[u8]) -> Result<(), Self> {
        if buf.len() < required_size {
            Err(Self::insufficient_buffer())
        } else {
            Ok(())
        }
    }

    /// WebSocket クローズコードを設定する
    pub fn with_close_code(mut self, code: u16) -> Self {
        self.close_code = Some(code);
        self
    }

    /// HTTP レスポンスエラーを生成する
    ///
    /// WebSocket ハンドシェイクで 101 以外の HTTP レスポンスを受信した場合に使用する
    #[track_caller]
    pub(crate) fn http_response(info: HttpResponseInfo) -> Self {
        let reason = format!("HTTP {} {}", info.status_code, info.reason_phrase);
        let mut err = Self::with_reason(ErrorKind::HttpResponse, reason);
        err.http_response = Some(Box::new(info));
        err
    }
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.reason)?;
        if let Some(code) = self.close_code {
            write!(f, " (WebSocket close code: {code})")?;
        }
        write!(f, " (at {}:{})", self.location.file(), self.location.line())?;
        if self.backtrace.status() == BacktraceStatus::Captured {
            write!(f, "\n\nBacktrace:\n{}", self.backtrace)?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}
