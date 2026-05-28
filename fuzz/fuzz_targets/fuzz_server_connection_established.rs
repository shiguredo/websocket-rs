#![no_main]
use libfuzzer_sys::fuzz_target;
use shiguredo_websocket::{ServerConnectionOptions, WebSocketServerConnection};

// RFC 6455 Section 4.1 / Section 4.2.1 に従って構築した最小の合法ハンドシェイクリクエスト
// RFC 6455 Section 4.1 では Sec-WebSocket-Key は各接続でランダム選択された 16 バイトの
// base64 が MUST だが、fuzz ターゲットではサーバ側がランダム性を検証できないため固定値を使う。
// Origin (Section 4.1 項目 8) は非ブラウザクライアントの場合 MAY のため省略する。
const HANDSHAKE_REQUEST: &[u8] = b"GET / HTTP/1.1\r\nHost: example.com\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: AQIDBAUGBwgJCgsMDQ4PEA==\r\nSec-WebSocket-Version: 13\r\n\r\n";

fuzz_target!(|data: &[u8]| {
    let options = ServerConnectionOptions::new()
        .max_frame_size(64 * 1024)
        .max_message_size(64 * 1024)
        .max_decompressed_size(64 * 1024);
    let mut conn = WebSocketServerConnection::new(options);

    // 合法なハンドシェイクリクエストを投入する
    if conn.feed_recv_buf(HANDSHAKE_REQUEST).is_err() {
        return;
    }

    // ハンドシェイクリクエストが受理されたら自動受諾する
    if conn.handshake_request().is_none() {
        return;
    }
    if conn.accept_handshake_auto().is_err() {
        return;
    }

    // サーバーから送信される初期出力をドレインする
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}

    // 任意バイトを Connected 状態のフレーム入力としてフィードする
    let _ = conn.feed_recv_buf(data);

    // イベントと出力をドレインする
    while conn.poll_event().is_some() {}
    while conn.poll_output().is_some() {}
});
