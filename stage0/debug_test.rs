use kore_stage0::diag::diagnostic::FileId;
use kore_stage0::diag::sink::DiagSink;
use kore_stage0::frontend::lex::tokenize;
use kore_stage0::frontend::parse::parse;
use kore_stage0::middleend::lower::lower_module;

fn main() {
    let source = r#"
abs :: (x i32) i32 => {
    ? {
        x < 0 => ret -x
        _ => ret x
    }
}
"#;

    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), source, &mut sink);

    if sink.has_errors() {
        println!("Lex errors: {:?}", sink.drain());
        return;
    }

    let ast = parse(&tokens, &mut sink);

    if sink.has_errors() {
        println!("Parse errors: {:?}", sink.drain());
        return;
    }

    let hir = lower_module(&ast, &mut sink);

    println!("HIR functions:");
    for func in &hir.functions {
        println!("\nFunction {}:", func.name);
        for (i, block) in func.blocks.iter().enumerate() {
            println!("  Block {}: {:?}", i, block.id);
            println!("    Stmts: {}", block.stmts.len());
            println!("    Terminator: {:?}", block.terminator);
        }
    }

    if sink.has_errors() {
        println!("\nLowering errors:");
        for diag in sink.drain() {
            println!("  {:?}", diag);
        }
    }
}
