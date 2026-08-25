#![no_main]

use libfuzzer_sys::fuzz_target;
use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;

fuzz_target!(|data: &[u8]| {
    // 只处理有效的 UTF-8 输入
    if let Ok(source) = std::str::from_utf8(data) {
        // 词法分析
        let mut lex_sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut lex_sink);

        // 语法分析
        let mut parse_sink = DiagSink::new();
        let _ = parse(FileId(0), tokens, &mut parse_sink);
        // 不 panic 就是成功
    }
});
