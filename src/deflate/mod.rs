//! WebSocket 圧縮拡張モジュール
//!
//! RFC 7692 permessage-deflate 拡張の実装

mod permessage_deflate;

pub use permessage_deflate::{Compressor, Decompressor, PerMessageDeflate};
