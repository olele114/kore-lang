use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;
use kore_stage0::frontend::resolve::Resolver;
use kore_stage0::frontend::typecheck::TypeChecker;
use kore_stage0::middleend::lower::lower_module;
use kore_stage0::backend::compile_to_llvm;

fn main() {
    let src = r#"
add :: (x i32, y i32) i32 => x + y
    "#;

    let mut sink = DiagSink::new();
    let file_id = FileId(0);

    // 词法分析
    let tokens = tokenize(file_id, src, &mut sink);
    if sink.has_errors() {
        eprintln!("Lexer errors: {:?}", sink.finish());
        return;
    }

    // 语法分析
    let ast = parse(file_id, tokens, &mut sink);
    if sink.has_errors() {
        eprintln!("Parser errors: {:?}", sink.finish());
        return;
    }

    // 名称解析
    let resolver = Resolver::new(&mut sink);
    let symtab = resolver.resolve(&ast);
    if sink.has_errors() {
        eprintln!("Resolver errors: {:?}", sink.finish());
        return;
    }

    // 类型检查
    let type_ctx = {
        let mut checker = TypeChecker::new(&symtab, &mut sink);
        checker.check_module(&ast);
        checker.type_context().clone()
    };
    if sink.has_errors() {
        eprintln!("Type checker errors: {:?}", sink.finish());
        return;
    }

    // HIR 降级
    let hir = lower_module(&ast, &symtab, &type_ctx, &mut sink);
    if sink.has_errors() {
        eprintln!("HIR lowering errors: {:?}", sink.finish());
        return;
    }

    println!("=== HIR Module ===");
    println!("Functions: {}", hir.functions.len());
    for func in &hir.functions {
        let body = func.body.as_ref().unwrap();
        println!("  - {}: {} params, {} locals, {} blocks",
                 func.name, func.params.len(), body.locals.len(), body.blocks.len());
    }

    // LLVM 代码生成
    match compile_to_llvm(&hir, &mut sink) {
        Some(llvm_ir) => {
            println!("\n=== LLVM IR ===");
            println!("{}", llvm_ir);
        }
        None => {
            eprintln!("\nBackend codegen failed");
        }
    }
}
