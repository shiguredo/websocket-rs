#![no_main]
use libfuzzer_sys::fuzz_target;
use shiguredo_websocket::{Decompressor, PerMessageDeflateConfig};

fuzz_target!(|data: &[u8]| {
    // 不正な DEFLATE ストリームや Zip Bomb に対する耐性を検証
    let config = PerMessageDeflateConfig::default();
    let mut decompressor = Decompressor::new(&config, true);

    // 最大解凍サイズを制限して Zip Bomb 対策を検証
    let max_size = 1024 * 1024; // 1MB
    let _ = decompressor.decompress(data, max_size);
});
