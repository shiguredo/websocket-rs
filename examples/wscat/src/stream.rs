//! 動的ディスパッチを使った I/O ストリーム抽象
//!
//! クライアントは `TcpStream` / TLS 上の `TlsStream` / プロキシ越しの TLS-on-TLS など
//! 多様なストリームを扱う必要がある。これらをまとめて `Box<dyn IoStream>` で扱う。

use tokio::io::{AsyncRead, AsyncWrite};

pub trait IoStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> IoStream for T where T: AsyncRead + AsyncWrite + Unpin + Send + ?Sized {}

pub type DynStream = Box<dyn IoStream>;
