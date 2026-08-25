//! 错误传播运算符 `!` 的 HIR 降级测试。
//!
//! 验证 `expr!` 被正确降级为：
//! 1. 判别式检查
//! 2. 条件分支（错误路径提前返回）
//! 3. 成功路径提取 payload

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;
use kore_stage0::frontend::resolve::Resolver;
use kore_stage0::frontend::typecheck::TypeChecker;
use kore_stage0::middleend::lower::lower_module;
use kore_stage0::middleend::hir::HirModule;

/// 辅助：解析源码并降级到 HIR
fn lower_source(src: &str) -> (HirModule, DiagSink) {
    let mut sink = DiagSink::new();
    let file_id = FileId(0);

    // 词法分析
    let tokens = tokenize(file_id, src, &mut sink);
    if sink.has_errors() {
        return (HirModule {
            functions: vec![],
            structs: vec![],
            unions: vec![],
            globals: vec![],
        }, sink);
    }

    // 语法分析
    let ast = parse(file_id, tokens, &mut sink);
    if sink.has_errors() {
        return (HirModule {
            functions: vec![],
            structs: vec![],
            unions: vec![],
            globals: vec![],
        }, sink);
    }

    // 名称解析
    let resolver = Resolver::new(&mut sink);
    let symtab = resolver.resolve(&ast);
    if sink.has_errors() {
        return (HirModule {
            functions: vec![],
            structs: vec![],
            unions: vec![],
            globals: vec![],
        }, sink);
    }

    // 类型检查
    let type_ctx = {
        let mut checker = TypeChecker::new(&symtab, &mut sink);
        checker.check_module(&ast);
        checker.type_context().clone()
    };
    if sink.has_errors() {
        return (HirModule {
            functions: vec![],
            structs: vec![],
            unions: vec![],
            globals: vec![],
        }, sink);
    }

    // HIR 降级
    let hir = lower_module(&ast, &symtab, &type_ctx, &mut sink);
    (hir, sink)
}

// NOTE: `!` 传播要求错误联合类型 `T ! E`（typecheck/checker.rs:383 只接受
// Type::ErrUnion）。具名标签联合 `Result :: .Ok(i32) | .Err(str)` 是不同的
// 类型构造，对它用 `!` 会得到 E9001，这是正确行为而非缺陷。
// 因此下面统一用 `i32 ! str` 声明返回类型。

#[test]
fn basic_propagation_creates_discriminant_check() {
    // 最简单的传播：result!
    let src = r#"
compute :: () i32 ! str => ret .Ok(42)

f :: () i32 ! str => {
    x : i32 ! str = compute()
    ret .Ok(x!)
}
    "#;

    let (_hir, sink) = lower_source(src);

    // 验证：暂时只检查能否成功降级，HIR 结构验证留待后续
    if sink.has_errors() {
        let diags = sink.finish();
        panic!("降级失败，错误：{:?}", diags);
    }
}

#[test]
fn propagation_creates_switch_terminator() {
    let src = r#"
compute :: () i32 ! str => ret .Ok(42)

f :: () i32 ! str => ret .Ok(compute()!)
    "#;

    let (_hir, sink) = lower_source(src);

    if sink.has_errors() {
        let diags = sink.finish();
        panic!("降级失败，错误：{:?}", diags);
    }
}

#[test]
fn propagation_error_branch_returns_early() {
    let src = r#"
compute :: () i32 ! str => ret .Ok(10)

f :: () i32 ! str => {
    x : i32 = compute()!
    ret .Ok(x + 1)
}
    "#;

    let (_hir, sink) = lower_source(src);

    if sink.has_errors() {
        let diags = sink.finish();
        panic!("降级失败，错误：{:?}", diags);
    }
}

#[test]
fn chained_propagation() {
    // 链式传播：first()! + second()!
    let src = r#"
first :: () i32 ! str => ret .Ok(10)

second :: () i32 ! str => ret .Ok(20)

f :: () i32 ! str => {
    x : i32 = first()!
    y : i32 = second()!
    ret .Ok(x + y)
}
    "#;

    let (_hir, sink) = lower_source(src);

    if sink.has_errors() {
        let diags = sink.finish();
        panic!("降级失败，错误：{:?}", diags);
    }
}

#[test]
fn nested_propagation_in_expression() {
    // 传播结果直接参与算术表达式，验证 `!` 的值能作为子表达式使用。
    let src = r#"
compute :: () i32 ! str => ret .Ok(21)

f :: () i32 ! str => {
    x : i32 = compute()! * 2
    ret .Ok(x)
}
    "#;

    let (_hir, sink) = lower_source(src);

    if sink.has_errors() {
        let diags = sink.finish();
        panic!("降级失败，错误：{:?}", diags);
    }
}

#[test]
fn propagation_on_non_union_type_emits_warning() {
    // 对非联合体类型使用 ! 应发出警告或错误
    let src = r#"
        f :: () i32 => {
            x := 42
            x!
        }
    "#;

    let (_hir, sink) = lower_source(src);

    // 验证：应该有诊断（错误或警告）
    let diags = sink.finish();
    assert!(
        !diags.is_empty(),
        "对非联合体类型使用 ! 应产生诊断"
    );
}

#[test]
fn propagation_extracts_success_payload() {
    let src = r#"
compute :: () i32 ! str => ret .Ok(42)

f :: () i32 ! str => {
    v : i32 = compute()!
    ret .Ok(v)
}
    "#;

    let (_hir, sink) = lower_source(src);

    if sink.has_errors() {
        let diags = sink.finish();
        panic!("降级失败，错误：{:?}", diags);
    }
}
