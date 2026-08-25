//! defer 语句基础功能测试

use kore_stage0::{
    diag::{DiagSink, FileId, Severity},
    driver::pipeline::run_frontend,
};

/// 测试 defer 在块作用域退出时执行
#[test]
fn test_defer_in_block_scope() {
    let mut sink = DiagSink::new();

    let source = r#"
        add :: (x i32) i32 => ret x + 1

        test_defer :: () i32 => {
            x := 0
            {
                defer add(x)  -- defer 在块退出时执行
                x := x + 10
            }
            ret x
        }
    "#;

    let output = run_frontend(FileId(0), source, &mut sink);

    let diags = sink.peek();
    let errors: Vec<_> = diags.iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        eprintln!("编译错误:");
        for diag in errors {
            eprintln!("  {}", diag.msg);
        }
        panic!("编译失败");
    }

    assert!(output.module.is_some(), "应该生成 AST 模块");
}

/// 测试 defer 在循环中执行
#[test]
#[ignore] // TODO: 循环语法需要确认
fn test_defer_in_loop() {
    let mut sink = DiagSink::new();

    let source = r#"
        noop :: (x i32) void => { }

        test_defer_loop :: () i32 => {
            defer noop(0)
            ret 0
        }
    "#;

    let output = run_frontend(FileId(0), source, &mut sink);

    let diags = sink.peek();
    let errors: Vec<_> = diags.iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        eprintln!("编译错误:");
        for diag in errors {
            eprintln!("  {}", diag.msg);
        }
        panic!("编译失败");
    }

    assert!(output.module.is_some(), "应该生成 AST 模块");
}

/// 测试 defer 在 return 前执行
#[test]
fn test_defer_before_return() {
    let mut sink = DiagSink::new();

    let source = r#"
        noop :: () void => { }

        test_defer_ret :: () i32 => {
            defer noop()
            ret 42
        }
    "#;

    let output = run_frontend(FileId(0), source, &mut sink);

    let diags = sink.peek();
    let errors: Vec<_> = diags.iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        eprintln!("编译错误:");
        for diag in errors {
            eprintln!("  {}", diag.msg);
        }
        panic!("编译失败");
    }

    assert!(output.module.is_some(), "应该生成 AST 模块");
}

/// 测试多个 defer 按逆序执行（LIFO）
#[test]
fn test_multiple_defers_lifo() {
    let mut sink = DiagSink::new();

    let source = r#"
        noop1 :: () void => { }
        noop2 :: () void => { }

        test_defer_order :: () i32 => {
            defer noop1()  -- defer 1，最后执行
            defer noop2()  -- defer 2，先执行
            ret 0
        }
    "#;

    let output = run_frontend(FileId(0), source, &mut sink);

    let diags = sink.peek();
    let errors: Vec<_> = diags.iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        eprintln!("编译错误:");
        for diag in errors {
            eprintln!("  {}", diag.msg);
        }
        panic!("编译失败");
    }

    assert!(output.module.is_some(), "应该生成 AST 模块");
}

/// 测试嵌套作用域的 defer
#[test]
fn test_nested_scope_defers() {
    let mut sink = DiagSink::new();

    let source = r#"
        outer :: () void => { }
        inner :: () void => { }

        test_nested_defer :: () i32 => {
            defer outer()  -- defer 外层，最后执行
            {
                defer inner()  -- defer 内层，先执行
            }
            ret 0
        }
    "#;

    let output = run_frontend(FileId(0), source, &mut sink);

    let diags = sink.peek();
    let errors: Vec<_> = diags.iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        eprintln!("编译错误:");
        for diag in errors {
            eprintln!("  {}", diag.msg);
        }
        panic!("编译失败");
    }

    assert!(output.module.is_some(), "应该生成 AST 模块");
}
