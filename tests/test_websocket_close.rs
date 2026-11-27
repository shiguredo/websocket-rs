//! websocket_close.rs の単体テスト
//!
//! PBT の一様サンプリングは境界値を保証しないため、送信可否と受信可否の
//! 境界を決定的に検証する。

use shiguredo_websocket::CloseCode;

/// 送信可否と受信可否を (code, is_valid, is_sendable) の表で検証する
#[test]
fn is_valid_and_is_sendable_boundaries() {
    let cases = [
        (0, false, false),
        (999, false, false),
        (1000, true, true),
        (1001, true, true),
        (1002, true, true),
        (1003, true, true),
        (1004, true, false),
        (1005, false, false),
        (1006, false, false),
        (1007, true, true),
        (1011, true, true),
        (1012, true, true),
        (1013, true, true),
        (1014, true, true),
        (1015, false, false),
        (1016, true, false),
        (1100, true, false),
        (1999, true, false),
        (2000, true, false),
        (2999, true, false),
        (3000, true, true),
        (3999, true, true),
        (4000, true, true),
        (4999, true, true),
        (5000, false, false),
        (65535, false, false),
    ];

    for (code, valid, sendable) in cases {
        let close_code = CloseCode::new(code);
        assert_eq!(close_code.is_valid(), valid, "{code} の is_valid");
        assert_eq!(close_code.is_sendable(), sendable, "{code} の is_sendable");
    }
}
