use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::diag::{DiagSink, FileId};

fn main() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "fn if", &mut sink);
    for (i, tok) in tokens.iter().enumerate() {
        println!("{}: {:?}", i, tok.kind);
    }
}
