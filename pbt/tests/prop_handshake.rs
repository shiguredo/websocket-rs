//! WebSocket ハンドシェイクのプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - ハンドシェイクリクエスト/レスポンスの検証ロジック
//! - エラーケースのハンドリング
//! - エッジケースの検出

use base64::Engine;
use proptest::prelude::*;
use shiguredo_websocket::{
    Extension, HandshakeRequest, HandshakeRequestValidator, HandshakeValidator,
    PerMessageDeflateConfig, ServerHandshakeResponse,
};

proptest! {
    /// Extension のパース・エンコードラウンドトリップ
    #[test]
    fn prop_extension_roundtrip(
        name in "[a-z][a-z0-9-]{0,20}",
        param_names in prop::collection::vec("[a-z][a-z0-9_]{0,10}", 0..5),
        param_values in prop::collection::vec(prop::option::of("[a-zA-Z0-9]{1,20}"), 0..5)
    ) {
        let mut ext = Extension::new(&name);
        for (pname, pvalue) in param_names.iter().zip(param_values.iter()) {
            ext = ext.param(pname, pvalue.as_deref());
        }

        let encoded = ext.encode();
        let parsed = Extension::parse(&encoded);

        prop_assert_eq!(parsed.len(), 1);
        prop_assert_eq!(&parsed[0].name, &name);
    }

    /// 複数の Extension のパース
    #[test]
    fn prop_multiple_extensions(
        names in prop::collection::vec("[a-z][a-z0-9-]{0,10}", 1..5)
    ) {
        let extensions: Vec<Extension> = names.iter()
            .map(|n| Extension::new(n))
            .collect();

        let encoded = extensions.iter()
            .map(|e| e.encode())
            .collect::<Vec<_>>()
            .join(", ");

        let parsed = Extension::parse(&encoded);
        prop_assert_eq!(parsed.len(), names.len());

        for (parsed_ext, original_name) in parsed.iter().zip(names.iter()) {
            prop_assert_eq!(parsed_ext.name.as_str(), original_name.as_str());
        }
    }

    /// PerMessageDeflateConfig のラウンドトリップ
    ///
    /// to_extension() は設定に応じて no_context_takeover を含めるため、
    /// ラウンドトリップ後も設定が保持される。
    #[test]
    fn prop_permessage_deflate_roundtrip(
        server_bits in prop::option::of(8u8..=15),
        client_bits in prop::option::of(8u8..=15),
        server_no_takeover in any::<bool>(),
        client_no_takeover in any::<bool>()
    ) {
        let mut config = PerMessageDeflateConfig::new();

        if let Some(bits) = server_bits {
            config = config.server_max_window_bits(bits);
        }
        if let Some(bits) = client_bits {
            config = config.client_max_window_bits(bits);
        }
        if server_no_takeover {
            config = config.server_no_context_takeover();
        }
        if client_no_takeover {
            config = config.client_no_context_takeover();
        }

        let ext = config.to_extension();
        let parsed = PerMessageDeflateConfig::from_extension(&ext).unwrap();

        // ラウンドトリップ後は設定が保持される
        prop_assert_eq!(parsed.server_no_context_takeover, server_no_takeover);
        prop_assert_eq!(parsed.client_no_context_takeover, client_no_takeover);

        // window bits は clamp されるので、元の値と比較
        if let Some(bits) = server_bits {
            prop_assert_eq!(parsed.server_max_window_bits, Some(bits.clamp(8, 15)));
        }
        if let Some(bits) = client_bits {
            prop_assert_eq!(parsed.client_max_window_bits, Some(bits.clamp(8, 15)));
        }
    }

    /// nonce のフォーマット
    #[test]
    fn prop_nonce_format(nonce in any::<[u8; 16]>()) {
        let request = HandshakeRequest::new("/", "example.com");
        let encoded = request.build(nonce);
        let s = String::from_utf8(encoded).unwrap();

        // 必須ヘッダーが含まれる
        prop_assert!(s.contains("GET / HTTP/1.1"));
        prop_assert!(s.contains("Host: example.com"));
        prop_assert!(s.contains("Upgrade: websocket"));
        prop_assert!(s.contains("Connection: Upgrade"));
        prop_assert!(s.contains("Sec-WebSocket-Key:"));
        prop_assert!(s.contains("Sec-WebSocket-Version: 13"));
    }
}

// =============================================================================
// HandshakeRequest ビルダーのテスト
// =============================================================================

proptest! {
    /// HandshakeRequest のビルダーパターン
    #[test]
    fn prop_handshake_request_builder(
        path in "/[a-zA-Z0-9/_-]{0,50}",
        host in "[a-z]{3,10}\\.[a-z]{2,4}",
        nonce in any::<[u8; 16]>()
    ) {
        let request = HandshakeRequest::new(&path, &host);
        let encoded = request.build(nonce);
        let s = String::from_utf8(encoded).unwrap();

        let expected_get = format!("GET {} HTTP/1.1", path);
        let expected_host = format!("Host: {}", host);
        prop_assert!(s.contains(&expected_get));
        prop_assert!(s.contains(&expected_host));
    }

    /// HandshakeRequest に origin を設定
    #[test]
    fn prop_handshake_request_with_origin(
        origin in "https://[a-z]{3,10}\\.[a-z]{2,4}",
        nonce in any::<[u8; 16]>()
    ) {
        let request = HandshakeRequest::new("/", "example.com").origin(&origin);
        let encoded = request.build(nonce);
        let s = String::from_utf8(encoded).unwrap();

        let expected_origin = format!("Origin: {}", origin);
        prop_assert!(s.contains(&expected_origin));
    }

    /// HandshakeRequest に protocol を設定
    #[test]
    fn prop_handshake_request_with_protocol(
        protocol in "[a-z]{3,15}",
        nonce in any::<[u8; 16]>()
    ) {
        let request = HandshakeRequest::new("/", "example.com").protocol(&protocol);
        let encoded = request.build(nonce);
        let s = String::from_utf8(encoded).unwrap();

        let expected_protocol = format!("Sec-WebSocket-Protocol: {}", protocol);
        prop_assert!(s.contains(&expected_protocol));
    }

    /// HandshakeRequest に複数の protocol を設定
    #[test]
    fn prop_handshake_request_with_multiple_protocols(
        protocols in prop::collection::vec("[a-z]{3,10}", 2..4),
        nonce in any::<[u8; 16]>()
    ) {
        let mut request = HandshakeRequest::new("/", "example.com");
        for p in &protocols {
            request = request.protocol(p);
        }
        let encoded = request.build(nonce);
        let s = String::from_utf8(encoded).unwrap();

        let expected_protocols = format!("Sec-WebSocket-Protocol: {}", protocols.join(", "));
        prop_assert!(s.contains(&expected_protocols));
    }

    /// HandshakeRequest に extension を設定
    #[test]
    fn prop_handshake_request_with_extension(
        extension in "[a-z][a-z0-9-]{3,20}",
        nonce in any::<[u8; 16]>()
    ) {
        let request = HandshakeRequest::new("/", "example.com").extension(&extension);
        let encoded = request.build(nonce);
        let s = String::from_utf8(encoded).unwrap();

        let expected_extension = format!("Sec-WebSocket-Extensions: {}", extension);
        prop_assert!(s.contains(&expected_extension));
    }

    /// HandshakeRequest に追加ヘッダーを設定
    #[test]
    fn prop_handshake_request_with_header(
        header_name in "[A-Z][a-zA-Z-]{3,15}",
        header_value in "[a-zA-Z0-9 ]{1,30}",
        nonce in any::<[u8; 16]>()
    ) {
        let request = HandshakeRequest::new("/", "example.com")
            .header(&header_name, &header_value);
        let encoded = request.build(nonce);
        let s = String::from_utf8(encoded).unwrap();

        let expected_header = format!("{}: {}", header_name, header_value);
        prop_assert!(s.contains(&expected_header));
    }
}

// =============================================================================
// ServerHandshakeResponse ビルダーのテスト
// =============================================================================

proptest! {
    /// ServerHandshakeResponse のビルダーパターン
    #[test]
    fn prop_server_response_builder(
        protocol in "[a-z]{3,15}",
        extension in "[a-z]{3,15}",
        header_name in "[A-Z][a-zA-Z-]{3,15}",
        header_value in "[a-zA-Z0-9 ]{1,30}"
    ) {
        let response = ServerHandshakeResponse::new()
            .protocol(&protocol)
            .extension(&extension)
            .header(&header_name, &header_value);

        prop_assert_eq!(response.protocol, Some(protocol));
        prop_assert_eq!(response.extensions, vec![extension]);
        prop_assert_eq!(response.additional_headers.len(), 1);
        prop_assert_eq!(&response.additional_headers[0].0, &header_name);
        prop_assert_eq!(&response.additional_headers[0].1, &header_value);
    }
}

// =============================================================================
// HandshakeRequestValidator のテスト
// =============================================================================

/// 有効な WebSocket キーを生成
fn generate_valid_ws_key() -> String {
    base64::engine::general_purpose::STANDARD.encode(b"0123456789ABCDEF")
}

proptest! {
    /// 有効なハンドシェイクリクエストが正しくパースされる
    #[test]
    fn prop_valid_handshake_request_validation(
        path in "/[a-zA-Z0-9/_-]{0,30}",
        host in "[a-z]{3,10}\\.[a-z]{2,4}"
    ) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET {} HTTP/1.1\r\n\
             Host: {}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            path, host, key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_ok());
        let req = result.unwrap().unwrap();
        prop_assert_eq!(req.path, path);
        prop_assert_eq!(req.host, host);
    }

    /// 不正なメソッドは拒否される
    #[test]
    fn prop_invalid_method_rejected(
        method in "(POST|PUT|DELETE|PATCH)"
    ) {
        let key = generate_valid_ws_key();
        let request = format!(
            "{} / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            method, key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// 不正な HTTP バージョンは拒否される
    #[test]
    fn prop_invalid_http_version_rejected(
        version in "(HTTP/1.0|HTTP/2.0|HTTP/0.9)"
    ) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / {}\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            version, key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// 不正な Upgrade ヘッダーは拒否される
    #[test]
    fn prop_invalid_upgrade_header_rejected(
        upgrade_value in "(http|ftp|ssh|invalid)"
    ) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: {}\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            upgrade_value, key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// 不正な Connection ヘッダーは拒否される
    #[test]
    fn prop_invalid_connection_header_rejected(
        conn_value in "(Close|Keep-Alive|invalid)"
    ) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: {}\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            conn_value, key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// 不正な Sec-WebSocket-Version は拒否される
    #[test]
    fn prop_invalid_websocket_version_rejected(
        version in "(8|9|10|11|12|14)"
    ) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: {}\r\n\
             \r\n",
            key, version
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// 不正な Sec-WebSocket-Key は拒否される
    #[test]
    fn prop_invalid_websocket_key_rejected(
        invalid_key in "[a-zA-Z0-9]{1,10}"
    ) {
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            invalid_key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }
}

// =============================================================================
// HandshakeValidator (クライアント側) のテスト
// =============================================================================

/// 正しい Sec-WebSocket-Accept 値を計算
fn calculate_expected_accept(nonce: &[u8; 16]) -> String {
    use sha1::{Digest, Sha1};
    const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

    let key = base64::engine::general_purpose::STANDARD.encode(nonce);
    let combined = format!("{}{}", key, WEBSOCKET_GUID);

    let mut hasher = Sha1::new();
    hasher.update(combined.as_bytes());
    let hash = hasher.finalize();

    base64::engine::general_purpose::STANDARD.encode(hash)
}

proptest! {
    /// 有効なハンドシェイクレスポンスが正しくパースされる
    #[test]
    fn prop_valid_handshake_response_validation(
        nonce in any::<[u8; 16]>()
    ) {
        let accept = calculate_expected_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             \r\n",
            accept
        );

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_ok());
        let resp = result.unwrap().unwrap();
        prop_assert!(resp.protocol.is_none());
        prop_assert!(resp.extensions.is_empty());
    }

    /// プロトコル付きのレスポンスが正しくパースされる
    #[test]
    fn prop_handshake_response_with_protocol(
        nonce in any::<[u8; 16]>(),
        protocol in "[a-z]{3,15}"
    ) {
        let accept = calculate_expected_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             Sec-WebSocket-Protocol: {}\r\n\
             \r\n",
            accept, protocol
        );

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_ok());
        let resp = result.unwrap().unwrap();
        prop_assert_eq!(resp.protocol, Some(protocol));
    }

    /// 不正なステータスコードは拒否される
    #[test]
    fn prop_invalid_status_code_rejected(
        nonce in any::<[u8; 16]>(),
        status in prop::sample::select(vec![200, 301, 400, 404, 500])
    ) {
        let accept = calculate_expected_accept(&nonce);
        let response = format!(
            "HTTP/1.1 {} OK\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             \r\n",
            status, accept
        );

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// 不正な Sec-WebSocket-Accept は拒否される
    #[test]
    fn prop_invalid_accept_rejected(
        nonce in any::<[u8; 16]>(),
        invalid_accept in "[a-zA-Z0-9+/]{20,30}="
    ) {
        let correct_accept = calculate_expected_accept(&nonce);

        // 不正な accept が正しい accept と異なる場合のみテスト
        if invalid_accept != correct_accept {
            let response = format!(
                "HTTP/1.1 101 Switching Protocols\r\n\
                 Upgrade: websocket\r\n\
                 Connection: Upgrade\r\n\
                 Sec-WebSocket-Accept: {}\r\n\
                 \r\n",
                invalid_accept
            );

            let mut validator = HandshakeValidator::new(nonce);
            validator.feed(response.as_bytes());
            let result = validator.validate();

            prop_assert!(result.is_err());
        }
    }
}

proptest! {
    // =============================================================================
    // 欠損ヘッダーのテスト
    // =============================================================================

    /// Host ヘッダーが欠損している場合は拒否される
    #[test]
    fn prop_missing_host_header_rejected(_dummy in 0u8..1) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// Upgrade ヘッダーが欠損している場合は拒否される
    #[test]
    fn prop_missing_upgrade_header_rejected(_dummy in 0u8..1) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// Connection ヘッダーが欠損している場合は拒否される
    #[test]
    fn prop_missing_connection_header_rejected(_dummy in 0u8..1) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// Sec-WebSocket-Key ヘッダーが欠損している場合は拒否される
    #[test]
    fn prop_missing_websocket_key_rejected(_dummy in 0u8..1) {
        let request = "\
GET / HTTP/1.1\r\n\
Host: example.com\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Version: 13\r\n\
\r\n";

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// Sec-WebSocket-Version ヘッダーが欠損している場合は拒否される
    #[test]
    fn prop_missing_websocket_version_rejected(_dummy in 0u8..1) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             \r\n",
            key
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    // =============================================================================
    // HandshakeValidator の欠損ヘッダーテスト
    // =============================================================================

    /// レスポンスで Upgrade ヘッダーが欠損している場合は拒否される
    #[test]
    fn prop_response_missing_upgrade_rejected(nonce in any::<[u8; 16]>()) {
        let accept = calculate_expected_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             \r\n",
            accept
        );

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// レスポンスで Connection ヘッダーが欠損している場合は拒否される
    #[test]
    fn prop_response_missing_connection_rejected(nonce in any::<[u8; 16]>()) {
        let accept = calculate_expected_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             \r\n",
            accept
        );

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }

    /// レスポンスで Sec-WebSocket-Accept ヘッダーが欠損している場合は拒否される
    #[test]
    fn prop_response_missing_accept_rejected(nonce in any::<[u8; 16]>()) {
        let response = "\
HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
\r\n";

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        prop_assert!(result.is_err());
    }
}

// =============================================================================
// チャンク送信のテスト
// =============================================================================

proptest! {
    /// ハンドシェイクリクエストをチャンクで送っても正しくパースされる
    #[test]
    fn prop_chunked_handshake_request(
        chunk_size in 1usize..20
    ) {
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n",
            key
        );

        let mut validator = HandshakeRequestValidator::new();
        for chunk in request.as_bytes().chunks(chunk_size) {
            validator.feed(chunk);
        }
        let result = validator.validate();

        prop_assert!(result.is_ok());
    }

    /// ハンドシェイクレスポンスをチャンクで送っても正しくパースされる
    #[test]
    fn prop_chunked_handshake_response(
        nonce in any::<[u8; 16]>(),
        chunk_size in 1usize..20
    ) {
        let accept = calculate_expected_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             \r\n",
            accept
        );

        let mut validator = HandshakeValidator::new(nonce);
        for chunk in response.as_bytes().chunks(chunk_size) {
            validator.feed(chunk);
        }
        let result = validator.validate();

        prop_assert!(result.is_ok());
    }
}
