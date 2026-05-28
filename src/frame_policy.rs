//! クライアント / サーバー間でフレーム送受信の差分を抽象化するトレイト
//!
//! RFC 6455 Section 5.1 によりクライアントは送信フレームを必ずマスクし、
//! サーバーは送信フレームをマスクしてはならない。受信側はその逆を検証する。
//! `FramePolicy` トレイトはこのマスク方向の違いを集約する。

use crate::error::Error;
use crate::websocket_connection_shared::SharedConnectionState;
use crate::websocket_connection_types::{ConnectionOutput, RandomSource};
use crate::websocket_frame::Frame;

/// フレームのエンコード方向 (マスクの有無) を抽象化するトレイト
pub(crate) trait FramePolicy {
    /// フレームのマスク方向を検証する。
    /// `masked` は `DecodedFrame.masked` から取得する。
    /// `Frame` 自体には `masked` フィールドがないため、呼び出し元で分離して渡す。
    fn verify_frame_masking(&self, masked: bool) -> Result<(), Error>;

    /// フレームをエンコードして送信キューに追加する。
    fn encode_and_send(&mut self, frame: &Frame, shared: &mut SharedConnectionState);
}

/// クライアント側のフレームポリシー
pub(crate) struct ClientFramePolicy<R: RandomSource> {
    random: R,
}

impl<R: RandomSource> ClientFramePolicy<R> {
    pub(crate) fn new(random: R) -> Self {
        Self { random }
    }

    /// ハンドシェイク用の nonce を生成する (`connect()` から利用)。
    pub(crate) fn nonce(&mut self) -> [u8; 16] {
        self.random.nonce()
    }
}

impl<R: RandomSource> FramePolicy for ClientFramePolicy<R> {
    fn verify_frame_masking(&self, masked: bool) -> Result<(), Error> {
        // RFC 6455 Section 5.1: サーバーからのフレームはマスクしてはならない
        // RFC 6455 Section 5.1, Section 7.4.1: 違反時は 1002 (protocol error) を使用してよい
        if masked {
            return Err(Error::protocol_violation("masked server frame"));
        }
        Ok(())
    }

    fn encode_and_send(&mut self, frame: &Frame, shared: &mut SharedConnectionState) {
        let masking_key = self.random.masking_key();
        let encoded = frame.encode(masking_key);
        shared.enqueue_output(ConnectionOutput::SendData(encoded));
    }
}

/// サーバー側のフレームポリシー
pub(crate) struct ServerFramePolicy;

impl FramePolicy for ServerFramePolicy {
    fn verify_frame_masking(&self, masked: bool) -> Result<(), Error> {
        // RFC 6455 Section 5.1: クライアントからのフレームはマスクしなければならない
        // RFC 6455 Section 5.1, Section 7.4.1: 違反時は 1002 (protocol error) を使用してよい
        if !masked {
            return Err(Error::protocol_violation("unmasked client frame"));
        }
        Ok(())
    }

    fn encode_and_send(&mut self, frame: &Frame, shared: &mut SharedConnectionState) {
        // RFC 6455 Section 5.1: サーバーは送信フレームをマスクしてはならない
        // RFC 6455 Section 5.2: MASK=0 のとき Masking-Key field は存在しない
        let encoded = frame.encode_unmasked();
        shared.enqueue_output(ConnectionOutput::SendData(encoded));
    }
}
