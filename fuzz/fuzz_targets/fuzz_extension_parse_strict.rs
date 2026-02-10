#![no_main]
use libfuzzer_sys::fuzz_target;
use shiguredo_websocket::Extension;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Extension::parse_strict のファジング
        // RFC 6455 Section 9.1 の ABNF 検証を行うセキュリティ上重要なパーサー
        let _ = Extension::parse_strict(s);
    }
});
