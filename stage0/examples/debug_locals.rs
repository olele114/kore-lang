use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::driver::run_frontend;
use kore_stage0::middleend::lower::lower_module;

fn main() {
    let source = r#"
Point :: { x i32, y i32 }

test :: (p own ^Point) void => {
    x := p^.x
}
"#;

    let mut diag = DiagSink::new();
    let frontend = run_frontend(FileId(0), source, &mut diag);

    if diag.has_errors() {
        eprintln!("Frontend errors:");
        for d in diag.peek() {
            eprintln!("  {:?}", d);
        }
        return;
    }

    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);
    let test_func = hir.functions.iter().find(|f| f.name == "test").unwrap();

    println!("Function: {}", test_func.name);
    let body = test_func.body.as_ref().unwrap();
    println!("Locals ({} total):", body.locals.len());
    for (idx, local) in body.locals.iter().enumerate() {
        println!("  LocalId({}) = {:?} (type: {:?})", idx, local.name, local.ty);
    }
}
