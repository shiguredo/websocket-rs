#![no_main]
use libfuzzer_sys::fuzz_target;
use shiguredo_websocket::{Extension, PerMessageDeflateConfig};

fuzz_target!(|data: &[u8]| {
    // UTF-8 文字列として扱う
    if let Ok(s) = std::str::from_utf8(data) {
        // Extension のパースを試みる（loose / strict 両方）
        let _ = Extension::parse(s);

        // parse_strict が成功した Extension を PMCE config パースに通す
        if let Ok(exts) = Extension::parse_strict(s) {
            for ext in &exts {
                let _ = PerMessageDeflateConfig::from_extension_for_server_request(ext);
                let _ = PerMessageDeflateConfig::from_extension_for_client_response(ext);
            }
        }
    }
});
