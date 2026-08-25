use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::lexer::tokenize;

fn main() {
    let src = r#"main :: (argc i32, argv [][]u8) void => {
    ~i := 0
    @ i < argc {
        print("test")
    }
}"#;

    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), src, &mut sink);

    println!("Tokens:");
    for (idx, tok) in tokens.iter().enumerate() {
        println!("{:3}: {:?} @ {:?}", idx, tok.kind, tok.span);
    }

    println!("\nHas errors: {}", sink.has_errors());
}
