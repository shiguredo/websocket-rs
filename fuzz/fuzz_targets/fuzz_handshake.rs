#![no_main]
use libfuzzer_sys::fuzz_target;
use shiguredo_websocket::HandshakeValidator;

fuzz_target!(|data: &[u8]| {
    // 固定の nonce を使用
    let nonce = *b"fuzz_test_nonce!";
    let mut validator = HandshakeValidator::new(nonce);

    validator.feed(data);

    // 検証を試みる（エラーでもパニックしなければOK）
    let _ = validator.validate();
});
