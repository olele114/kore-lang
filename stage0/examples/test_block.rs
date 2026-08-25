use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;
use kore_stage0::frontend::ast::node::{Item, Expr};

fn main() {
    let mut sink = DiagSink::new();
    let source = r#"
main :: () void => {
    x := 1
    y := 2
    x + y
}
"#;
    let tokens = tokenize(FileId(0), source, &mut sink);
    println!("Tokens: {:?}", tokens.iter().map(|t| &t.kind).collect::<Vec<_>>());

    let module = parse(FileId(0), tokens, &mut sink);

    if let Some(Item::Func(f)) = module.items.first()
        && let Expr::Block { stmts, .. } = &f.body
    {
        println!("Block has {} statements:", stmts.len());
        for (i, stmt) in stmts.iter().enumerate() {
            println!("  [{}] {:?}", i, stmt);
        }
    }

    for diag in sink.peek() {
        println!("Diagnostic: {} - {}", diag.code_str(), diag.msg);
    }
}
