#![no_main]
use libfuzzer_sys::fuzz_target;
use shiguredo_websocket::{Compressor, PerMessageDeflateConfig};

fuzz_target!(|data: &[u8]| {
    // 入力先頭バイトで is_client / no_context_takeover を切り替え、
    // RFC 7692 Section 7.1.1 の両端側コンテキスト持ち越しと履歴リセット経路を網羅する
    let flags = data.first().copied().unwrap_or(0);
    let is_client = flags & 0b0001 == 0;
    let mut config = PerMessageDeflateConfig::default();
    if flags & 0b0010 != 0 {
        config = config.client_no_context_takeover();
    }
    if flags & 0b0100 != 0 {
        config = config.server_no_context_takeover();
    }

    let mut compressor = Compressor::new(&config, is_client);
    let payload = data.get(1..).unwrap_or(&[]);

    // 任意のバイト列を圧縮してもパニックしないことを検証する
    let _ = compressor.compress(payload);
});
