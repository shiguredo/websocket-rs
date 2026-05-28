//! フラグメント収集バッファ
//!
//! RFC 6455 Section 5.4 のフラグメント化メッセージ (FIN=0 の継続フレーム列)
//! を組み立てるためのバッファ。最初のフレームの opcode と RSV1 (圧縮フラグ)
//! を保持し、後続の継続フレームのペイロードを連結する。

use crate::websocket_opcode::Opcode;

/// フラグメント収集バッファ
#[derive(Debug, Default)]
pub(crate) struct FragmentBuffer {
    /// 最初のフレームのオペコード
    opcode: Option<Opcode>,
    /// 収集中のペイロード
    payload: Vec<u8>,
    /// メッセージが圧縮されているか (最初のフレームの RSV1)
    compressed: bool,
}

impl FragmentBuffer {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.opcode.is_none()
    }

    pub(crate) fn len(&self) -> usize {
        self.payload.len()
    }

    pub(crate) fn start(&mut self, opcode: Opcode, payload: Vec<u8>, compressed: bool) {
        self.opcode = Some(opcode);
        self.payload = payload;
        self.compressed = compressed;
    }

    pub(crate) fn append(&mut self, payload: &[u8]) {
        self.payload.extend_from_slice(payload);
    }

    pub(crate) fn take(&mut self) -> (Opcode, Vec<u8>, bool) {
        let opcode = self
            .opcode
            .take()
            .expect("FragmentBuffer::take called on empty buffer");
        let payload = std::mem::take(&mut self.payload);
        let compressed = self.compressed;
        self.compressed = false;
        (opcode, payload, compressed)
    }

    pub(crate) fn clear(&mut self) {
        self.opcode = None;
        self.payload.clear();
        self.compressed = false;
    }
}
