//! websocket_frame.rs の単体テスト
//!
//! PBT の一様サンプリングは境界値やエラー条件を保証しないため、
//! `Frame::close` の拒否条件と境界を決定的に検証する。

use shiguredo_websocket::{ErrorKind, Frame, Opcode};

/// 送信不可のコードは拒否される
#[test]
fn close_rejects_unsendable_codes() {
    for code in [1004, 1005, 1006, 1015, 1016, 2000, 5000, 65535] {
        let err =
            Frame::close(Some(code), "").expect_err(&format!("{code} は送信不可のため拒否される"));
        assert_eq!(err.kind, ErrorKind::InvalidInput, "{code} の ErrorKind");
        assert!(
            err.to_string().contains("close code is not sendable"),
            "{code} のエラーメッセージ: {err}"
        );
    }
}

/// 送信可能なコードと理由はそのまま格納される
#[test]
fn close_accepts_sendable_codes() {
    for code in [1000, 1003, 1007, 1014, 3000, 4999] {
        let frame = Frame::close(Some(code), "bye").expect("送信可能なコードは受理される");
        assert_eq!(frame.opcode, Opcode::Close);
        assert_eq!(&frame.payload[..2], &code.to_be_bytes(), "{code} のコード");
        assert_eq!(&frame.payload[2..], b"bye", "{code} の理由");
    }

    let frame = Frame::close(None, "").expect("コードなしの Close は受理される");
    assert!(frame.payload.is_empty());
}

/// コードなしで理由だけを指定することはできない
#[test]
fn close_rejects_reason_without_code() {
    let err = Frame::close(None, "bye").expect_err("コードなしの理由は拒否される");
    assert_eq!(err.kind, ErrorKind::InvalidInput);
    assert!(
        err.to_string()
            .contains("close reason requires a close code"),
        "エラーメッセージ: {err}"
    );
}

/// 理由は 123 バイト以下でなければならない
#[test]
fn close_rejects_reason_over_123_bytes() {
    let err = Frame::close(Some(1000), &"a".repeat(124)).expect_err("124 バイトの理由は拒否される");
    assert_eq!(err.kind, ErrorKind::InvalidInput);

    let frame = Frame::close(Some(1000), &"a".repeat(123)).expect("123 バイトの理由は受理される");
    assert_eq!(frame.payload.len(), 125);

    // UTF-8 のバイト長で判定する ("あ" は 3 バイト)
    let frame = Frame::close(Some(1000), &"あ".repeat(41)).expect("123 バイトの理由は受理される");
    assert_eq!(frame.payload.len(), 125);
    let err = Frame::close(Some(1000), &"あ".repeat(42)).expect_err("126 バイトの理由は拒否される");
    assert_eq!(err.kind, ErrorKind::InvalidInput);
}
