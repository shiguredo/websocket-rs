#![no_main]
use libfuzzer_sys::fuzz_target;
use shiguredo_websocket::Extension;

fuzz_target!(|data: &[u8]| {
    // UTF-8 文字列として扱う
    if let Ok(s) = std::str::from_utf8(data) {
        // Extension のパースを試みる
        let _ = Extension::parse(s);
    }
});
