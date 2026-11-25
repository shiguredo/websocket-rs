//! WebSocket ハンドシェイクのプロパティベーステスト
//!
//! 目的: バグの洗い出し
//! - ハンドシェイクリクエスト/レスポンスの検証ロジック
//! - エラーケースのハンドリング
//! - エッジケースの検出

use std::cell::Cell;

use base64ct::{Base64, Encoding};
use shiguredo_websocket::{
    Extension, HandshakeRequest, HandshakeRequestValidator, HandshakeValidator,
    PerMessageDeflateConfig, ServerHandshakeResponse,
};

/// 小文字。`[a-z]`。
const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";

/// 大文字。`[A-Z]`。
const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// 英字。`[a-zA-Z]`。
const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// 英数字。`[a-zA-Z0-9]`。
const ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// 英数字とスペース。`[a-zA-Z0-9 ]`。
const ALNUM_SPACE: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ";

/// 小文字、数字、ハイフン。`[a-z0-9-]` の末尾ハイフンはリテラル。
const LOWER_DIGIT_HYPHEN: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789-";

/// 小文字、数字、アンダースコア。`[a-z0-9_]`。
const LOWER_DIGIT_UNDERSCORE: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_";

/// パス断片の文字。`[a-zA-Z0-9/_-]` の末尾ハイフンはリテラル。
const PATH_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/_-";

/// ヘッダー名の 2 文字目以降。`[a-zA-Z-]` の末尾ハイフンはリテラル。
const HEADER_NAME_REST: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ-";

/// Base64 の文字。`[a-zA-Z0-9+/]`。
const BASE64_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+/";

/// token に使える文字。ハイフンは末尾のリテラル。
const TOKEN_CHARS: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!#$%&'*+.^_`|~-";

/// 受理しない HTTP メソッド。
const INVALID_METHODS: [&str; 4] = ["POST", "PUT", "DELETE", "PATCH"];

/// 受理しない HTTP バージョン。
const INVALID_HTTP_VERSIONS: [&str; 3] = ["HTTP/1.0", "HTTP/2.0", "HTTP/0.9"];

/// 受理しない Upgrade 値。
const INVALID_UPGRADE_VALUES: [&str; 4] = ["http", "ftp", "ssh", "invalid"];

/// 受理しない Connection 値。
const INVALID_CONNECTION_VALUES: [&str; 3] = ["Close", "Keep-Alive", "invalid"];

/// 受理しない Sec-WebSocket-Version。
const INVALID_WS_VERSIONS: [&str; 6] = ["8", "9", "10", "11", "12", "14"];

/// 101 以外のステータスコード。
const INVALID_STATUS_CODES: [u16; 5] = [200, 301, 400, 404, 500];

/// token に含まれない区切り文字。カンマとスペースは含めない。
const INVALID_PROTOCOL_CHARS: [char; 16] = [
    '(', ')', '<', '>', '@', ';', ':', '\\', '"', '/', '[', ']', '?', '=', '{', '}',
];

/// クライアントが重複して送ってはならない予約済みヘッダー。
const RESERVED_HEADERS: [&str; 7] = [
    "Host",
    "Upgrade",
    "Connection",
    "Sec-WebSocket-Key",
    "Sec-WebSocket-Version",
    "Sec-WebSocket-Protocol",
    "Sec-WebSocket-Extensions",
];

/// Extension のパース・エンコードラウンドトリップ
#[test]
fn prop_extension_roundtrip() -> noprop::TestResult {
    pbt::run(|ctx| {
        let name = pbt::sample_prefixed(ctx, LOWER, LOWER_DIGIT_HYPHEN, 0, 20);
        let param_names = pbt::sample_vec(ctx, 0, 5, |ctx| {
            pbt::sample_prefixed(ctx, LOWER, LOWER_DIGIT_UNDERSCORE, 0, 10)
        });
        let param_values = pbt::sample_vec(ctx, 0, 5, |ctx| {
            if noprop::sample_bool(ctx) {
                Some(pbt::sample_alphabet(ctx, ALNUM, 1, 20))
            } else {
                None
            }
        });

        let mut ext = Extension::new(&name);
        for (pname, pvalue) in param_names.iter().zip(param_values.iter()) {
            ext = ext.param(pname, pvalue.as_deref());
        }

        let encoded = ext.encode();
        let parsed = Extension::parse(&encoded);

        assert_eq!(parsed.len(), 1);
        assert_eq!(&parsed[0].name, &name);
        Ok(())
    })?;
    Ok(())
}

/// 複数の Extension のパース
#[test]
fn prop_multiple_extensions() -> noprop::TestResult {
    pbt::run(|ctx| {
        let names = pbt::sample_vec(ctx, 1, 5, |ctx| {
            pbt::sample_prefixed(ctx, LOWER, LOWER_DIGIT_HYPHEN, 0, 10)
        });

        let extensions: Vec<Extension> = names.iter().map(|n| Extension::new(n)).collect();

        let encoded = extensions
            .iter()
            .map(|e| e.encode())
            .collect::<Vec<_>>()
            .join(", ");

        let parsed = Extension::parse(&encoded);
        assert_eq!(parsed.len(), names.len());

        for (parsed_ext, original_name) in parsed.iter().zip(names.iter()) {
            assert_eq!(parsed_ext.name.as_str(), original_name.as_str());
        }
        Ok(())
    })?;
    Ok(())
}

/// PerMessageDeflateConfig のラウンドトリップ
///
/// to_extension() は設定に応じて no_context_takeover を含めるため、
/// ラウンドトリップ後も設定が保持される。
#[test]
fn prop_permessage_deflate_roundtrip() -> noprop::TestResult {
    // window bits の表明は Some のときだけ走る。None と Some は等確率。
    // 256 ケースで Some が一度も出ない確率は 2 の 256 乗分の 1 で、server と client は独立。
    let server_window_checked = Cell::new(0usize);
    let client_window_checked = Cell::new(0usize);
    let runner = pbt::run(|ctx| {
        let server_bits = if noprop::sample_bool(ctx) {
            Some(noprop::sample_u8_in(ctx, 8..=15))
        } else {
            None
        };
        let client_bits = if noprop::sample_bool(ctx) {
            Some(noprop::sample_u8_in(ctx, 8..=15))
        } else {
            None
        };
        let server_no_takeover = noprop::sample_bool(ctx);
        let client_no_takeover = noprop::sample_bool(ctx);

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
        // サーバーリクエストとしてパース（クライアントからの要求）
        let parsed = PerMessageDeflateConfig::from_extension_for_server_request(&ext)
            .expect("拡張設定のパースに失敗してはならない");

        // ラウンドトリップ後は設定が保持される
        assert_eq!(parsed.server_no_context_takeover, server_no_takeover);
        assert_eq!(parsed.client_no_context_takeover, client_no_takeover);

        // window bits は clamp されるので、元の値と比較
        if let Some(bits) = server_bits {
            assert_eq!(parsed.server_max_window_bits, Some(bits.clamp(8, 15)));
            server_window_checked.set(server_window_checked.get() + 1);
        }
        if let Some(bits) = client_bits {
            assert_eq!(parsed.client_max_window_bits, Some(bits.clamp(8, 15)));
            client_window_checked.set(client_window_checked.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        server_window_checked.get() > 0,
        "server_max_window_bits が Some のときを検証していない\n{runner}"
    );
    assert!(
        client_window_checked.get() > 0,
        "client_max_window_bits が Some のときを検証していない\n{runner}"
    );
    Ok(())
}

/// nonce のフォーマット
#[test]
fn prop_nonce_format() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);

        let request = HandshakeRequest::new("/", "example.com");
        let encoded = request
            .build(nonce)
            .expect("ハンドシェイクリクエストの構築に失敗してはならない");
        let s = String::from_utf8(encoded).expect("UTF-8 への変換に失敗してはならない");

        // 必須ヘッダーが含まれる
        assert!(s.contains("GET / HTTP/1.1"));
        assert!(s.contains("Host: example.com"));
        assert!(s.contains("Upgrade: websocket"));
        assert!(s.contains("Connection: Upgrade"));
        assert!(s.contains("Sec-WebSocket-Key:"));
        assert!(s.contains("Sec-WebSocket-Version: 13"));
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// HandshakeRequest ビルダーのテスト
// =============================================================================

/// HandshakeRequest のビルダーパターン
#[test]
fn prop_handshake_request_builder() -> noprop::TestResult {
    pbt::run(|ctx| {
        let path_rest = pbt::sample_alphabet(ctx, PATH_CHARS, 0, 50);
        let path = format!("/{path_rest}");
        let host_label = pbt::sample_alphabet(ctx, LOWER, 3, 10);
        let host_tld = pbt::sample_alphabet(ctx, LOWER, 2, 4);
        let host = format!("{host_label}.{host_tld}");
        let nonce = noprop::sample_bytes::<16>(ctx);

        let request = HandshakeRequest::new(&path, &host);
        let encoded = request
            .build(nonce)
            .expect("ハンドシェイクリクエストの構築に失敗してはならない");
        let s = String::from_utf8(encoded).expect("UTF-8 への変換に失敗してはならない");

        let expected_get = format!("GET {} HTTP/1.1", path);
        let expected_host = format!("Host: {}", host);
        assert!(s.contains(&expected_get));
        assert!(s.contains(&expected_host));
        Ok(())
    })?;
    Ok(())
}

/// HandshakeRequest に origin を設定
#[test]
fn prop_handshake_request_with_origin() -> noprop::TestResult {
    pbt::run(|ctx| {
        let origin_label = pbt::sample_alphabet(ctx, LOWER, 3, 10);
        let origin_tld = pbt::sample_alphabet(ctx, LOWER, 2, 4);
        let origin = format!("https://{origin_label}.{origin_tld}");
        let nonce = noprop::sample_bytes::<16>(ctx);

        let request = HandshakeRequest::new("/", "example.com").origin(&origin);
        let encoded = request
            .build(nonce)
            .expect("ハンドシェイクリクエストの構築に失敗してはならない");
        let s = String::from_utf8(encoded).expect("UTF-8 への変換に失敗してはならない");

        let expected_origin = format!("Origin: {}", origin);
        assert!(s.contains(&expected_origin));
        Ok(())
    })?;
    Ok(())
}

/// HandshakeRequest に protocol を設定
#[test]
fn prop_handshake_request_with_protocol() -> noprop::TestResult {
    pbt::run(|ctx| {
        let protocol = pbt::sample_alphabet(ctx, LOWER, 3, 15);
        let nonce = noprop::sample_bytes::<16>(ctx);

        let request = HandshakeRequest::new("/", "example.com").protocol(&protocol);
        let encoded = request
            .build(nonce)
            .expect("ハンドシェイクリクエストの構築に失敗してはならない");
        let s = String::from_utf8(encoded).expect("UTF-8 への変換に失敗してはならない");

        let expected_protocol = format!("Sec-WebSocket-Protocol: {}", protocol);
        assert!(s.contains(&expected_protocol));
        Ok(())
    })?;
    Ok(())
}

/// HandshakeRequest に複数の protocol を設定
#[test]
fn prop_handshake_request_with_multiple_protocols() -> noprop::TestResult {
    pbt::run(|ctx| {
        let protocols = pbt::sample_vec(ctx, 2, 4, |ctx| pbt::sample_alphabet(ctx, LOWER, 3, 10));
        let nonce = noprop::sample_bytes::<16>(ctx);

        let mut request = HandshakeRequest::new("/", "example.com");
        for p in &protocols {
            request = request.protocol(p);
        }
        let encoded = request
            .build(nonce)
            .expect("ハンドシェイクリクエストの構築に失敗してはならない");
        let s = String::from_utf8(encoded).expect("UTF-8 への変換に失敗してはならない");

        let expected_protocols = format!("Sec-WebSocket-Protocol: {}", protocols.join(", "));
        assert!(s.contains(&expected_protocols));
        Ok(())
    })?;
    Ok(())
}

/// HandshakeRequest に extension を設定
#[test]
fn prop_handshake_request_with_extension() -> noprop::TestResult {
    pbt::run(|ctx| {
        let extension = pbt::sample_prefixed(ctx, LOWER, LOWER_DIGIT_HYPHEN, 3, 20);
        let nonce = noprop::sample_bytes::<16>(ctx);

        let request = HandshakeRequest::new("/", "example.com").extension(&extension);
        let encoded = request
            .build(nonce)
            .expect("ハンドシェイクリクエストの構築に失敗してはならない");
        let s = String::from_utf8(encoded).expect("UTF-8 への変換に失敗してはならない");

        let expected_extension = format!("Sec-WebSocket-Extensions: {}", extension);
        assert!(s.contains(&expected_extension));
        Ok(())
    })?;
    Ok(())
}

/// HandshakeRequest に追加ヘッダーを設定
#[test]
fn prop_handshake_request_with_header() -> noprop::TestResult {
    pbt::run(|ctx| {
        let header_name = pbt::sample_prefixed(ctx, UPPER, HEADER_NAME_REST, 3, 15);
        let header_value = pbt::sample_alphabet(ctx, ALNUM_SPACE, 1, 30);
        let nonce = noprop::sample_bytes::<16>(ctx);

        let request = HandshakeRequest::new("/", "example.com").header(&header_name, &header_value);
        let encoded = request
            .build(nonce)
            .expect("ハンドシェイクリクエストの構築に失敗してはならない");
        let s = String::from_utf8(encoded).expect("UTF-8 への変換に失敗してはならない");

        let expected_header = format!("{}: {}", header_name, header_value);
        assert!(s.contains(&expected_header));
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// ServerHandshakeResponse ビルダーのテスト
// =============================================================================

/// ServerHandshakeResponse のビルダーパターン
#[test]
fn prop_server_response_builder() -> noprop::TestResult {
    pbt::run(|ctx| {
        let protocol = pbt::sample_alphabet(ctx, LOWER, 3, 15);
        let extension = pbt::sample_alphabet(ctx, LOWER, 3, 15);
        let header_name = pbt::sample_prefixed(ctx, UPPER, HEADER_NAME_REST, 3, 15);
        let header_value = pbt::sample_alphabet(ctx, ALNUM_SPACE, 1, 30);

        let response = ServerHandshakeResponse::new()
            .protocol(&protocol)
            .extension(&extension)
            .header(&header_name, &header_value);

        assert_eq!(response.protocol, Some(protocol));
        assert_eq!(response.extensions, vec![extension]);
        assert_eq!(response.additional_headers.len(), 1);
        assert_eq!(&response.additional_headers[0].0, &header_name);
        assert_eq!(&response.additional_headers[0].1, &header_value);
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// HandshakeRequestValidator のテスト
// =============================================================================

/// 有効な WebSocket キーを生成
fn generate_valid_ws_key() -> String {
    Base64::encode_string(b"0123456789ABCDEF")
}

/// 有効なハンドシェイクリクエストが正しくパースされる
#[test]
fn prop_valid_handshake_request_validation() -> noprop::TestResult {
    pbt::run(|ctx| {
        let path_rest = pbt::sample_alphabet(ctx, PATH_CHARS, 0, 30);
        let path = format!("/{path_rest}");
        let host_label = pbt::sample_alphabet(ctx, LOWER, 3, 10);
        let host_tld = pbt::sample_alphabet(ctx, LOWER, 2, 4);
        let host = format!("{host_label}.{host_tld}");

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

        assert!(result.is_ok());
        let req = result
            .expect("直前の is_ok の確認どおり成功するはず")
            .expect("検証済みのリクエストが取得できるはず");
        assert_eq!(req.path, path);
        assert_eq!(req.host, host);
        Ok(())
    })?;
    Ok(())
}

/// 不正なメソッドは拒否される
#[test]
fn prop_invalid_method_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let method = noprop::sample_choice(ctx, &INVALID_METHODS);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// 不正な HTTP バージョンは拒否される
#[test]
fn prop_invalid_http_version_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let version = noprop::sample_choice(ctx, &INVALID_HTTP_VERSIONS);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// 不正な Upgrade ヘッダーは拒否される
#[test]
fn prop_invalid_upgrade_header_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let upgrade_value = noprop::sample_choice(ctx, &INVALID_UPGRADE_VALUES);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// 不正な Connection ヘッダーは拒否される
#[test]
fn prop_invalid_connection_header_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let conn_value = noprop::sample_choice(ctx, &INVALID_CONNECTION_VALUES);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// 不正な Sec-WebSocket-Version は拒否される
#[test]
fn prop_invalid_websocket_version_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let version = noprop::sample_choice(ctx, &INVALID_WS_VERSIONS);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// 不正な Sec-WebSocket-Key は拒否される
#[test]
fn prop_invalid_websocket_key_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let invalid_key = pbt::sample_alphabet(ctx, ALNUM, 1, 10);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// HandshakeValidator (クライアント側) のテスト
// =============================================================================

/// 正しい Sec-WebSocket-Accept 値を計算
fn calculate_expected_accept(nonce: &[u8; 16]) -> String {
    const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

    let key = Base64::encode_string(nonce.as_slice());
    let combined = format!("{}{}", key, WEBSOCKET_GUID);

    let hash = aws_lc_rs::digest::digest(
        &aws_lc_rs::digest::SHA1_FOR_LEGACY_USE_ONLY,
        combined.as_bytes(),
    );

    Base64::encode_string(hash.as_ref())
}

/// 有効なハンドシェイクレスポンスが正しくパースされる
#[test]
fn prop_valid_handshake_response_validation() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);

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

        assert!(result.is_ok());
        let resp = result
            .expect("直前の is_ok の確認どおり成功するはず")
            .expect("検証済みのレスポンスが取得できるはず");
        assert!(resp.protocol.is_none());
        assert!(resp.extensions.is_empty());
        Ok(())
    })?;
    Ok(())
}

/// プロトコル付きのレスポンスが正しくパースされる
#[test]
fn prop_handshake_response_with_protocol() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
        let protocol = pbt::sample_alphabet(ctx, LOWER, 3, 15);

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

        assert!(result.is_ok());
        let resp = result
            .expect("直前の is_ok の確認どおり成功するはず")
            .expect("検証済みのレスポンスが取得できるはず");
        assert_eq!(resp.protocol, Some(protocol));
        Ok(())
    })?;
    Ok(())
}

/// 不正なステータスコードは拒否される
#[test]
fn prop_invalid_status_code_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
        let status = noprop::sample_choice(ctx, &INVALID_STATUS_CODES);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// 不正な Sec-WebSocket-Accept は拒否される
#[test]
fn prop_invalid_accept_rejected() -> noprop::TestResult {
    // 表明は、生成した Accept が正しい値と異なるときだけ走る。
    // 正しい Accept は SHA-1 の Base64 で、常に 28 文字。本体は 27 文字、末尾は = である。
    // 本体長は 20..=30 の 11 通り。境界は 5 回に 1 回引き、境界値 20 と 21 と 30 に 27 は無い。
    // 長さが 27 になる確率は約 4/55。64 文字から 27 文字が一致する確率は 64 の 27 乗分の 1。
    // 256 ケースのすべてで一致し、表明が一度も走らない確率は実質 0。
    let mismatched_accept = Cell::new(0usize);
    let runner = pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
        let invalid_accept_body = pbt::sample_alphabet(ctx, BASE64_CHARS, 20, 30);
        let invalid_accept = format!("{invalid_accept_body}=");

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

            assert!(result.is_err());
            mismatched_accept.set(mismatched_accept.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        mismatched_accept.get() > 0,
        "正しい Accept と異なる値で拒否を検証していない\n{runner}"
    );
    Ok(())
}

// =============================================================================
// 欠損ヘッダーのテスト
// =============================================================================

/// Host ヘッダーが欠損している場合は拒否される
#[test]
fn prop_missing_host_header_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。元の定数入力を残す。
        let _dummy = noprop::sample_u8_in(ctx, 0..1);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Upgrade ヘッダーが欠損している場合は拒否される
#[test]
fn prop_missing_upgrade_header_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。元の定数入力を残す。
        let _dummy = noprop::sample_u8_in(ctx, 0..1);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Connection ヘッダーが欠損している場合は拒否される
#[test]
fn prop_missing_connection_header_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。元の定数入力を残す。
        let _dummy = noprop::sample_u8_in(ctx, 0..1);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Sec-WebSocket-Key ヘッダーが欠損している場合は拒否される
#[test]
fn prop_missing_websocket_key_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。元の定数入力を残す。
        let _dummy = noprop::sample_u8_in(ctx, 0..1);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// Sec-WebSocket-Version ヘッダーが欠損している場合は拒否される
#[test]
fn prop_missing_websocket_version_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        // 0..1 は 0 だけ。元の定数入力を残す。
        let _dummy = noprop::sample_u8_in(ctx, 0..1);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// HandshakeValidator の欠損ヘッダーテスト
// =============================================================================

/// レスポンスで Upgrade ヘッダーが欠損している場合は拒否される
#[test]
fn prop_response_missing_upgrade_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// レスポンスで Connection ヘッダーが欠損している場合は拒否される
#[test]
fn prop_response_missing_connection_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);

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

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// レスポンスで Sec-WebSocket-Accept ヘッダーが欠損している場合は拒否される
#[test]
fn prop_response_missing_accept_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);

        let response = "\
HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
\r\n";

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// Extension::parse_strict のテスト
// =============================================================================

/// parse_strict: quoted-string の閉じクォート後に余剰文字がある場合は拒否
#[test]
fn prop_parse_strict_rejects_trailing_after_quote() -> noprop::TestResult {
    pbt::run(|ctx| {
        let token = pbt::sample_alphabet(ctx, LOWER, 3, 10);
        let trailing = pbt::sample_alphabet(ctx, LOWER, 1, 5);

        let input = format!(r#"ext; param="{}"{}"#, token, trailing);
        let result = Extension::parse_strict(&input);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// parse_strict: quoted-string 内のスペース（token ABNF 違反）は拒否
#[test]
fn prop_parse_strict_rejects_space_in_quoted_string() -> noprop::TestResult {
    pbt::run(|ctx| {
        let left = pbt::sample_alphabet(ctx, LOWER, 1, 10);
        let right = pbt::sample_alphabet(ctx, LOWER, 1, 10);

        let input = format!(r#"ext; param="{} {}""#, left, right);
        let result = Extension::parse_strict(&input);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// parse_strict: quoted-string 内のエスケープされたダブルクォート（token ABNF 違反）は拒否
#[test]
fn prop_parse_strict_rejects_escaped_quote() -> noprop::TestResult {
    pbt::run(|ctx| {
        let left = pbt::sample_alphabet(ctx, LOWER, 1, 10);
        let right = pbt::sample_alphabet(ctx, LOWER, 1, 10);

        let input = format!(r#"ext; param="{}\"{}""#, left, right);
        let result = Extension::parse_strict(&input);
        // \" は復号後 " になるが、" は tchar に含まれないため拒否
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// parse_strict: quoted-string 内のエスケープされたバックスラッシュ（token ABNF 違反）は拒否
#[test]
fn prop_parse_strict_rejects_escaped_backslash() -> noprop::TestResult {
    pbt::run(|ctx| {
        let left = pbt::sample_alphabet(ctx, LOWER, 1, 10);
        let right = pbt::sample_alphabet(ctx, LOWER, 1, 10);

        let input = format!(r#"ext; param="{}\\{}""#, left, right);
        let result = Extension::parse_strict(&input);
        // \\ は復号後 \ になるが、\ は tchar に含まれないため拒否
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// parse_strict: 有効な token を quoted-string で囲んでも正しくパースされる
#[test]
fn prop_parse_strict_accepts_valid_quoted_token() -> noprop::TestResult {
    pbt::run(|ctx| {
        let token = pbt::sample_alphabet(ctx, TOKEN_CHARS, 1, 20);

        let input = format!(r#"ext; param="{}""#, token);
        let result = Extension::parse_strict(&input);
        assert!(result.is_ok());
        let exts = result.expect("直前の is_ok の確認どおり成功するはず");
        assert_eq!(exts.len(), 1);
        assert_eq!(&exts[0].params[0].value, &Some(token));
        Ok(())
    })?;
    Ok(())
}

/// parse_strict: unquoted token がそのまま保持される
#[test]
fn prop_parse_strict_preserves_unquoted_token() -> noprop::TestResult {
    pbt::run(|ctx| {
        let token = pbt::sample_alphabet(ctx, TOKEN_CHARS, 1, 20);

        let input = format!("ext; param={}", token);
        let result = Extension::parse_strict(&input);
        assert!(result.is_ok());
        let exts = result.expect("直前の is_ok の確認どおり成功するはず");
        assert_eq!(exts.len(), 1);
        assert_eq!(&exts[0].params[0].value, &Some(token));
        Ok(())
    })?;
    Ok(())
}

/// parse_strict: quoted-string 内のカンマは拡張区切りとして扱わない
#[test]
fn prop_parse_strict_respects_comma_in_quotes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let left = pbt::sample_alphabet(ctx, LOWER, 1, 5);
        let right = pbt::sample_alphabet(ctx, LOWER, 1, 5);

        // "left,right" は token ABNF 違反（カンマは tchar に含まれない）なので拒否される
        let input = format!(r#"ext; param="{},{}", other"#, left, right);
        let result = Extension::parse_strict(&input);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// parse_strict: quoted-string 内のセミコロンはパラメータ区切りとして扱わない
#[test]
fn prop_parse_strict_respects_semicolon_in_quotes() -> noprop::TestResult {
    pbt::run(|ctx| {
        let left = pbt::sample_alphabet(ctx, LOWER, 1, 5);
        let right = pbt::sample_alphabet(ctx, LOWER, 1, 5);

        // "left;right" は token ABNF 違反（セミコロンは tchar に含まれない）なので拒否される
        let input = format!(r#"ext; param="{};{}""#, left, right);
        let result = Extension::parse_strict(&input);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// parse_strict: token に準拠しない非 quoted 値は拒否される
#[test]
fn prop_parse_strict_rejects_invalid_unquoted_value() -> noprop::TestResult {
    pbt::run(|ctx| {
        let left = pbt::sample_alphabet(ctx, LOWER, 1, 5);
        let right = pbt::sample_alphabet(ctx, LOWER, 1, 5);

        // スペースを含む非 quoted 値
        let input = format!("ext; param={} {}", left, right);
        let result = Extension::parse_strict(&input);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// Extension::parse (loose) の quoted-string テスト
// =============================================================================

/// parse (loose): quoted-string 内のスペース（token ABNF 違反）で拡張全体を除外
#[test]
fn prop_parse_loose_excludes_space_in_quoted() -> noprop::TestResult {
    pbt::run(|ctx| {
        let left = pbt::sample_alphabet(ctx, LOWER, 1, 10);
        let right = pbt::sample_alphabet(ctx, LOWER, 1, 10);

        let input = format!(r#"ext; param="{} {}""#, left, right);
        let extensions = Extension::parse(&input);
        assert!(extensions.is_empty());
        Ok(())
    })?;
    Ok(())
}

/// parse (loose): 有効な token を quoted-string で囲んでも正しくパースされる
#[test]
fn prop_parse_loose_accepts_valid_quoted_token() -> noprop::TestResult {
    pbt::run(|ctx| {
        let token = pbt::sample_alphabet(ctx, ALNUM, 1, 20);

        let input = format!(r#"ext; param="{}""#, token);
        let extensions = Extension::parse(&input);
        assert_eq!(extensions.len(), 1);
        assert_eq!(&extensions[0].params[0].value, &Some(token));
        Ok(())
    })?;
    Ok(())
}

/// parse (loose): token に準拠しない非 quoted 値で拡張全体を除外
#[test]
fn prop_parse_loose_excludes_invalid_unquoted() -> noprop::TestResult {
    pbt::run(|ctx| {
        let left = pbt::sample_alphabet(ctx, LOWER, 1, 5);
        let right = pbt::sample_alphabet(ctx, LOWER, 1, 5);

        let input = format!("ext; param={} {}", left, right);
        let extensions = Extension::parse(&input);
        assert!(extensions.is_empty());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// チャンク送信のテスト
// =============================================================================

/// ハンドシェイクリクエストをチャンクで送っても正しくパースされる
#[test]
fn prop_chunked_handshake_request() -> noprop::TestResult {
    pbt::run(|ctx| {
        let chunk_size = pbt::sample_len(ctx, 1, 20);

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

        assert!(result.is_ok());
        Ok(())
    })?;
    Ok(())
}

/// ハンドシェイクレスポンスをチャンクで送っても正しくパースされる
#[test]
fn prop_chunked_handshake_response() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
        let chunk_size = pbt::sample_len(ctx, 1, 20);

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

        assert!(result.is_ok());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// Sec-WebSocket-Protocol の token 検証・一意性チェックのテスト
// =============================================================================

/// token ABNF に違反する Sec-WebSocket-Protocol 値は拒否される
///
/// カンマはリスト区切りとして split されるため除外する。
/// スペースは trim で除去されるため除外する。
#[test]
fn prop_invalid_protocol_token_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let invalid_char = noprop::sample_choice(ctx, &INVALID_PROTOCOL_CHARS);
        let prefix = pbt::sample_alphabet(ctx, LOWER, 1, 5);
        let suffix = pbt::sample_alphabet(ctx, LOWER, 1, 5);

        let key = generate_valid_ws_key();
        let invalid_protocol = format!("{}{}{}", prefix, invalid_char, suffix);
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Sec-WebSocket-Protocol: {}\r\n\
             \r\n",
            key, invalid_protocol
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// 重複する Sec-WebSocket-Protocol 値は拒否される
#[test]
fn prop_duplicate_protocol_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let protocol = pbt::sample_alphabet(ctx, LOWER, 3, 10);

        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Sec-WebSocket-Protocol: {}, {}\r\n\
             \r\n",
            key, protocol, protocol
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// 有効な token の Sec-WebSocket-Protocol 値は受理される
#[test]
fn prop_valid_protocol_token_accepted() -> noprop::TestResult {
    pbt::run(|ctx| {
        let protocol = pbt::sample_prefixed(ctx, LETTERS, TOKEN_CHARS, 0, 19);

        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Sec-WebSocket-Protocol: {}\r\n\
             \r\n",
            key, protocol
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        assert!(result.is_ok());
        let req = result
            .expect("直前の is_ok の確認どおり成功するはず")
            .expect("検証済みのリクエストが取得できるはず");
        assert_eq!(req.protocols, vec![protocol]);
        Ok(())
    })?;
    Ok(())
}

/// 一意な複数プロトコルは受理される
#[test]
fn prop_unique_multiple_protocols_accepted() -> noprop::TestResult {
    pbt::run(|ctx| {
        let p1 = pbt::sample_alphabet(ctx, LOWER, 3, 8);
        let p2 = pbt::sample_alphabet(ctx, UPPER, 3, 8);

        // p1 と p2 が同一でないことを保証（大小が異なるため常に異なる）
        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Sec-WebSocket-Protocol: {}, {}\r\n\
             \r\n",
            key, p1, p2
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        assert!(result.is_ok());
        let req = result
            .expect("直前の is_ok の確認どおり成功するはず")
            .expect("検証済みのリクエストが取得できるはず");
        assert_eq!(req.protocols, vec![p1, p2]);
        Ok(())
    })?;
    Ok(())
}

/// 複数ヘッダー行に分散した重複プロトコルも拒否される
#[test]
fn prop_duplicate_protocol_across_headers_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let protocol = pbt::sample_alphabet(ctx, LOWER, 3, 10);

        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Sec-WebSocket-Protocol: {}\r\n\
             Sec-WebSocket-Protocol: {}\r\n\
             \r\n",
            key, protocol, protocol
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// 空要素ヘッダーの拒否テスト (RFC 6455 ABNF 1#extension / 1#token)
// =============================================================================

/// サーバー側: Sec-WebSocket-Extensions にカンマのみの値は拒否される
#[test]
fn prop_server_extensions_empty_elements_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let commas = pbt::sample_repeated_ascii(ctx, b',', 1, 5);

        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Sec-WebSocket-Extensions: {}\r\n\
             \r\n",
            key, commas
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// クライアント側: サーバーレスポンスの Sec-WebSocket-Extensions にカンマのみの値は拒否される
#[test]
fn prop_client_extensions_empty_elements_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
        let commas = pbt::sample_repeated_ascii(ctx, b',', 1, 5);

        let accept = calculate_expected_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             Sec-WebSocket-Extensions: {}\r\n\
             \r\n",
            accept, commas
        );

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// サーバー側: Sec-WebSocket-Protocol にカンマのみの値は拒否される
#[test]
fn prop_server_protocol_empty_elements_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let commas = pbt::sample_repeated_ascii(ctx, b',', 1, 5);

        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Sec-WebSocket-Protocol: {}\r\n\
             \r\n",
            key, commas
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// build() のプロトコル検証テスト
// =============================================================================

/// build() は不正な token 文字を含むプロトコルを拒否する
#[test]
fn prop_build_rejects_invalid_protocol_token() -> noprop::TestResult {
    pbt::run(|ctx| {
        let invalid_char = noprop::sample_choice(ctx, &INVALID_PROTOCOL_CHARS);
        let prefix = pbt::sample_alphabet(ctx, LOWER, 1, 5);
        let suffix = pbt::sample_alphabet(ctx, LOWER, 1, 5);
        let nonce = noprop::sample_bytes::<16>(ctx);

        let invalid_protocol = format!("{}{}{}", prefix, invalid_char, suffix);
        let request = HandshakeRequest::new("/", "example.com").protocol(&invalid_protocol);
        let result = request.build(nonce);

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// build() は重複プロトコルを拒否する
#[test]
fn prop_build_rejects_duplicate_protocol() -> noprop::TestResult {
    pbt::run(|ctx| {
        let protocol = pbt::sample_alphabet(ctx, LOWER, 3, 10);
        let nonce = noprop::sample_bytes::<16>(ctx);

        let request = HandshakeRequest::new("/", "example.com")
            .protocol(&protocol)
            .protocol(&protocol);
        let result = request.build(nonce);

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// RFC 6455 Section 9.1: trailing ';' ABNF 違反テスト
// =============================================================================

/// parse_strict: trailing ';' は ABNF 違反として拒否される
///
/// RFC 6455 Section 9.1: extension = extension-token *( ";" extension-param )
/// ";" の後は必ず extension-param が必要。
#[test]
fn prop_parse_strict_rejects_trailing_semicolon() -> noprop::TestResult {
    pbt::run(|ctx| {
        let name = pbt::sample_prefixed(ctx, LOWER, LOWER_DIGIT_HYPHEN, 2, 10);

        let input = format!("{};", name);
        let result = Extension::parse_strict(&input);
        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

/// parse_strict: 有効な拡張はそのまま受理される（正常系の確認）
#[test]
fn prop_parse_strict_accepts_valid_extension() -> noprop::TestResult {
    pbt::run(|ctx| {
        let name = pbt::sample_prefixed(ctx, LOWER, LOWER_DIGIT_HYPHEN, 2, 10);

        let result = Extension::parse_strict(&name);
        assert!(result.is_ok());
        assert_eq!(
            result.expect("直前の is_ok の確認どおり成功するはず").len(),
            1
        );
        Ok(())
    })?;
    Ok(())
}

/// HandshakeRequestValidator: trailing ';' を持つ拡張ヘッダーは拒否される
///
/// RFC 6455 Section 9.1: ABNF に適合しない場合は接続を失敗させなければならない。
#[test]
fn prop_request_validator_rejects_trailing_semicolon_in_extension() -> noprop::TestResult {
    pbt::run(|ctx| {
        let name = pbt::sample_prefixed(ctx, LOWER, LOWER_DIGIT_HYPHEN, 2, 10);

        let key = generate_valid_ws_key();
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: example.com\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Sec-WebSocket-Extensions: {};\r\n\
             \r\n",
            key, name
        );

        let mut validator = HandshakeRequestValidator::new();
        validator.feed(request.as_bytes());
        let result = validator.validate();

        assert!(result.is_err());
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// RFC 6455 Section 4.2.2 項目 6: HTTP レスポンス複数行 Sec-WebSocket-Extensions 統合テスト
// =============================================================================

/// HandshakeValidator: レスポンスで Sec-WebSocket-Extensions が複数行ある場合は受理される
///
/// RFC 6455 Section 4.2.2 項目 6: 複数行の Sec-WebSocket-Extensions を許容し統合する
#[test]
fn prop_response_multiple_extension_headers_accepted() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
        let ext1 = pbt::sample_prefixed(ctx, LOWER, LOWER_DIGIT_HYPHEN, 2, 10);
        let ext2 = pbt::sample_prefixed(ctx, LOWER, LOWER_DIGIT_HYPHEN, 2, 10);

        let accept = calculate_expected_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             Sec-WebSocket-Extensions: {}\r\n\
             Sec-WebSocket-Extensions: {}\r\n\
             \r\n",
            accept, ext1, ext2
        );

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        assert!(
            result.is_ok(),
            "複数行の Sec-WebSocket-Extensions は受理されるべき"
        );
        let resp = result
            .expect("直前の is_ok の確認どおり成功するはず")
            .expect("バリデーション結果が取得できるはず");
        assert!(
            resp.extensions.contains(&ext1) || resp.extensions.contains(&ext2),
            "マージ後の extensions に拡張が含まれていない"
        );
        Ok(())
    })?;
    Ok(())
}

/// HandshakeValidator: レスポンスで Sec-WebSocket-Extensions が 1 行のみの場合は受理される
#[test]
fn prop_response_single_extension_header_accepted() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
        let ext = pbt::sample_prefixed(ctx, LOWER, LOWER_DIGIT_HYPHEN, 2, 10);

        let accept = calculate_expected_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             Sec-WebSocket-Extensions: {}\r\n\
             \r\n",
            accept, ext
        );

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        assert!(result.is_ok());
        Ok(())
    })?;
    Ok(())
}

/// HandshakeValidator: 1 行目が空、2 行目が有効な場合はマージ後 1 要素
#[test]
fn prop_response_first_empty_second_valid_extension() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);
        let ext = pbt::sample_prefixed(ctx, LOWER, LOWER_DIGIT_HYPHEN, 2, 10);

        let accept = calculate_expected_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             Sec-WebSocket-Extensions: \r\n\
             Sec-WebSocket-Extensions: {}\r\n\
             \r\n",
            accept, ext
        );

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        assert!(result.is_ok(), "1 行目が空でも受理されるべき");
        let resp = result
            .expect("直前の is_ok の確認どおり成功するはず")
            .expect("検証済みのレスポンスが取得できるはず");
        assert!(
            resp.extensions.contains(&ext),
            "マージ後の extensions に有効な拡張が含まれていない"
        );
        Ok(())
    })?;
    Ok(())
}

/// HandshakeValidator: 2 行とも空の場合は malformed エラー
#[test]
fn prop_response_both_empty_extension_headers() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);

        let accept = calculate_expected_accept(&nonce);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             Sec-WebSocket-Extensions: \r\n\
             Sec-WebSocket-Extensions: \r\n\
             \r\n",
            accept
        );

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        assert!(
            result.is_err(),
            "2 行とも空の場合は malformed エラーであるべき"
        );
        Ok(())
    })?;
    Ok(())
}

/// HandshakeValidator: 2 行とも ABNF 違反の場合はエラー
#[test]
fn prop_response_both_invalid_extension_headers() -> noprop::TestResult {
    pbt::run(|ctx| {
        let nonce = noprop::sample_bytes::<16>(ctx);

        let accept = calculate_expected_accept(&nonce);
        // ABNF 違反: token に使用できない文字 '(' を含む拡張文字列
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             Sec-WebSocket-Extensions: inv(alid\r\n\
             Sec-WebSocket-Extensions: als(obad\r\n\
             \r\n",
            accept
        );

        let mut validator = HandshakeValidator::new(nonce);
        validator.feed(response.as_bytes());
        let result = validator.validate();

        assert!(result.is_err(), "ABNF 違反の拡張は拒否されるべき");
        Ok(())
    })?;
    Ok(())
}

// =============================================================================
// RFC 7692: window_bits 先頭ゼロ拒否テスト
// =============================================================================

/// server_max_window_bits に先頭ゼロが含まれる場合は拒否される
///
/// RFC 7692 Section 7.1.2.1: a decimal integer value without leading zeroes
#[test]
fn prop_server_max_window_bits_leading_zeros_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let valid_bits = noprop::sample_u8_in(ctx, 8..=15);
        let zero_count = pbt::sample_len_inclusive(ctx, 1, 3);

        let s = valid_bits.to_string();
        let value = format!("{:0>width$}", s, width = s.len() + zero_count);
        let ext = Extension::new("permessage-deflate")
            .param("server_max_window_bits", Some(value.as_str()));
        let result = PerMessageDeflateConfig::from_extension_for_server_request(&ext);
        assert!(result.is_err(), "leading zeros must be rejected: {}", value);
        Ok(())
    })?;
    Ok(())
}

/// client_max_window_bits に先頭ゼロが含まれる場合は拒否される
///
/// RFC 7692 Section 7.1.2.2: a decimal integer value without leading zeroes
#[test]
fn prop_client_max_window_bits_leading_zeros_rejected() -> noprop::TestResult {
    pbt::run(|ctx| {
        let valid_bits = noprop::sample_u8_in(ctx, 8..=15);
        let zero_count = pbt::sample_len_inclusive(ctx, 1, 3);

        let s = valid_bits.to_string();
        let value = format!("{:0>width$}", s, width = s.len() + zero_count);
        let ext = Extension::new("permessage-deflate")
            .param("client_max_window_bits", Some(value.as_str()));
        let result = PerMessageDeflateConfig::from_extension_for_server_request(&ext);
        assert!(result.is_err(), "leading zeros must be rejected: {}", value);
        Ok(())
    })?;
    Ok(())
}

/// 先頭ゼロなしの有効な window_bits は受理される
#[test]
fn prop_window_bits_valid_values_accepted() -> noprop::TestResult {
    pbt::run(|ctx| {
        let bits = noprop::sample_u8_in(ctx, 8..=15);

        let value = bits.to_string();
        let ext =
            Extension::new("permessage-deflate").param("server_max_window_bits", Some(&value));
        let result = PerMessageDeflateConfig::from_extension_for_server_request(&ext);
        assert!(result.is_ok(), "valid value must be accepted: {}", value);
        Ok(())
    })?;
    Ok(())
}

/// HandshakeRequest::build で予約済みヘッダーを追加するとエラーになる
///
/// RFC 6455 Section 4.1: クライアントハンドシェイクの予約済みヘッダーは重複してはならない
#[test]
fn prop_build_rejects_reserved_header() -> noprop::TestResult {
    pbt::run(|ctx| {
        let reserved = noprop::sample_choice(ctx, &RESERVED_HEADERS);
        let value = pbt::sample_alphabet(ctx, ALNUM, 1, 20);
        let nonce = noprop::sample_bytes::<16>(ctx);

        let request = HandshakeRequest::new("/ws", "example.com").header(reserved, &value);

        let result = request.build(nonce);
        assert!(
            result.is_err(),
            "build() should reject reserved header '{}'",
            reserved
        );
        Ok(())
    })?;
    Ok(())
}
