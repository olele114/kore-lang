use kore_stage0::diag::diagnostic::FileId;
use kore_stage0::diag::sink::DiagSink;
use kore_stage0::driver::run_frontend;
use kore_stage0::middleend::lower::lower_module;

fn main() {
    let source = r#"
Point :: {
    x, y i32
}

get_x :: (p Point) i32 => p.x
"#;

    let mut sink = DiagSink::new();
    let frontend_out = run_frontend(FileId(0), source, &mut sink);

    if let Some(module) = &frontend_out.module {
        println!("AST items: {:?}", module.items.len());

        // Debug: check AST field types
        use kore_stage0::frontend::ast::Item;
        for item in &module.items {
            if let Item::Struct(s) = item {
                println!("AST Struct '{}' with {} fields:", s.name, s.fields.len());
                for f in &s.fields {
                    println!("  - field '{}': {:?}", f.name, f.ty);
                }
            }
        }
    }

    if let Some(type_ctx) = &frontend_out.type_ctx {
        println!("TypeContext structs registered:");
        // Debug: check if Point struct is in type_ctx
        if let Some(fields) = type_ctx.get_struct("Point") {
            println!("  Point struct found with {} fields", fields.len());
            for (fname, fty) in fields {
                println!("    - {}: {:?}", fname, fty);
            }
        } else {
            println!("  Point struct NOT found!");
        }
    }

    if sink.has_errors() {
        println!("\nFrontend Errors:");
        for diag in sink.finish() {
            println!("  {:?}", diag);
        }
        return;
    }

    // 使用前端产出的 symbols 和 type_ctx 进行 lowering
    let module = frontend_out.module.as_ref().unwrap();
    let symbols = frontend_out.symbols.as_ref().unwrap();
    let type_ctx = frontend_out.type_ctx.as_ref().unwrap();

    let hir = lower_module(module, symbols, type_ctx, &mut sink);

    println!("\nHIR structs: {:?}", hir.structs.len());
    for s in &hir.structs {
        println!("  HIR Struct name: {}, fields: {:?}", s.name, s.fields.len());
    }

    println!("\nHIR functions: {:?}", hir.functions.len());
    for func in &hir.functions {
        println!("  Function: {}", func.name);
        println!("    Return type: {:?}", func.ret_type);
        if let Some(body) = &func.body {
            for (i, block) in body.blocks.iter().enumerate() {
                println!("    Block {}:", i);
                for stmt in &block.stmts {
                    println!("      Stmt: {:?}", stmt);
                }
                println!("      Term: {:?}", block.terminator);
            }
        }
    }

    if sink.has_errors() {
        println!("\nErrors:");
        for diag in sink.finish() {
            println!("  {:?}", diag);
        }
    }
}
