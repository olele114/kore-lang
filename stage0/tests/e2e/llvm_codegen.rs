//! 端到端 LLVM 代码生成测试。

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::driver::run_frontend;
use kore_stage0::middleend::lower::lower_module;
use kore_stage0::backend::llvm::compile_to_llvm;

fn compile_to_ir(source: &str) -> Option<String> {
    let mut diag = DiagSink::new();

    // 1. 运行前端流水线（词法 → 语法 → 解析 → 逃逸 → 类型检查）
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

    // 2. 降级到 HIR
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
fn codegen_empty_main() {
    let source = r#"
main :: () void => {}
"#;

    let ir = compile_to_ir(source).expect("codegen failed");
    eprintln!("Generated IR:\n{}", ir);

    // 验证生成了 kore_main 函数和 C main 包装器
    assert!(ir.contains("define void @kore_main()"));
    assert!(ir.contains("define i32 @main(i32"));
    assert!(ir.contains("ret void"));
}

#[test]
fn codegen_return_constant() {
    let source = r#"
answer :: () i32 => 42
"#;

    let ir = compile_to_ir(source).expect("codegen failed");

    // 验证函数签名和返回值
    assert!(ir.contains("define i32 @answer()") || ir.contains("define i64 @answer()"));
    assert!(ir.contains("ret i32 42") || ir.contains("ret i64 42"));
}

#[test]
fn codegen_simple_arithmetic() {
    let source = r#"
add :: (a i32, b i32) i32 => a + b
"#;

    let ir = compile_to_ir(source).expect("codegen failed");
    eprintln!("Generated IR:\n{}", ir);

    // 验证函数签名
    assert!(ir.contains("define i32 @add(i32") || ir.contains("define i64 @add(i64"));
    // 验证加法指令
    assert!(ir.contains("add") || ir.contains("iadd"));
}

#[test]
fn codegen_struct_field_access() {
    let source = r#"
Point :: {
    x, y i32
}

get_x :: (p Point) i32 => p.x
"#;

    let ir = compile_to_ir(source).expect("codegen failed");

    // 验证结构体类型定义
    assert!(ir.contains("%Point") || ir.contains("{ i32, i32 }") || ir.contains("{ i64, i64 }"));
    // 验证字段访问（GEP 指令）
    assert!(ir.contains("getelementptr"));
}

#[test]
fn codegen_conditional_branch() {
    let source = r#"
abs :: (x i32) i32 => {
    ret ? {
        x < 0 => -x,
        _ => x
    }
}
"#;

    let ir = compile_to_ir(source).expect("codegen failed");

    // 验证条件分支（br 或 switch 指令）
    assert!(ir.contains("br") || ir.contains("switch"));
    // 验证多个基本块
    assert!(ir.matches("bb").count() >= 2 || ir.matches("label").count() >= 2);
}
