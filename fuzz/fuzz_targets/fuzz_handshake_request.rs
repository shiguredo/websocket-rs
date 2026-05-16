#![no_main]
use libfuzzer_sys::fuzz_target;
use shiguredo_websocket::HandshakeRequestValidator;

fuzz_target!(|data: &[u8]| {
    let mut validator = HandshakeRequestValidator::new();
    validator.feed(data);

    // 検証を試みる（エラーでもパニックしなければOK）
    let _ = validator.validate();
});
