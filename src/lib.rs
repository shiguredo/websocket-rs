//! Sans I/O な WebSocket ライブラリ
//!
//! このライブラリは I/O を含まない純粋な WebSocket プロトコル実装を提供します。
//! HTTP/1.1 上での WebSocket 接続のみをサポートしています。
//!
//! # 特徴
//!
//! - Sans I/O パターンによる I/O 非依存の設計
//! - クライアント・サーバー両方の接続をサポート
//! - permessage-deflate 拡張 (RFC 7692) に対応
//! - フレームの直接操作が可能な低レベル API
//!
//! # クライアント接続
//!
//! ```no_run
//! use shiguredo_websocket::{
//!     ClientConnectionOptions, ConnectionEvent, ConnectionOutput,
//!     RandomSource, WebSocketClientConnection, Timestamp,
//! };
//!
//! // 乱数ソースの実装（実際には getrandom などを使用）
//! struct DemoRandom {
//!     counter: u32,
//! }
//!
//! impl RandomSource for DemoRandom {
//!     fn masking_key(&mut self) -> [u8; 4] {
//!         self.counter = self.counter.wrapping_add(1);
//!         self.counter.to_le_bytes()
//!     }
//!     fn nonce(&mut self) -> [u8; 16] {
//!         [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
//!     }
//! }
//!
//! // WebSocket 接続オプション
//! let options = ClientConnectionOptions::new("example.com", "/");
//!
//! // WebSocket 接続作成
//! let mut ws = WebSocketClientConnection::new(options, DemoRandom { counter: 0 });
//!
//! // 接続開始
//! ws.connect().expect("handshake must succeed");
//!
//! // 出力データを取得してソケットに送信
//! while let Some(output) = ws.poll_output() {
//!     if let ConnectionOutput::SendData(data) = output {
//!         // data をソケットに送信
//!     }
//! }
//! ```
//!
//! # サーバー接続
//!
//! ```no_run
//! use shiguredo_websocket::{
//!     ServerConnectionOptions, WebSocketServerConnection,
//! };
//!
//! // サーバー接続オプション
//! let options = ServerConnectionOptions::new();
//!
//! // WebSocket サーバー接続作成
//! let mut ws = WebSocketServerConnection::new(options);
//! ```
//!
//! # 準拠規格
//!
//! - RFC 6455 - The WebSocket Protocol
//! - RFC 7692 - Compression Extensions for WebSocket

mod buf;
mod deflate;
mod error;
mod time;
mod token;
mod websocket_client_connection;
mod websocket_close;
mod websocket_connection_shared;
mod websocket_connection_types;
mod websocket_extension;
mod websocket_frame;
mod websocket_handshake;
mod websocket_opcode;
mod websocket_server_connection;

pub use deflate::PerMessageDeflate;
pub use error::{Error, ErrorKind, HttpResponseInfo};
pub use time::Timestamp;
pub use websocket_client_connection::{ClientConnectionOptions, WebSocketClientConnection};
pub use websocket_close::CloseCode;
pub use websocket_connection_types::{
    ConnectionEvent, ConnectionOutput, ConnectionState, RandomSource, TimerId,
};
pub use websocket_extension::{
    Extension, ExtensionParam, ExtensionParseContext, ExtensionParseError, PerMessageDeflateConfig,
};
pub use websocket_frame::{Frame, FrameDecoder};
pub use websocket_handshake::{HandshakeRequest, HandshakeResponse, HandshakeValidator};
pub use websocket_handshake::{
    HandshakeRequestValidator, ServerHandshakeRequest, ServerHandshakeResponse,
};
pub use websocket_opcode::Opcode;
pub use websocket_server_connection::{ServerConnectionOptions, WebSocketServerConnection};

// 内部向け API。クレート外の通常利用は想定していないが、PBT (`ByteSliceExt` / `VecExt` /
// `truncate_reason`) および fuzz (`Decompressor`) から参照されるため pub のまま、通常の
// pub use では公開せず #[doc(hidden)] でドキュメント上は非表示にする。
// `Compressor` は同経路で hidden 再エクスポートする（PerMessageDeflate と対になる API のため）。
#[doc(hidden)]
pub use buf::{ByteSliceExt, VecExt};
#[doc(hidden)]
pub use deflate::{Compressor, Decompressor};
#[doc(hidden)]
pub use token::is_valid_token;
#[doc(hidden)]
pub use websocket_close::truncate_reason;
