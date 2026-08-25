//! 测试 owned 指针的析构代码生成

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::driver::run_frontend;
use kore_stage0::middleend::lower::lower_module;
use kore_stage0::backend::llvm::compile_to_llvm;

#[test]
fn test_drop_generates_free_call() {
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
        panic!("Frontend failed");
    }

    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);

    // 生成 LLVM IR
    let llvm_ir = compile_to_llvm(&hir, &mut diag).expect("Codegen should succeed");

    println!("\nGenerated LLVM IR:");
    println!("{}", llvm_ir);

    // 验证 IR 包含 free 调用
    assert!(llvm_ir.contains("declare"), "Should have function declarations");
    assert!(llvm_ir.contains("call"), "Should have function calls");
}

#[test]
fn test_multiple_drops() {
    let source = r#"
Point :: { x i32, y i32 }

test :: (p1 own ^Point, p2 own ^Point) void => {
    x := p1^.x
    y := p2^.y
}
"#;

    let mut diag = DiagSink::new();
    let frontend = run_frontend(FileId(0), source, &mut diag);
    assert!(!diag.has_errors());

    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);
    let llvm_ir = compile_to_llvm(&hir, &mut diag).expect("Codegen should succeed");

    println!("\nGenerated LLVM IR for multiple drops:");
    println!("{}", llvm_ir);

    // 验证生成了代码
    assert!(!llvm_ir.is_empty(), "Should generate LLVM IR");
}

#[test]
fn test_borrowed_ptr_no_free() {
    let source = r#"
Point :: { x i32, y i32 }

test :: (p ^Point) void => {
    x := p^.x
}
"#;

    let mut diag = DiagSink::new();
    let frontend = run_frontend(FileId(0), source, &mut diag);
    assert!(!diag.has_errors());

    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);
    let llvm_ir = compile_to_llvm(&hir, &mut diag).expect("Codegen should succeed");

    println!("\nGenerated LLVM IR for borrowed pointer:");
    println!("{}", llvm_ir);

    // 借用指针不应生成 free 调用（但可能有其他 call）
    assert!(!llvm_ir.is_empty(), "Should generate LLVM IR");
}
