//! websocket_server_connection.rs の単体テスト

use shiguredo_websocket::{
    ConnectionOutput, ConnectionState, ErrorKind, PerMessageDeflateConfig, ServerConnectionOptions,
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

/// Sec-WebSocket-Extensions を指定したハンドシェイク要求を生成する
fn handshake_request(extensions: &str) -> Vec<u8> {
    format!(
        "GET / HTTP/1.1\r\n\
         Host: example.com\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Extensions: {}\r\n\
         \r\n",
        extensions
    )
    .into_bytes()
}

/// 送信キューから Sec-WebSocket-Extensions ヘッダーを取り出す
fn response_extensions(conn: &mut WebSocketServerConnection) -> String {
    while let Some(output) = conn.poll_output() {
        if let ConnectionOutput::SendData(data) = output {
            let text = String::from_utf8_lossy(&data);
            for line in text.lines() {
                if let Some(value) = line.strip_prefix("Sec-WebSocket-Extensions: ") {
                    return value.to_string();
                }
            }
        }
    }
    String::new()
}

/// deflate を有効にしたサーバーに、指定した offer の要求を投入する
fn setup_server_with_offers(extensions: &str) -> WebSocketServerConnection {
    let mut conn = WebSocketServerConnection::new(
        ServerConnectionOptions::new().deflate(PerMessageDeflateConfig::default()),
    );
    conn.feed_recv_buf(&handshake_request(extensions))
        .expect("ハンドシェイク要求の投入に失敗してはならない");
    conn
}

/// 8 の offer をスキップし、後続の 15 の offer で自動応答できる
#[test]
fn auto_handshake_selects_a_later_supported_offer() {
    let mut conn = setup_server_with_offers(
        "permessage-deflate; server_max_window_bits=8, \
         permessage-deflate; server_max_window_bits=15",
    );
    conn.accept_handshake_auto()
        .expect("後続の有効な offer で応答できる");
    assert_eq!(conn.state(), ConnectionState::Connected);
    let extensions = response_extensions(&mut conn);
    assert!(
        extensions.contains("server_max_window_bits=15"),
        "応答が選択した offer の値と一致しない: {extensions}"
    );
}

/// 後続の offer に一致する手動応答は受理される
#[test]
fn manual_handshake_accepts_matching_response_for_later_offer() {
    let mut conn = setup_server_with_offers(
        "permessage-deflate; server_max_window_bits=8, \
         permessage-deflate; server_max_window_bits=15",
    );
    let response =
        ServerHandshakeResponse::new().extension("permessage-deflate; server_max_window_bits=15");
    conn.accept_handshake(response)
        .expect("後続の offer に一致する応答は受理される");
    assert_eq!(conn.state(), ConnectionState::Connected);
}

/// どの単一 offer も満たさない応答は拒否される
#[test]
fn response_with_client_max_window_bits_rejects_mixed_offers() {
    let mut conn = setup_server_with_offers(
        "permessage-deflate; server_max_window_bits=8; client_max_window_bits, \
         permessage-deflate; server_max_window_bits=15",
    );
    let response = ServerHandshakeResponse::new()
        .extension("permessage-deflate; server_max_window_bits=15; client_max_window_bits=15");

    let err = conn
        .accept_handshake(response)
        .expect_err("どの単一 offer も満たさない応答は拒否される");
    assert_eq!(err.kind, ErrorKind::HandshakeRejected);
    assert!(
        err.reason.contains("does not match any offer"),
        "拒否理由: {}",
        err.reason
    );
}

/// 単一の 15 の offer には 15 の応答で接続できる
#[test]
fn single_offer_15_accepts_response_with_15() {
    let mut conn = setup_server_with_offers("permessage-deflate; server_max_window_bits=15");
    let response =
        ServerHandshakeResponse::new().extension("permessage-deflate; server_max_window_bits=15");
    conn.accept_handshake(response)
        .expect("単一の 15 の offer には 15 の応答で接続できる");
    assert_eq!(conn.state(), ConnectionState::Connected);
    assert!(
        response_extensions(&mut conn).contains("server_max_window_bits=15"),
        "応答に server_max_window_bits=15 が含まれる"
    );
}

/// cmwb を含む offer には自動応答で cmwb=15 を返して接続できる
#[test]
fn auto_handshake_accepts_offer_with_client_max_window_bits() {
    let mut conn = setup_server_with_offers("permessage-deflate; client_max_window_bits");
    conn.accept_handshake_auto()
        .expect("cmwb を含む offer で応答できる");
    assert_eq!(conn.state(), ConnectionState::Connected);
    assert_eq!(
        response_extensions(&mut conn),
        "permessage-deflate; client_max_window_bits=15"
    );
}

/// cmwb を含む offer には cmwb なしの手動応答でも接続できる
#[test]
fn manual_handshake_accepts_offer_with_client_max_window_bits() {
    let mut conn = setup_server_with_offers("permessage-deflate; client_max_window_bits");
    let response = ServerHandshakeResponse::new().extension("permessage-deflate");
    conn.accept_handshake(response)
        .expect("cmwb なしの応答でも接続できる");
    assert_eq!(conn.state(), ConnectionState::Connected);
}

/// 応答の server_max_window_bits=8 はサポート外として拒否される
#[test]
fn response_with_unsupported_server_max_window_bits_is_rejected() {
    let mut conn = setup_server_with_offers("permessage-deflate; server_max_window_bits=15");
    let response =
        ServerHandshakeResponse::new().extension("permessage-deflate; server_max_window_bits=8");

    let err = conn
        .accept_handshake(response)
        .expect_err("8 はサポート外として拒否される");
    assert_eq!(err.kind, ErrorKind::HandshakeRejected);
    assert!(
        err.reason.contains("only 15 is supported"),
        "拒否理由: {}",
        err.reason
    );
}

/// 単一の 8 の offer に 15 の手動応答を返すと拒否される
#[test]
fn single_offer_8_rejects_response_with_15() {
    let mut conn = setup_server_with_offers("permessage-deflate; server_max_window_bits=8");
    let response =
        ServerHandshakeResponse::new().extension("permessage-deflate; server_max_window_bits=15");

    let err = conn
        .accept_handshake(response)
        .expect_err("offer の実効値より大きい server_max_window_bits は拒否される");
    assert_eq!(err.kind, ErrorKind::HandshakeRejected);
    assert!(
        err.reason.contains("does not match any offer"),
        "拒否理由: {}",
        err.reason
    );
}

/// 単一の 8 の offer に smwb なしの応答 (実効値 15) を返すと拒否される
#[test]
fn single_offer_8_rejects_response_without_smwb() {
    let mut conn = setup_server_with_offers("permessage-deflate; server_max_window_bits=8");
    let response = ServerHandshakeResponse::new().extension("permessage-deflate");

    let err = conn
        .accept_handshake(response)
        .expect_err("smwb なしの応答は実効値 15 として拒否される");
    assert_eq!(err.kind, ErrorKind::HandshakeRejected);
}

/// 単一の 8 の offer では拡張を採用せず接続する
#[test]
fn single_offer_8_is_not_negotiated() {
    let mut conn = setup_server_with_offers("permessage-deflate; server_max_window_bits=8");
    conn.accept_handshake_auto()
        .expect("8 の offer は不採用のまま接続できる");
    assert_eq!(conn.state(), ConnectionState::Connected);
    assert!(
        response_extensions(&mut conn).is_empty(),
        "permessage-deflate を応答に含めない"
    );
}

/// smwb なしの offer には smwb なしの応答で接続できる
#[test]
fn single_offer_without_smwb_accepts_response_without_smwb() {
    let mut conn = setup_server_with_offers("permessage-deflate");
    let response = ServerHandshakeResponse::new().extension("permessage-deflate");
    conn.accept_handshake(response)
        .expect("smwb なしの offer には smwb なしの応答で接続できる");
    assert_eq!(conn.state(), ConnectionState::Connected);
    assert_eq!(response_extensions(&mut conn), "permessage-deflate");
}
