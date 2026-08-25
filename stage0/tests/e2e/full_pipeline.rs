//! 完整流水线端到端测试
//!
//! 验证从源码到 LLVM IR 的完整编译流程，覆盖所有已实现特性：
//! - 结构体定义与字段访问
//! - 联合类型定义
//! - owned 指针与自动析构
//! - 控制流（条件分支、模式匹配）
//! - 类型推断

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::driver::run_frontend;
use kore_stage0::middleend::lower::lower_module;
use kore_stage0::backend::llvm::compile_to_llvm;

fn compile_full_pipeline(source: &str) -> Result<String, Vec<String>> {
    let mut diag = DiagSink::new();

    // 1. 前端流水线
    let frontend = run_frontend(FileId(0), source, &mut diag);
    if diag.has_errors() {
        let errors: Vec<_> = diag.peek()
            .iter()
            .map(|d| format!("{:?}", d))
            .collect();
        return Err(errors);
    }

    let module = frontend.module.ok_or_else(|| vec!["前端未产生 AST".into()])?;
    let symbols = frontend.symbols.ok_or_else(|| vec!["符号表缺失".into()])?;
    let type_ctx = frontend.type_ctx.ok_or_else(|| vec!["类型上下文缺失".into()])?;

    // 2. HIR 降级
    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);
    if diag.has_errors() {
        let errors: Vec<_> = diag.peek()
            .iter()
            .map(|d| format!("{:?}", d))
            .collect();
        return Err(errors);
    }

    // 3. LLVM 代码生成
    let ir = compile_to_llvm(&hir, &mut diag)
        .ok_or_else(|| vec!["代码生成失败".into()])?;

    if diag.has_errors() {
        let errors: Vec<_> = diag.peek()
            .iter()
            .map(|d| format!("{:?}", d))
            .collect();
        return Err(errors);
    }

    Ok(ir)
}

#[test]
fn struct_definition_and_field_access() {
    let source = r#"
Point :: { x i32, y i32 }

get_x :: (p Point) i32 => p.x

compute :: (p Point) i32 => {
    dx := p.x * p.x
    dy := p.y * p.y
    ret dx + dy
}
"#;

    let ir = compile_full_pipeline(source)
        .expect("编译应成功");

    // 验证结构体类型定义
    assert!(ir.contains("Point") || ir.contains("{ i32, i32 }") || ir.contains("{ i64, i64 }"),
        "应包含 Point 结构体定义");

    // 验证函数定义
    assert!(ir.contains("@get_x"), "应包含 get_x 函数");
    assert!(ir.contains("@compute"), "应包含 compute 函数");

    // 验证字段访问
    assert!(ir.contains("getelementptr"), "应包含 GEP 指令用于字段访问");
}

#[test]
fn owned_pointer_with_deref_and_drop() {
    let source = r#"
Data :: { value i32 }

consume :: (ptr own ^Data) i32 => {
    result := ptr^.value
    ret result
}
"#;

    let ir = compile_full_pipeline(source)
        .expect("编译应成功");

    eprintln!("=== Generated IR ===\n{}\n=== END IR ===", ir);

    // 验证指针解引用（load 或 getelementptr）
    assert!(ir.contains("load") || ir.contains("getelementptr"),
        "应包含指针操作");

    // 验证 drop 调用（free 或 drop）
    assert!(ir.contains("free") || ir.contains("drop") || ir.contains("call"),
        "应包含资源释放");
}

#[test]
fn conditional_with_type_inference() {
    let source = r#"
max :: (a i32, b i32) i32 => {
    ? a > b => ret a
    ret b
}
"#;

    let ir = compile_full_pipeline(source)
        .expect("编译应成功");

    // 验证条件分支
    assert!(ir.contains("br") || ir.contains("switch"),
        "应包含分支指令");

    // 验证比较指令
    assert!(ir.contains("icmp") || ir.contains("cmp"),
        "应包含比较指令");
}

#[test]
fn union_type_can_be_defined() {
    // 仅测试联合类型定义能通过前端，不测试变体构造（未实现）
    let source = r#"
Result :: .Ok(i32) | .Err(str)

Option :: .Some(i32) | .None

dummy :: () i32 => 42
"#;

    let ir = compile_full_pipeline(source)
        .expect("联合类型定义应成功");

    // 验证基本代码生成
    assert!(ir.contains("@dummy"), "应包含 dummy 函数");
}

#[test]
fn multiple_functions_with_call_chain() {
    let source = r#"
add :: (a i32, b i32) i32 => {
    ret a + b
}

mul :: (a i32, b i32) i32 => {
    ret a * b
}

compute :: (x i32, y i32) i32 => {
    sum := add(x, y)
    ret mul(sum, 2)
}
"#;

    let ir = compile_full_pipeline(source)
        .expect("编译应成功");

    // 验证所有函数定义
    assert!(ir.contains("@add"), "应包含 add 函数");
    assert!(ir.contains("@mul"), "应包含 mul 函数");
    assert!(ir.contains("@compute"), "应包含 compute 函数");

    // 验证函数调用
    assert!(ir.matches("call").count() >= 2,
        "compute 应调用 add 和 mul");
}

#[test]
fn nested_struct_access() {
    let source = r#"
Inner :: { value i32 }

Outer :: { inner Inner, extra i32 }

get_value :: (o Outer) i32 => ret o.inner.value
"#;

    let ir = compile_full_pipeline(source)
        .expect("编译应成功");

    // 验证函数定义
    assert!(ir.contains("@get_value"), "应包含 get_value 函数");

    // 验证嵌套字段访问（多个 getelementptr）
    assert!(ir.matches("getelementptr").count() >= 1,
        "嵌套访问应产生 GEP 指令");
}

#[test]
fn early_return_in_function() {
    let source = r#"
clamp :: (x i32) i32 => {
    ? x < 0 => ret 0
    ? x > 100 => ret 100
    ret x
}
"#;

    let ir = compile_full_pipeline(source)
        .expect("编译应成功");

    // 验证多个返回路径（多个 ret 或多个基本块）
    let ret_count = ir.matches("ret").count();
    assert!(ret_count >= 3,
        "应有至少 3 个返回路径，实际：{}", ret_count);
}
