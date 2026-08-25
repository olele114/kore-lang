use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;

fn main() {
    let src = r#"main :: (argc i32, argv [][]u8) void => {
    ~i := 0
    @ i < argc {
        print("test")
    }
}"#;

    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), src, &mut sink);

    println!("=== Parsing ===");
    let ast = parse(FileId(0), tokens, &mut sink);

    if sink.has_errors() {
        println!("\nParsing failed with errors");
    } else {
        println!("\nParsing succeeded!");
        println!("AST: {:#?}", ast);
    }
}
