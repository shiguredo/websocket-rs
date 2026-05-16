#![no_main]
use libfuzzer_sys::fuzz_target;
use shiguredo_websocket::{ServerConnectionOptions, WebSocketServerConnection};

fuzz_target!(|data: &[u8]| {
    let options = ServerConnectionOptions::new()
        .max_frame_size(64 * 1024)
        .max_message_size(64 * 1024)
        .max_decompressed_size(64 * 1024);
    let mut conn = WebSocketServerConnection::new(options);

    // 任意バイトをフィードする（ハンドシェイク + フレーム処理パス）
    if conn.feed_recv_buf(data).is_err() {
        return;
    }

    // ハンドシェイクリクエストがパースできた場合は自動受諾する
    if conn.handshake_request().is_some() {
        let _ = conn.accept_handshake_auto();
    }

    // イベントと出力をドレインする
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}
});
