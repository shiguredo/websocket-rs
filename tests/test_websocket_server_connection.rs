//! websocket_server_connection.rs の単体テスト

use shiguredo_websocket::{
    ConnectionState, ErrorKind, PerMessageDeflateConfig, ServerConnectionOptions,
    ServerHandshakeResponse, WebSocketServerConnection,
};

/// 符号付きの `server_max_window_bits` を含む offer には permessage-deflate で応答できない
#[test]
fn server_は符号付き_offer_への_permessage_deflate_応答を拒否する() {
    let request = "GET / HTTP/1.1\r\n\
                   Host: example.com\r\n\
                   Upgrade: websocket\r\n\
                   Connection: Upgrade\r\n\
                   Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                   Sec-WebSocket-Version: 13\r\n\
                   Sec-WebSocket-Extensions: permessage-deflate; server_max_window_bits=+8\r\n\
                   \r\n";
    let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
    conn.feed_recv_buf(request.as_bytes())
        .expect("ハンドシェイク要求の投入に失敗してはならない");

    let response = ServerHandshakeResponse::new().extension("permessage-deflate");
    let err = conn
        .accept_handshake(response)
        .expect_err("符号付きの値の offer に対する応答は拒否される");

    assert_eq!(err.kind, ErrorKind::HandshakeRejected);
    assert!(
        err.reason.contains("without a valid offer"),
        "不正な offer だけの応答は有効な offer 不在として拒否される: {}",
        err.reason
    );
}

/// 不正な値の offer を、client_max_window_bits の offer 根拠に使わない
#[test]
fn server_は不正な_offer_の_client_max_window_bits_を応答の根拠にしない() {
    let request = "GET / HTTP/1.1\r\n\
                   Host: example.com\r\n\
                   Upgrade: websocket\r\n\
                   Connection: Upgrade\r\n\
                   Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                   Sec-WebSocket-Version: 13\r\n\
                   Sec-WebSocket-Extensions: permessage-deflate; client_max_window_bits=+8, permessage-deflate\r\n\
                   \r\n";
    let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
    conn.feed_recv_buf(request.as_bytes())
        .expect("ハンドシェイク要求の投入に失敗してはならない");

    let response =
        ServerHandshakeResponse::new().extension("permessage-deflate; client_max_window_bits=15");
    let err = conn
        .accept_handshake(response)
        .expect_err("不正な offer だけが cmwb を持つ場合、応答の cmwb は拒否される");

    assert_eq!(err.kind, ErrorKind::HandshakeRejected);
    assert!(
        err.reason
            .contains("client_max_window_bits included without client offer"),
        "cmwb の offer 不在として拒否される: {}",
        err.reason
    );
}

/// 不正な値の offer を捨て、後続の有効な offer を根拠に応答を受理する
#[test]
fn server_は不正な_offer_を捨てて後続の_offer_で応答を受理する() {
    let request = "GET / HTTP/1.1\r\n\
                   Host: example.com\r\n\
                   Upgrade: websocket\r\n\
                   Connection: Upgrade\r\n\
                   Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                   Sec-WebSocket-Version: 13\r\n\
                   Sec-WebSocket-Extensions: permessage-deflate; server_max_window_bits=+8, permessage-deflate\r\n\
                   \r\n";
    let mut conn = WebSocketServerConnection::new(ServerConnectionOptions::new());
    conn.feed_recv_buf(request.as_bytes())
        .expect("ハンドシェイク要求の投入に失敗してはならない");

    let response =
        ServerHandshakeResponse::new().extension("permessage-deflate; server_max_window_bits=15");
    conn.accept_handshake(response)
        .expect("有効な offer に基づく permessage-deflate 応答は受理される");
}

/// 不正な値の offer は自動応答では拡張不採用になる
#[test]
fn server_は不正な_offer_を自動応答で不採用にする() {
    let request = "GET / HTTP/1.1\r\n\
                   Host: example.com\r\n\
                   Upgrade: websocket\r\n\
                   Connection: Upgrade\r\n\
                   Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                   Sec-WebSocket-Version: 13\r\n\
                   Sec-WebSocket-Extensions: permessage-deflate; server_max_window_bits=+15\r\n\
                   \r\n";
    let mut conn = WebSocketServerConnection::new(
        ServerConnectionOptions::new().deflate(PerMessageDeflateConfig::default()),
    );
    conn.feed_recv_buf(request.as_bytes())
        .expect("ハンドシェイク要求の投入に失敗してはならない");
    conn.accept_handshake_auto()
        .expect("不正な offer を不採用にして接続を確立する");

    assert_eq!(conn.state(), ConnectionState::Connected);
    assert!(
        conn.extensions().is_empty(),
        "不正な offer では permessage-deflate をネゴシエートしない"
    );
}
