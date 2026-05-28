//! 接続状態・イベント・出力の共通型定義
//!
//! クライアント / サーバー両方の WebSocket 接続で共有される型をこのモジュールに集約する。

use crate::websocket_close::CloseCode;

/// 乱数生成のトレイト
///
/// WebSocket クライアントが使用する masking key と nonce を生成するためのトレイト。
/// ライブラリはこのトレイトのみを提供し、実装は利用者側で行う。
///
/// # Example
///
/// ```ignore
/// // 本番環境: 暗号学的に安全な乱数を使用
/// pub struct SecureRandom;
///
/// impl RandomSource for SecureRandom {
///     fn masking_key(&mut self) -> [u8; 4] {
///         let mut key = [0u8; 4];
///         getrandom::fill(&mut key).expect("failed to generate masking key");
///         key
///     }
///
///     fn nonce(&mut self) -> [u8; 16] {
///         let mut nonce = [0u8; 16];
///         getrandom::fill(&mut nonce).expect("failed to generate nonce");
///         nonce
///     }
/// }
///
/// // テスト環境: 固定値を使用
/// pub struct FixedRandom {
///     pub masking_key: [u8; 4],
///     pub nonce: [u8; 16],
/// }
///
/// impl RandomSource for FixedRandom {
///     fn masking_key(&mut self) -> [u8; 4] { self.masking_key }
///     fn nonce(&mut self) -> [u8; 16] { self.nonce }
/// }
/// ```
pub trait RandomSource: Send {
    /// masking key (4 bytes) を生成する
    fn masking_key(&mut self) -> [u8; 4];

    /// nonce (16 bytes) を生成する
    fn nonce(&mut self) -> [u8; 16];
}

/// 接続状態
///
/// 許可される状態遷移は以下のとおり。同一状態への遷移は no-op として扱う。
///
/// | 現在の状態 | 遷移先 | 起点 |
/// |---|---|---|
/// | `Disconnected` | `Connecting` | client `connect()` / server ハンドシェイク受信 |
/// | `Connecting` | `Connected` | client / server ハンドシェイク完了 |
/// | `Connecting` | `Closed` | server `reject_handshake()` |
/// | `Connected` | `Closing` | Close フレーム送信 (`close_internal`) |
/// | `Connected` | `Closed` | 相手 Close フレーム受信 (close 未送信時) |
/// | `Closing` | `Closed` | 相手 Close フレーム受信 / Close タイムアウト |
///
/// 表外の遷移は `set_state` が `Error::invalid_state` を返す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionState {
    /// 切断状態
    #[default]
    Disconnected,
    /// ハンドシェイク中
    Connecting,
    /// 接続確立
    Connected,
    /// クローズハンドシェイク中
    Closing,
    /// 切断完了
    Closed,
}

impl ConnectionState {
    /// `next` への遷移が許可されているかを返す。同一状態への遷移も許可する
    #[doc(hidden)]
    pub fn can_transition_to(self, next: ConnectionState) -> bool {
        if self == next {
            return true;
        }
        matches!(
            (self, next),
            (Self::Disconnected, Self::Connecting)
                | (Self::Connecting, Self::Connected)
                | (Self::Connecting, Self::Closed)
                | (Self::Connected, Self::Closing)
                | (Self::Connected, Self::Closed)
                | (Self::Closing, Self::Closed)
        )
    }
}

/// タイマー ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimerId {
    /// Ping 送信タイマー
    Ping,
    /// Pong タイムアウト
    PongTimeout,
    /// クローズタイムアウト
    CloseTimeout,
}

/// 接続イベント
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionEvent {
    /// 接続完了
    Connected {
        /// ネゴシエートされたサブプロトコル
        protocol: Option<String>,
        /// ネゴシエートされた拡張
        extensions: Vec<String>,
    },
    /// テキストメッセージ受信
    TextMessage(String),
    /// バイナリメッセージ受信
    BinaryMessage(Vec<u8>),
    /// Ping 受信
    Ping(Vec<u8>),
    /// Pong 受信
    Pong(Vec<u8>),
    /// クローズ受信
    Close {
        code: Option<CloseCode>,
        reason: String,
    },
    /// 状態変化
    StateChanged(ConnectionState),
    /// エラー発生
    Error(String),
}

/// 接続出力アクション
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionOutput {
    /// データ送信
    SendData(Vec<u8>),
    /// タイマー設定
    SetTimer { id: TimerId, duration_millis: u64 },
    /// タイマークリア
    ClearTimer { id: TimerId },
    /// 接続をクローズ
    CloseConnection,
}
