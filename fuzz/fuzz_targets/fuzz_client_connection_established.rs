#![no_main]
use libfuzzer_sys::fuzz_target;
use shiguredo_websocket::{
    ClientConnectionOptions, RandomSource, Timestamp, WebSocketClientConnection,
};

struct FixedRandom;

impl RandomSource for FixedRandom {
    fn masking_key(&mut self) -> [u8; 4] {
        [0x12, 0x34, 0x56, 0x78]
    }
    fn nonce(&mut self) -> [u8; 16] {
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
    }
}

// RFC 6455 Section 4.2.2 / Section 1.3 に従って構築した最小の合法 101 レスポンス
// Sec-WebSocket-Accept = base64(SHA-1(base64(nonce) + GUID))
//   - base64(FixedRandom::nonce) = "AQIDBAUGBwgJCgsMDQ4PEA=="
//   - GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
//   - SHA-1 を base64 した結果が "C/0nmHhBztSRGR1CwL6Tf4ZjwpY="
// FixedRandom::nonce を変更した場合は Sec-WebSocket-Accept も再計算して同期させること
const HANDSHAKE_RESPONSE: &[u8] = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: C/0nmHhBztSRGR1CwL6Tf4ZjwpY=\r\n\r\n";

fuzz_target!(|data: &[u8]| {
    let options = ClientConnectionOptions::new("example.com", "/")
        .max_frame_size(64 * 1024)
        .max_message_size(64 * 1024)
        .max_decompressed_size(64 * 1024);
    let mut conn = WebSocketClientConnection::new(options, FixedRandom);

    let now = Timestamp::from_millis(0);

    // 接続開始（ハンドシェイクリクエストを生成）
    if conn.connect().is_err() {
        return;
    }

    // クライアントから送信される初期出力をドレインする
    while conn.poll_output().is_some() {}

    // 合法な 101 レスポンスを投入してハンドシェイクを完了させる
    if conn.feed_recv_buf(HANDSHAKE_RESPONSE, now).is_err() {
        return;
    }
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    // 任意バイトを Connected 状態のフレーム入力としてフィードする
    let _ = conn.feed_recv_buf(data, now);

    // イベントと出力をドレインする
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}
});
