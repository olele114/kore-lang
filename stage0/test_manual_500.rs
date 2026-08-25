use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;

fn main() {
    let mut source = String::from("main :: () void => {\n");
    for _ in 0..500 {
        source.push_str("{\n");
    }
    source.push_str("x := 1\n");
    for _ in 0..500 {
        source.push_str("}\n");
    }
    source.push_str("}");
    
    println!("Source length: {} bytes", source.len());
    println!("Starting tokenize...");
    
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), &source, &mut sink);
    
    println!("Tokens: {}", tokens.len());
    println!("Starting parse...");
    
    let _result = parse(FileId(0), tokens, &mut sink);
    
    println!("Parse completed");
    println!("Errors: {}", sink.has_errors());
    
    // 触发 drop
    println!("Done, exiting...");
}
