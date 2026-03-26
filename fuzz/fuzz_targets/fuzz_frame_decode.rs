#![no_main]
use libfuzzer_sys::fuzz_target;
use shiguredo_websocket::FrameDecoder;

fuzz_target!(|data: &[u8]| {
    let mut decoder = FrameDecoder::new();
    decoder.feed(data);

    // デコードを試みる（エラーでもパニックしなければOK）
    loop {
        match decoder.decode() {
            Ok(Some(_frame)) => {
                // 正常にデコードできた
            }
            Ok(None) => {
                // データ不足
                break;
            }
            Err(_) => {
                // プロトコルエラー（これは期待される動作）
                break;
            }
        }
    }
});
