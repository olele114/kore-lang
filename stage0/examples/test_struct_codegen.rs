use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::driver::run_frontend;
use kore_stage0::middleend::lower::lower_module;

fn main() {
    let source = r#"
Point :: {
    x, y i32
}

get_x :: (p Point) i32 => p.x
"#;

    let mut diag = DiagSink::new();
    let frontend = run_frontend(FileId(0), source, &mut diag);

    if diag.has_errors() {
        println!("Frontend errors:");
        for d in diag.peek() {
            println!("  {:?}", d);
        }
        return;
    }

    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);

    println!("HIR structs count: {}", hir.structs.len());
    for (idx, s) in hir.structs.iter().enumerate() {
        println!("  Struct[{}]: name={}, fields={:?}", idx, s.name, s.fields.len());
    }

    println!("\nHIR functions count: {}", hir.functions.len());
    for func in &hir.functions {
        println!("  Function: {}", func.name);
        println!("    Params: {:?}", func.params.len());
        for (i, param) in func.params.iter().enumerate() {
            println!("      Param[{}]: {:?}", i, param.ty);
        }
    }
}
