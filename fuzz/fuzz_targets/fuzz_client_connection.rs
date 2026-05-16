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

fuzz_target!(|data: &[u8]| {
    let options = ClientConnectionOptions::new("example.com", "/")
        .max_frame_size(64 * 1024)
        .max_message_size(64 * 1024)
        .max_decompressed_size(64 * 1024);
    let mut conn = WebSocketClientConnection::new(options, FixedRandom);

    // 接続開始（ハンドシェイクリクエストを生成）
    if conn.connect().is_err() {
        return;
    }

    // 初期出力をドレインする
    while conn.poll_output().is_some() {}

    // 任意バイトをサーバーレスポンスとしてフィードする
    let now = Timestamp::from_millis(0);
    let _ = conn.feed_recv_buf(data, now);

    // イベントと出力をドレインする
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}
});
