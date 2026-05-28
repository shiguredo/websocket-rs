//! クライアント / サーバー間で共有されるフレーム処理ロジック
//!
//! `SharedConnectionState` はフレームのデコード・エンコード・フラグメント管理・
//! クローズハンドシェイク・タイマー処理など、WebSocket 接続の共通ロジックを集約する。
//! マスキングの有無など client/server で異なる振る舞いは `crate::frame_policy::FramePolicy`
//! トレイト経由で抽象化する。フラグメント収集バッファは `crate::fragment_buffer` に分離。

use std::collections::VecDeque;

use crate::deflate::PerMessageDeflate;
use crate::error::Error;
use crate::fragment_buffer::FragmentBuffer;
use crate::frame_policy::FramePolicy;
use crate::websocket_close::{CloseCode, truncate_reason};
use crate::websocket_connection_types::{
    ConnectionEvent, ConnectionOutput, ConnectionState, TimerId,
};
use crate::websocket_frame::{DecodedFrame, Frame, FrameDecoder};
use crate::websocket_opcode::Opcode;

/// デフォルトの最大フレームサイズ（64MB）
pub const DEFAULT_MAX_FRAME_SIZE: usize = 64 * 1024 * 1024;

/// デフォルトの最大メッセージサイズ（64MB）
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;

/// デフォルトの最大解凍サイズ（16MB）
pub const DEFAULT_MAX_DECOMPRESSED_SIZE: usize = 16 * 1024 * 1024;

/// クライアント / サーバー間で共有される接続状態
///
/// `state` の書き込みは `set_state` に限定して不正な遷移を防ぐ。
pub(crate) struct SharedConnectionState {
    state: ConnectionState,
    close_sent: bool,
    close_received: bool,
    awaiting_pong: bool,
    failed: bool,
    event_queue: VecDeque<ConnectionEvent>,
    output_queue: VecDeque<ConnectionOutput>,
    frame_decoder: FrameDecoder,
    fragment_buffer: FragmentBuffer,
    deflate: Option<PerMessageDeflate>,
    max_frame_size: usize,
    max_message_size: usize,
    max_decompressed_size: usize,
    ping_interval_millis: u64,
    pong_timeout_millis: u64,
    close_timeout_millis: u64,
}

impl SharedConnectionState {
    pub(crate) fn new(
        max_frame_size: usize,
        max_message_size: usize,
        max_decompressed_size: usize,
        ping_interval_millis: u64,
        pong_timeout_millis: u64,
        close_timeout_millis: u64,
    ) -> Self {
        Self {
            state: ConnectionState::Disconnected,
            close_sent: false,
            close_received: false,
            awaiting_pong: false,
            failed: false,
            event_queue: VecDeque::new(),
            output_queue: VecDeque::new(),
            frame_decoder: FrameDecoder::new(),
            fragment_buffer: FragmentBuffer::new(),
            deflate: None,
            max_frame_size,
            max_message_size,
            max_decompressed_size,
            ping_interval_millis,
            pong_timeout_millis,
            close_timeout_millis,
        }
    }

    // === 共通メソッド ===

    pub(crate) fn state(&self) -> ConnectionState {
        self.state
    }

    /// 致命的エラーが発生済みかを返す。RFC 6455 Section 7.1.7 の
    /// Fail the WebSocket Connection ラッチを表す
    pub(crate) fn is_failed(&self) -> bool {
        self.failed
    }

    /// 致命的エラー発生を記録する。以降の feed_recv_buf は即時 Err となる
    pub(crate) fn mark_failed(&mut self) {
        self.failed = true;
    }

    /// permessage-deflate を有効化する。ハンドシェイクで合意が成立した
    /// 直後に 1 回だけ呼び出される想定
    pub(crate) fn enable_deflate(&mut self, deflate: PerMessageDeflate) {
        self.deflate = Some(deflate);
    }

    pub(crate) fn emit_event(&mut self, event: ConnectionEvent) {
        self.event_queue.push_back(event);
    }

    pub(crate) fn enqueue_output(&mut self, output: ConnectionOutput) {
        self.output_queue.push_back(output);
    }

    /// `state` への書き込みは本メソッドに集約する。差分時のみ
    /// `StateChanged` イベントを emit して二重通知を避ける。
    /// 許可されていない遷移は `Error::invalid_state` を返す。
    /// 許可遷移表は `ConnectionState` の doc コメントを参照
    pub(crate) fn set_state(&mut self, new_state: ConnectionState) -> Result<(), Error> {
        if self.state == new_state {
            return Ok(());
        }
        if !self.state.can_transition_to(new_state) {
            return Err(Error::invalid_state(format!(
                "invalid state transition from {:?} to {:?}",
                self.state, new_state
            )));
        }
        self.state = new_state;
        self.event_queue
            .push_back(ConnectionEvent::StateChanged(new_state));
        Ok(())
    }

    pub(crate) fn check_connected(&self) -> Result<(), Error> {
        if self.state != ConnectionState::Connected {
            return Err(Error::invalid_state("not connected"));
        }
        Ok(())
    }

    /// 公開 API 用の検証付きクローズ
    ///
    /// 状態・送信可能コード・reason 長を検証し、いずれかが不正なら `Err` を返す。
    /// 成功時は `close_internal` に委譲する。
    ///
    /// RFC 6455 Section 7.1.2: Close フレームは established connection 上でのみ送信可能
    /// RFC 6455 Section 7.4.1: 送信禁止のクローズコード (1005, 1006, 1015) は拒否される
    /// RFC 6455 Section 5.5: reason は 123 バイト以下でなければならない
    pub(crate) fn close(
        &mut self,
        code: CloseCode,
        reason: &str,
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        if !matches!(
            self.state,
            ConnectionState::Connected | ConnectionState::Closing
        ) {
            return Err(Error::invalid_state("connection is not established"));
        }

        // RFC 6455 Section 7.4.1: 送信禁止のクローズコードをチェック
        if !code.is_sendable() {
            return Err(Error::invalid_input(format!(
                "close code {} is not sendable",
                code.as_u16()
            )));
        }

        // reason が 123 バイト超の場合は public API では呼び出し元にエラーとして通知する。
        // close_internal は truncate_reason で切り詰めるが、公開 API では拒否する。
        if reason.len() > 123 {
            return Err(Error::invalid_input(format!(
                "close reason exceeds 123 bytes: {} bytes",
                reason.len()
            )));
        }

        self.close_internal(code, reason, policy);
        Ok(())
    }

    /// 内部エラー処理用のクローズ
    ///
    /// 理由が長すぎる場合は UTF-8 文字境界で切り詰める
    pub(crate) fn close_internal(
        &mut self,
        code: CloseCode,
        reason: &str,
        policy: &mut impl FramePolicy,
    ) {
        // RFC 6455 Section 7.1.2: Close フレーム送信は確立済み接続のみ。
        // Connecting / Disconnected / Closed では Close フレームを送らず終了する
        if !matches!(
            self.state,
            ConnectionState::Connected | ConnectionState::Closing
        ) {
            return;
        }

        if !self.close_sent {
            // RFC 6455 Section 5.5 / 5.5.1: コントロールフレームのペイロードは 125 バイト以下、
            // Close フレームは先頭 2 バイトが status code のため reason は 123 バイト以下
            // truncate_reason 後は reason が常に 123 バイト以下だが、
            // Frame::close の将来的なエラー条件追加に備えて unwrap_or_else を維持する。
            let truncated = truncate_reason(reason, 123);
            let frame = Frame::close(Some(code.as_u16()), truncated).unwrap_or_else(|_| {
                Frame::close(Some(code.as_u16()), "")
                    .expect("empty reason close frame must always succeed")
            });
            policy.encode_and_send(&frame, self);
            self.close_sent = true;

            // クローズタイムアウト設定
            self.output_queue.push_back(ConnectionOutput::SetTimer {
                id: TimerId::CloseTimeout,
                duration_millis: self.close_timeout_millis,
            });

            // close_internal の冒頭で Connected / Closing 以外を弾いているため、
            // ここでの遷移元は Connected または Closing に限定される。
            // Closing → Closing は set_state 内で同一状態として早期 Ok 返却される
            self.set_state(ConnectionState::Closing)
                .expect("unreachable: Connected/Closing -> Closing must be valid");
        }
    }

    /// テキストメッセージを送信する公開 API 用ヘルパ
    pub(crate) fn send_text(
        &mut self,
        text: &str,
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        self.check_connected()?;
        self.send_data_frame(Opcode::Text, text.as_bytes().to_vec(), policy)
    }

    /// バイナリメッセージを送信する公開 API 用ヘルパ
    pub(crate) fn send_binary(
        &mut self,
        data: &[u8],
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        self.check_connected()?;
        self.send_data_frame(Opcode::Binary, data.to_vec(), policy)
    }

    /// Ping を送信する公開 API 用ヘルパ
    ///
    /// RFC 6455 Section 5.5: data は 125 バイト以下でなければならない
    pub(crate) fn send_ping(
        &mut self,
        data: &[u8],
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        self.check_connected()?;
        self.send_ping_internal(data, policy)
    }

    /// データフレームを送信（圧縮対応）
    pub(crate) fn send_data_frame(
        &mut self,
        opcode: Opcode,
        payload: Vec<u8>,
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        let (payload, compressed) = self.compress_if_enabled(payload)?;
        let mut frame = Frame::new(opcode, payload);
        frame.rsv1 = compressed;
        policy.encode_and_send(&frame, self);
        Ok(())
    }

    /// 圧縮が有効な場合、ペイロードを圧縮する
    pub(crate) fn compress_if_enabled(
        &mut self,
        payload: Vec<u8>,
    ) -> Result<(Vec<u8>, bool), Error> {
        if let Some(deflate) = &mut self.deflate {
            // 小さなメッセージは圧縮しない（圧縮のオーバーヘッドが大きくなる可能性）
            const COMPRESSION_THRESHOLD: usize = 64;
            if deflate.should_compress(&payload, COMPRESSION_THRESHOLD) {
                let compressed = deflate.compress(&payload)?;
                Ok((compressed, true))
            } else {
                Ok((payload, false))
            }
        } else {
            Ok((payload, false))
        }
    }

    /// 必要に応じてペイロードを解凍する
    pub(crate) fn decompress_if_needed(
        &mut self,
        payload: Vec<u8>,
        compressed: bool,
        policy: &mut impl FramePolicy,
    ) -> Result<Vec<u8>, Error> {
        if compressed {
            if let Some(deflate) = &mut self.deflate {
                deflate.decompress(&payload, self.max_decompressed_size)
            } else {
                self.close_internal(
                    CloseCode::PROTOCOL_ERROR,
                    "received compressed frame without permessage-deflate",
                    policy,
                );
                Err(Error::protocol_violation(
                    "received compressed frame without permessage-deflate",
                ))
            }
        } else {
            Ok(payload)
        }
    }

    /// 受信バッファからフレームをデコードして処理する
    pub(crate) fn process_frames(
        &mut self,
        buf: &[u8],
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        self.frame_decoder.feed(buf);

        loop {
            match self.frame_decoder.decode_with_info() {
                Ok(Some(decoded)) => {
                    self.handle_decoded_frame(decoded, policy)?;
                }
                Ok(None) => break,
                Err(e) => {
                    // RFC 6455 Section 7.1.7: 接続確立後のプロトコル違反では
                    // Close フレームを送信してから接続を終了する
                    self.close_internal(CloseCode::PROTOCOL_ERROR, "frame decode error", policy);
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    /// デコード済みフレームのマスク検証とフレーム処理を行う
    pub(crate) fn handle_decoded_frame(
        &mut self,
        decoded: DecodedFrame,
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        if let Err(e) = policy.verify_frame_masking(decoded.masked) {
            self.close_internal(CloseCode::PROTOCOL_ERROR, &e.to_string(), policy);
            return Err(e);
        }
        self.handle_frame(decoded.frame, policy)
    }

    /// フレームの種別に応じてハンドラに振り分ける
    pub(crate) fn handle_frame(
        &mut self,
        frame: Frame,
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        // フレームサイズチェック（コントロールフレームは RFC 6455 で 125 バイト以下が保証済み）
        if !frame.opcode.is_control() && frame.payload.len() > self.max_frame_size {
            self.close_internal(
                CloseCode::MESSAGE_TOO_BIG,
                "frame payload too large",
                policy,
            );
            return Err(Error::protocol_violation("frame payload too large"));
        }
        // RSV ビットチェック（permessage-deflate 以外は禁止）
        if frame.rsv2 || frame.rsv3 {
            self.close_internal(CloseCode::PROTOCOL_ERROR, "reserved bits set", policy);
            return Err(Error::protocol_violation("reserved bits set"));
        }
        // RFC 7692 Section 6: RSV1 検証
        if frame.rsv1 {
            if self.deflate.is_none() {
                self.close_internal(
                    CloseCode::PROTOCOL_ERROR,
                    "rsv1 set without permessage-deflate",
                    policy,
                );
                return Err(Error::protocol_violation(
                    "rsv1 set without permessage-deflate",
                ));
            }
            // コントロールフレームでは RSV1=0 必須
            if frame.opcode.is_control() {
                self.close_internal(
                    CloseCode::PROTOCOL_ERROR,
                    "rsv1 must not be set on control frames",
                    policy,
                );
                return Err(Error::protocol_violation(
                    "rsv1 must not be set on control frames",
                ));
            }
            // 継続フレームでは RSV1=0 必須
            if frame.opcode == Opcode::Continuation {
                self.close_internal(
                    CloseCode::PROTOCOL_ERROR,
                    "rsv1 must not be set on continuation frames",
                    policy,
                );
                return Err(Error::protocol_violation(
                    "rsv1 must not be set on continuation frames",
                ));
            }
        }

        match frame.opcode {
            Opcode::Continuation => self.handle_continuation(frame, policy)?,
            Opcode::Text | Opcode::Binary => self.handle_data_frame(frame, policy)?,
            Opcode::Close => self.handle_close(frame, policy)?,
            Opcode::Ping => self.handle_ping(frame, policy)?,
            Opcode::Pong => self.handle_pong(frame)?,
        }

        Ok(())
    }

    /// データフレーム（Text / Binary）の処理
    pub(crate) fn handle_data_frame(
        &mut self,
        frame: Frame,
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        // RFC 6455 Section 5.4: フラグメント中に新しいデータフレームは禁止
        if !self.fragment_buffer.is_empty() {
            self.close_internal(
                CloseCode::PROTOCOL_ERROR,
                "new message started before previous completed",
                policy,
            );
            return Err(Error::protocol_violation(
                "new message started before previous completed",
            ));
        }

        if frame.fin {
            // 完全なメッセージ
            let payload = self.decompress_if_needed(frame.payload, frame.rsv1, policy)?;
            self.emit_message(frame.opcode, payload, policy)?;
        } else {
            // フラグメント開始 (RSV1 は最初のフレームにのみ設定される)
            if frame.payload.len() > self.max_message_size {
                self.close_internal(CloseCode::MESSAGE_TOO_BIG, "message too large", policy);
                return Err(Error::protocol_violation("message too large"));
            }
            self.fragment_buffer
                .start(frame.opcode, frame.payload, frame.rsv1);
        }
        Ok(())
    }

    /// 継続フレームの処理
    pub(crate) fn handle_continuation(
        &mut self,
        frame: Frame,
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        if self.fragment_buffer.is_empty() {
            self.close_internal(
                CloseCode::PROTOCOL_ERROR,
                "continuation frame without initial frame",
                policy,
            );
            return Err(Error::protocol_violation(
                "continuation frame without initial frame",
            ));
        }

        self.fragment_buffer.append(&frame.payload);

        // フラグメント累積サイズチェック
        if self.fragment_buffer.len() > self.max_message_size {
            self.close_internal(CloseCode::MESSAGE_TOO_BIG, "message too large", policy);
            return Err(Error::protocol_violation("message too large"));
        }

        if frame.fin {
            let (opcode, payload, compressed) = self.fragment_buffer.take();
            let payload = self.decompress_if_needed(payload, compressed, policy)?;
            self.emit_message(opcode, payload, policy)?;
        }

        Ok(())
    }

    /// メッセージの種別に応じてイベントキューに追加する
    pub(crate) fn emit_message(
        &mut self,
        opcode: Opcode,
        payload: Vec<u8>,
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        match opcode {
            Opcode::Text => match String::from_utf8(payload) {
                Ok(text) => {
                    self.event_queue
                        .push_back(ConnectionEvent::TextMessage(text));
                }
                Err(e) => {
                    // RFC 6455 Section 8.1: UTF-8 検証失敗時は接続を失敗させる
                    self.event_queue.push_back(ConnectionEvent::Error(format!(
                        "invalid UTF-8 in text message: {}",
                        e
                    )));
                    self.close_internal(CloseCode::INVALID_PAYLOAD, "invalid UTF-8", policy);
                    return Err(Error::protocol_violation("invalid UTF-8 in text message"));
                }
            },
            Opcode::Binary => {
                self.event_queue
                    .push_back(ConnectionEvent::BinaryMessage(payload));
            }
            _ => {}
        }
        Ok(())
    }

    /// Close フレームの処理
    pub(crate) fn handle_close(
        &mut self,
        frame: Frame,
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        self.close_received = true;

        // RFC 6455 Section 5.5.1: ペイロード長は 0 または 2 以上でなければならない
        if frame.payload.len() == 1 {
            self.close_internal(
                CloseCode::PROTOCOL_ERROR,
                "close frame payload length must be 0 or >= 2",
                policy,
            );
            return Err(Error::protocol_violation(
                "close frame payload length must be 0 or >= 2",
            ));
        }

        let (code, reason) = if frame.payload.len() >= 2 {
            let code_val = u16::from_be_bytes([frame.payload[0], frame.payload[1]]);
            let close_code = CloseCode::new(code_val);

            // RFC 6455 Section 7.4.1: クローズコードの妥当性検証
            if !close_code.is_valid() {
                self.close_internal(
                    CloseCode::PROTOCOL_ERROR,
                    &format!("invalid close code: {}", code_val),
                    policy,
                );
                return Err(Error::protocol_violation(format!(
                    "invalid close code: {}",
                    code_val
                )));
            }

            // RFC 6455 Section 5.5.1: 理由は有効な UTF-8 でなければならない
            let reason = match String::from_utf8(frame.payload[2..].to_vec()) {
                Ok(r) => r,
                Err(_) => {
                    self.close_internal(
                        CloseCode::PROTOCOL_ERROR,
                        "close frame reason is not valid UTF-8",
                        policy,
                    );
                    return Err(Error::protocol_violation(
                        "close frame reason is not valid UTF-8",
                    ));
                }
            };

            (Some(close_code), reason)
        } else {
            (None, String::new())
        };

        self.event_queue
            .push_back(ConnectionEvent::Close { code, reason });

        if !self.close_sent {
            // クローズフレームを返送
            // 送信禁止コードの場合は 1000 (Normal Closure) を使用
            let reply_code = code
                .filter(|c| c.is_sendable())
                .map(|c| c.as_u16())
                .unwrap_or(1000);
            let reply_frame = Frame::close(Some(reply_code), "")
                .expect("empty reason close reply frame must always succeed");
            policy.encode_and_send(&reply_frame, self);
            self.close_sent = true;
        }

        // 両方向でクローズが完了
        // Ping/Pong 関連の状態とタイマーをクリア
        self.awaiting_pong = false;
        self.output_queue.push_back(ConnectionOutput::ClearTimer {
            id: TimerId::PongTimeout,
        });
        self.output_queue
            .push_back(ConnectionOutput::ClearTimer { id: TimerId::Ping });
        self.output_queue.push_back(ConnectionOutput::ClearTimer {
            id: TimerId::CloseTimeout,
        });
        // handle_close は process_frames 経由で呼ばれ、caller の feed_recv_buf が
        // Connected または Closing でのみ process_frames に到達させる。
        // 通常は Ok を返すが、前提崩れに備えて `?` で伝播する
        self.set_state(ConnectionState::Closed)?;
        self.output_queue
            .push_back(ConnectionOutput::CloseConnection);

        // クリーンアップ
        self.frame_decoder.clear();
        self.fragment_buffer.clear();

        Ok(())
    }

    /// Ping フレームの処理
    pub(crate) fn handle_ping(
        &mut self,
        frame: Frame,
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        // Ping イベントを発行
        self.event_queue
            .push_back(ConnectionEvent::Ping(frame.payload.clone()));

        // RFC 6455 Section 5.5.2: Close を受信済みなら Pong を送らない
        if !self.close_received {
            // Pong を自動返信（受信した Ping のペイロードをそのまま返すので 125 バイト以下は保証される）
            let pong = Frame::pong(frame.payload)?;
            policy.encode_and_send(&pong, self);
        }

        Ok(())
    }

    /// Pong フレームの処理
    pub(crate) fn handle_pong(&mut self, frame: Frame) -> Result<(), Error> {
        self.awaiting_pong = false;

        // Pong タイムアウトをクリア
        self.output_queue.push_back(ConnectionOutput::ClearTimer {
            id: TimerId::PongTimeout,
        });

        // Pong イベントを発行
        self.event_queue
            .push_back(ConnectionEvent::Pong(frame.payload));

        Ok(())
    }

    /// タイマーイベントを処理する
    pub(crate) fn handle_timer(
        &mut self,
        timer_id: TimerId,
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        match timer_id {
            TimerId::Ping => {
                if self.state == ConnectionState::Connected && !self.awaiting_pong {
                    self.send_ping_internal(&[], policy)?;
                }
                // 次の Ping タイマー設定（Connected 状態の場合のみ）
                if self.state == ConnectionState::Connected && self.ping_interval_millis > 0 {
                    self.output_queue.push_back(ConnectionOutput::SetTimer {
                        id: TimerId::Ping,
                        duration_millis: self.ping_interval_millis,
                    });
                }
            }
            TimerId::PongTimeout => {
                if self.awaiting_pong {
                    // Pong タイムアウト - 接続を閉じる
                    self.event_queue
                        .push_back(ConnectionEvent::Error("pong timeout".to_string()));
                    self.close_internal(CloseCode::POLICY_VIOLATION, "pong timeout", policy);
                }
            }
            TimerId::CloseTimeout => {
                if self.state == ConnectionState::Closing {
                    // クローズタイムアウト - 強制切断。直前の if で Closing を確認済み
                    self.set_state(ConnectionState::Closed)?;
                    self.output_queue
                        .push_back(ConnectionOutput::CloseConnection);
                }
            }
        }
        Ok(())
    }

    /// Ping フレームを送信し、awaiting_pong フラグと PongTimeout タイマーを設定する
    pub(crate) fn send_ping_internal(
        &mut self,
        data: &[u8],
        policy: &mut impl FramePolicy,
    ) -> Result<(), Error> {
        let frame = Frame::ping(data.to_vec())?;
        policy.encode_and_send(&frame, self);
        self.awaiting_pong = true;
        self.output_queue.push_back(ConnectionOutput::SetTimer {
            id: TimerId::PongTimeout,
            duration_millis: self.pong_timeout_millis,
        });
        Ok(())
    }

    /// イベントを取得する
    pub(crate) fn poll_event(&mut self) -> Option<ConnectionEvent> {
        self.event_queue.pop_front()
    }

    /// 出力を取得する
    pub(crate) fn poll_output(&mut self) -> Option<ConnectionOutput> {
        self.output_queue.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    fn new_shared() -> SharedConnectionState {
        SharedConnectionState::new(
            DEFAULT_MAX_FRAME_SIZE,
            DEFAULT_MAX_MESSAGE_SIZE,
            DEFAULT_MAX_DECOMPRESSED_SIZE,
            0,
            0,
            0,
        )
    }

    #[test]
    fn set_state_は許可遷移を成功させ_state_changed_を_emit_する() {
        let mut shared = new_shared();
        assert!(matches!(
            shared.set_state(ConnectionState::Connecting),
            Ok(())
        ));
        assert_eq!(shared.state(), ConnectionState::Connecting);
        let event = shared
            .poll_event()
            .expect("StateChanged event must be emitted");
        assert_eq!(
            event,
            ConnectionEvent::StateChanged(ConnectionState::Connecting)
        );
    }

    #[test]
    fn set_state_は同一状態への遷移を_no_op_として_ok_を返す() {
        let mut shared = new_shared();
        // Disconnected -> Disconnected は no-op
        assert!(matches!(
            shared.set_state(ConnectionState::Disconnected),
            Ok(())
        ));
        assert!(shared.poll_event().is_none());
    }

    #[test]
    fn set_state_は不正遷移を_invalid_state_で拒否し_state_を変えない() {
        let mut shared = new_shared();
        // Disconnected -> Connected は許可遷移表外
        let err = shared
            .set_state(ConnectionState::Connected)
            .expect_err("invalid transition must return Err");
        assert_eq!(err.kind, ErrorKind::InvalidState);
        assert!(err.reason.contains("Disconnected"));
        assert!(err.reason.contains("Connected"));
        assert_eq!(shared.state(), ConnectionState::Disconnected);
        assert!(shared.poll_event().is_none());
    }

    #[test]
    fn set_state_は終端状態からの遷移を拒否する() {
        let mut shared = new_shared();
        // Disconnected -> Connecting -> Connected -> Closing -> Closed
        shared
            .set_state(ConnectionState::Connecting)
            .expect("Disconnected -> Connecting");
        shared
            .set_state(ConnectionState::Connected)
            .expect("Connecting -> Connected");
        shared
            .set_state(ConnectionState::Closing)
            .expect("Connected -> Closing");
        shared
            .set_state(ConnectionState::Closed)
            .expect("Closing -> Closed");
        // Closed から他状態への遷移は全て拒否される
        for next in [
            ConnectionState::Disconnected,
            ConnectionState::Connecting,
            ConnectionState::Connected,
            ConnectionState::Closing,
        ] {
            let err = shared
                .set_state(next)
                .expect_err("transition from Closed must be rejected");
            assert_eq!(err.kind, ErrorKind::InvalidState);
        }
        assert_eq!(shared.state(), ConnectionState::Closed);
    }
}
