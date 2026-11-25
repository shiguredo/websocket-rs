//! WebSocket 圧縮拡張モジュール
//!
//! RFC 7692 permessage-deflate 拡張の実装

mod permessage_deflate;

pub use permessage_deflate::PerMessageDeflate;

// 内部向け API。lib.rs から #[doc(hidden)] で再エクスポートするため pub のまま公開する。
// pub(crate) にすると lib.rs 側の pub use（#[doc(hidden)] を含む）でエラーになる。
pub use permessage_deflate::{Compressor, Decompressor};
