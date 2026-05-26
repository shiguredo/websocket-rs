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
