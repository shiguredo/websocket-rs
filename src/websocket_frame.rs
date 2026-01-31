use crate::error::Error;
use crate::websocket_opcode::Opcode;

/// WebSocket フレーム (RFC 6455 Section 5.2)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// 最終フラグメントかどうか
    pub fin: bool,
    /// RSV1 ビット（permessage-deflate で使用）
    pub rsv1: bool,
    /// RSV2 ビット（予約）
    pub rsv2: bool,
    /// RSV3 ビット（予約）
    pub rsv3: bool,
    /// オペコード
    pub opcode: Opcode,
    /// ペイロード
    pub payload: Vec<u8>,
}

/// デコード済みフレーム情報
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    pub frame: Frame,
    pub masked: bool,
}

impl Frame {
    /// 新しいフレームを生成する
    pub fn new(opcode: Opcode, payload: Vec<u8>) -> Self {
        Self {
            fin: true,
            rsv1: false,
            rsv2: false,
            rsv3: false,
            opcode,
            payload,
        }
    }

    /// テキストフレームを生成する
    pub fn text(payload: &str) -> Self {
        Self::new(Opcode::Text, payload.as_bytes().to_vec())
    }

    /// バイナリフレームを生成する
    pub fn binary(payload: Vec<u8>) -> Self {
        Self::new(Opcode::Binary, payload)
    }

    /// Ping フレームを生成する
    ///
    /// RFC 6455 Section 5.5: コントロールフレームのペイロードは 125 バイト以下
    pub fn ping(payload: Vec<u8>) -> Result<Self, Error> {
        if payload.len() > 125 {
            return Err(Error::invalid_input(format!(
                "ping payload exceeds 125 bytes: {} bytes",
                payload.len()
            )));
        }
        Ok(Self::new(Opcode::Ping, payload))
    }

    /// Pong フレームを生成する
    ///
    /// RFC 6455 Section 5.5: コントロールフレームのペイロードは 125 バイト以下
    pub fn pong(payload: Vec<u8>) -> Result<Self, Error> {
        if payload.len() > 125 {
            return Err(Error::invalid_input(format!(
                "pong payload exceeds 125 bytes: {} bytes",
                payload.len()
            )));
        }
        Ok(Self::new(Opcode::Pong, payload))
    }

    /// Close フレームを生成する
    ///
    /// RFC 6455 Section 5.5: コントロールフレームのペイロードは 125 バイト以下
    /// Close フレームの場合、コード (2 バイト) + 理由で 125 バイト以下
    pub fn close(code: Option<u16>, reason: &str) -> Result<Self, Error> {
        let payload = match code {
            Some(c) => {
                // 理由は 123 バイト以下 (125 - 2 バイトのコード)
                if reason.len() > 123 {
                    return Err(Error::invalid_input(format!(
                        "close reason exceeds 123 bytes: {} bytes",
                        reason.len()
                    )));
                }
                let mut p = Vec::with_capacity(2 + reason.len());
                p.extend_from_slice(&c.to_be_bytes());
                p.extend_from_slice(reason.as_bytes());
                p
            }
            None => Vec::new(),
        };
        Ok(Self::new(Opcode::Close, payload))
    }

    /// フレームをエンコードする（クライアントはマスキング必須）
    pub fn encode(&self, masking_key: [u8; 4]) -> Vec<u8> {
        self.encode_internal(true, masking_key)
    }

    /// フレームをエンコードする（マスキングなし、サーバー用）
    #[allow(dead_code)]
    pub fn encode_unmasked(&self) -> Vec<u8> {
        self.encode_internal(false, [0; 4])
    }

    fn encode_internal(&self, masked: bool, masking_key: [u8; 4]) -> Vec<u8> {
        let payload_len = self.payload.len();

        // ヘッダーサイズを計算
        let header_size =
            2 + if payload_len >= 65536 {
                8
            } else if payload_len >= 126 {
                2
            } else {
                0
            } + if masked { 4 } else { 0 };

        let mut buf = Vec::with_capacity(header_size + payload_len);

        // 最初のバイト: FIN + RSV1-3 + Opcode
        let byte0 = (if self.fin { 0x80 } else { 0 })
            | (if self.rsv1 { 0x40 } else { 0 })
            | (if self.rsv2 { 0x20 } else { 0 })
            | (if self.rsv3 { 0x10 } else { 0 })
            | self.opcode.as_u8();
        buf.push(byte0);

        // 2 番目のバイト: MASK + Payload length
        let mask_bit = if masked { 0x80 } else { 0 };
        if payload_len >= 65536 {
            buf.push(mask_bit | 127);
            buf.extend_from_slice(&(payload_len as u64).to_be_bytes());
        } else if payload_len >= 126 {
            buf.push(mask_bit | 126);
            buf.extend_from_slice(&(payload_len as u16).to_be_bytes());
        } else {
            buf.push(mask_bit | payload_len as u8);
        }

        // マスキングキー
        if masked {
            buf.extend_from_slice(&masking_key);
        }

        // ペイロード（マスキング）
        if masked {
            for (i, byte) in self.payload.iter().enumerate() {
                buf.push(byte ^ masking_key[i % 4]);
            }
        } else {
            buf.extend_from_slice(&self.payload);
        }

        buf
    }
}

/// フレームデコーダー（Sans I/O パターン）
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    /// 新しいデコーダーを生成する
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// 受信データをバッファに追加する
    pub fn feed(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// フレームをデコードする（完全なフレームがあれば返す）
    pub fn decode(&mut self) -> Result<Option<Frame>, Error> {
        self.decode_with_info()
            .map(|opt| opt.map(|decoded| decoded.frame))
    }

    /// フレームをデコードし、マスク情報も返す
    pub fn decode_with_info(&mut self) -> Result<Option<DecodedFrame>, Error> {
        if self.buf.len() < 2 {
            return Ok(None);
        }

        let byte0 = self.buf[0];
        let byte1 = self.buf[1];

        let fin = (byte0 & 0x80) != 0;
        let rsv1 = (byte0 & 0x40) != 0;
        let rsv2 = (byte0 & 0x20) != 0;
        let rsv3 = (byte0 & 0x10) != 0;
        let opcode_value = byte0 & 0x0F;

        let opcode = Opcode::from_u8(opcode_value)
            .ok_or_else(|| Error::protocol_violation(format!("unknown opcode: {opcode_value}")))?;

        let masked = (byte1 & 0x80) != 0;
        let payload_len_7 = byte1 & 0x7F;

        // ペイロード長を決定
        let (payload_len, header_len): (usize, usize) = match payload_len_7 {
            127 => {
                if self.buf.len() < 10 {
                    return Ok(None);
                }
                // RFC 6455 Section 5.2: 最上位ビットは 0 でなければならない
                if self.buf[2] & 0x80 != 0 {
                    return Err(Error::protocol_violation(
                        "64-bit payload length MSB must be 0",
                    ));
                }
                let len = u64::from_be_bytes([
                    self.buf[2],
                    self.buf[3],
                    self.buf[4],
                    self.buf[5],
                    self.buf[6],
                    self.buf[7],
                    self.buf[8],
                    self.buf[9],
                ]);
                let len = usize::try_from(len)
                    .map_err(|_| Error::protocol_violation("payload length too large"))?;
                // RFC 6455 Section 5.2: 最小表現チェック
                // 64 ビット表現は 65535 より大きい場合のみ使用可能
                if len <= 65535 {
                    return Err(Error::protocol_violation(
                        "64-bit payload length must be > 65535 (non-minimal encoding)",
                    ));
                }
                (len, 10)
            }
            126 => {
                if self.buf.len() < 4 {
                    return Ok(None);
                }
                let len = u16::from_be_bytes([self.buf[2], self.buf[3]]) as usize;
                // RFC 6455 Section 5.2: 最小表現チェック
                // 16 ビット表現は 126 以上の場合のみ使用可能
                if len < 126 {
                    return Err(Error::protocol_violation(
                        "16-bit payload length must be >= 126 (non-minimal encoding)",
                    ));
                }
                (len, 4)
            }
            _ => (payload_len_7 as usize, 2),
        };

        let masking_key_len = if masked { 4 } else { 0 };
        let total_len = header_len
            .checked_add(masking_key_len)
            .and_then(|len| len.checked_add(payload_len))
            .ok_or_else(|| Error::protocol_violation("payload length too large"))?;

        if self.buf.len() < total_len {
            return Ok(None);
        }

        // マスキングキーを読み取る
        let masking_key = if masked {
            [
                self.buf[header_len],
                self.buf[header_len + 1],
                self.buf[header_len + 2],
                self.buf[header_len + 3],
            ]
        } else {
            [0; 4]
        };

        // ペイロードを読み取る
        let payload_start = header_len + masking_key_len;
        let mut payload = self.buf[payload_start..payload_start + payload_len].to_vec();

        // マスク解除
        if masked {
            for (i, byte) in payload.iter_mut().enumerate() {
                *byte ^= masking_key[i % 4];
            }
        }

        // コントロールフレームの検証
        if opcode.is_control() {
            if !fin {
                return Err(Error::protocol_violation(
                    "control frame must not be fragmented",
                ));
            }
            if payload_len > 125 {
                return Err(Error::protocol_violation("control frame payload too large"));
            }
        }

        // 処理済みデータを削除
        self.buf.drain(..total_len);

        Ok(Some(DecodedFrame {
            frame: Frame {
                fin,
                rsv1,
                rsv2,
                rsv3,
                opcode,
                payload,
            },
            masked,
        }))
    }

    /// バッファをクリアする
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// バッファの長さを取得する
    #[allow(dead_code)]
    pub fn buffer_len(&self) -> usize {
        self.buf.len()
    }
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_text() {
        let frame = Frame::text("Hello");
        let masking_key = [0x12, 0x34, 0x56, 0x78];
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        assert!(decoded.fin);
        assert_eq!(decoded.opcode, Opcode::Text);
        assert_eq!(decoded.payload, b"Hello");
    }

    #[test]
    fn test_encode_decode_binary() {
        let frame = Frame::binary(vec![0x01, 0x02, 0x03, 0x04]);
        let masking_key = [0xAA, 0xBB, 0xCC, 0xDD];
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        assert!(decoded.fin);
        assert_eq!(decoded.opcode, Opcode::Binary);
        assert_eq!(decoded.payload, vec![0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn test_encode_decode_ping_pong() {
        let ping = Frame::ping(vec![0x01, 0x02]).unwrap();
        let masking_key = [0x11, 0x22, 0x33, 0x44];
        let encoded = ping.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        assert!(decoded.fin);
        assert_eq!(decoded.opcode, Opcode::Ping);
        assert_eq!(decoded.payload, vec![0x01, 0x02]);
    }

    #[test]
    fn test_encode_decode_close() {
        let close = Frame::close(Some(1000), "goodbye").unwrap();
        let masking_key = [0x55, 0x66, 0x77, 0x88];
        let encoded = close.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        assert!(decoded.fin);
        assert_eq!(decoded.opcode, Opcode::Close);
        assert_eq!(&decoded.payload[0..2], &[0x03, 0xE8]); // 1000 in big-endian
        assert_eq!(&decoded.payload[2..], b"goodbye");
    }

    #[test]
    fn test_large_payload() {
        // 126 バイト以上のペイロード
        let payload = vec![0xAB; 1000];
        let frame = Frame::binary(payload.clone());
        let masking_key = [0x12, 0x34, 0x56, 0x78];
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn test_very_large_payload() {
        // 65536 バイト以上のペイロード
        let payload = vec![0xCD; 70000];
        let frame = Frame::binary(payload.clone());
        let masking_key = [0x98, 0x76, 0x54, 0x32];
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();
        decoder.feed(&encoded);
        let decoded = decoder.decode().unwrap().unwrap();

        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn test_partial_frame() {
        let frame = Frame::text("Hello");
        let masking_key = [0x12, 0x34, 0x56, 0x78];
        let encoded = frame.encode(masking_key);

        let mut decoder = FrameDecoder::new();

        // 1 バイトずつ送信
        for byte in &encoded[..encoded.len() - 1] {
            decoder.feed(&[*byte]);
            assert!(decoder.decode().unwrap().is_none());
        }

        // 最後のバイトを送信
        decoder.feed(&[encoded[encoded.len() - 1]]);
        let decoded = decoder.decode().unwrap().unwrap();
        assert_eq!(decoded.payload, b"Hello");
    }

    // === RFC 6455 準拠テスト ===

    #[test]
    fn test_ping_payload_limit_125_bytes() {
        // 125 バイトは OK
        let payload = vec![0x42; 125];
        assert!(Frame::ping(payload).is_ok());

        // 126 バイトは NG
        let payload = vec![0x42; 126];
        assert!(Frame::ping(payload).is_err());
    }

    #[test]
    fn test_pong_payload_limit_125_bytes() {
        // 125 バイトは OK
        let payload = vec![0x42; 125];
        assert!(Frame::pong(payload).is_ok());

        // 126 バイトは NG
        let payload = vec![0x42; 126];
        assert!(Frame::pong(payload).is_err());
    }

    #[test]
    fn test_close_reason_limit_123_bytes() {
        // 123 バイトは OK
        let reason = "a".repeat(123);
        assert!(Frame::close(Some(1000), &reason).is_ok());

        // 124 バイトは NG
        let reason = "a".repeat(124);
        assert!(Frame::close(Some(1000), &reason).is_err());
    }

    #[test]
    fn test_reject_non_minimal_16bit_length() {
        // 16 ビット表現で 125 バイト以下を表現するのは不正
        // 手動で不正なフレームを構築: mask=0, length=126 (16-bit), actual=100
        let mut invalid_frame = vec![
            0x82, // FIN + Binary
            0x7E, // 126 = 16-bit length follows
            0x00, 0x64, // 100 in big-endian (should use 7-bit encoding)
        ];
        invalid_frame.extend(vec![0x00; 100]); // 100 bytes payload

        let mut decoder = FrameDecoder::new();
        decoder.feed(&invalid_frame);
        let result = decoder.decode();
        assert!(result.is_err());
    }

    #[test]
    fn test_reject_non_minimal_64bit_length() {
        // 64 ビット表現で 65535 バイト以下を表現するのは不正
        // 手動で不正なフレームを構築: mask=0, length=127 (64-bit), actual=1000
        let mut invalid_frame = vec![
            0x82, // FIN + Binary
            0x7F, // 127 = 64-bit length follows
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0xE8, // 1000 in big-endian
        ];
        invalid_frame.extend(vec![0x00; 1000]); // 1000 bytes payload

        let mut decoder = FrameDecoder::new();
        decoder.feed(&invalid_frame);
        let result = decoder.decode();
        assert!(result.is_err());
    }

    #[test]
    fn test_accept_minimal_16bit_length() {
        // 16 ビット表現で 126 バイトは正当
        let mut valid_frame = vec![
            0x82, // FIN + Binary
            0x7E, // 126 = 16-bit length follows
            0x00, 0x7E, // 126 in big-endian
        ];
        valid_frame.extend(vec![0x00; 126]); // 126 bytes payload

        let mut decoder = FrameDecoder::new();
        decoder.feed(&valid_frame);
        let result = decoder.decode().unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().payload.len(), 126);
    }

    #[test]
    fn test_accept_minimal_64bit_length() {
        // 64 ビット表現で 65536 バイトは正当
        let mut valid_frame = vec![
            0x82, // FIN + Binary
            0x7F, // 127 = 64-bit length follows
            0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, // 65536 in big-endian
        ];
        valid_frame.extend(vec![0x00; 65536]); // 65536 bytes payload

        let mut decoder = FrameDecoder::new();
        decoder.feed(&valid_frame);
        let result = decoder.decode().unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().payload.len(), 65536);
    }
}
