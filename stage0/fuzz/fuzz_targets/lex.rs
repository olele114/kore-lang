#![no_main]

use libfuzzer_sys::fuzz_target;
use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::lexer::tokenize;

fuzz_target!(|data: &[u8]| {
    // 只处理有效的 UTF-8 输入
    if let Ok(source) = std::str::from_utf8(data) {
        let mut sink = DiagSink::new();
        let _ = tokenize(FileId(0), source, &mut sink);
        // 不 panic 就是成功
    }
});
