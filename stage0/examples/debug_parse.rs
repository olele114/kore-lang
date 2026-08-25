use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;

fn main() {
    let mut sink = DiagSink::new();
    let source = std::io::read_to_string(std::io::stdin()).unwrap_or_else(|_| "x :: 42".to_string());

    println!("Source: {}", source);

    let tokens = tokenize(FileId(0), &source, &mut sink);
    println!("Tokens count: {}", tokens.len());
    for tok in &tokens {
        println!("  {:?}", tok);
    }
    println!("Lexer errors: {}", sink.err_count());

    let module = parse(FileId(0), tokens, &mut sink);
    println!("Items count: {}", module.items.len());
    println!("Items: {:#?}", module.items);
    println!("Parser errors: {}", sink.err_count());

    for diag in sink.peek() {
        println!("Diagnostic: {} - {}", diag.code_str(), diag.msg);
    }
}
