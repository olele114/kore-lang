//! 字符串类型 LLVM 代码生成端到端测试

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::driver::run_frontend;
use kore_stage0::middleend::lower::lower_module;
use kore_stage0::backend::llvm::compile_to_llvm;

fn compile_to_ir(source: &str) -> Option<String> {
    let mut diag = DiagSink::new();

    // 1. 前端流水线
    let frontend = run_frontend(FileId(0), source, &mut diag);
    if diag.has_errors() {
        eprintln!("Frontend errors:");
        for d in diag.peek() {
            eprintln!("  {:?}", d);
        }
        return None;
    }

    let module = frontend.module?;
    let symbols = frontend.symbols?;
    let type_ctx = frontend.type_ctx?;

    // 2. HIR 降级
    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);
    if diag.has_errors() {
        eprintln!("Lowering errors:");
        for d in diag.peek() {
            eprintln!("  {:?}", d);
        }
        return None;
    }

    // 3. LLVM 代码生成
    let result = compile_to_llvm(&hir, &mut diag);
    if diag.has_errors() {
        eprintln!("Codegen errors:");
        for d in diag.peek() {
            eprintln!("  {:?}", d);
        }
    }
    result
}

#[test]
fn codegen_string_literal() {
    let source = r#"
main :: () str => "hello"
"#;

    let ir = compile_to_ir(source).expect("codegen failed");
    eprintln!("Generated IR:\n{}", ir);

    // 验证函数签名：str 类型应该是 {ptr, len}
    assert!(ir.contains("define { ptr, i64 } @kore_main()"));

    // 验证全局字符串常量
    assert!(ir.contains(".str"));

    // 验证返回结构体
    assert!(ir.contains("ret { ptr, i64 }"));
}

#[test]
fn codegen_string_parameter() {
    let source = r#"
identity :: (s str) str => s
"#;

    let ir = compile_to_ir(source).expect("codegen failed");
    eprintln!("Generated IR:\n{}", ir);

    // 验证函数签名
    assert!(ir.contains("define { ptr, i64 } @identity({ ptr, i64 }"));

    // 验证参数传递和返回
    assert!(ir.contains("ret { ptr, i64 }"));
}

#[test]
fn codegen_string_variable() {
    let source = r#"
main :: () void => {
    ~msg := "test"
}
"#;

    let ir = compile_to_ir(source).expect("codegen failed");
    eprintln!("Generated IR:\n{}", ir);

    // 验证局部变量分配
    assert!(ir.contains("alloca { ptr, i64 }"));

    // 验证字符串赋值
    assert!(ir.contains("store { ptr, i64 }"));
}

#[test]
fn codegen_empty_string() {
    let source = r#"
empty :: () str => ""
"#;

    let ir = compile_to_ir(source).expect("codegen failed");
    eprintln!("Generated IR:\n{}", ir);

    // 验证空字符串（长度为 0）
    assert!(ir.contains("i64 0"));
}
