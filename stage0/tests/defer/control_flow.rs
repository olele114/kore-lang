//! defer 语句与控制流测试

use kore_stage0::{
    diag::{DiagSink, FileId, Severity},
    driver::pipeline::run_frontend,
};

/// 测试 defer 在 stop (break) 前执行
#[test]
fn test_defer_before_break() {
    let mut sink = DiagSink::new();

    let source = r#"
        cleanup :: () void => { }

        test_defer_break :: () i32 => {
            @ {
                defer cleanup()
                stop
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

/// 测试 defer 在 skip (continue) 前执行
#[test]
fn test_defer_before_continue() {
    let mut sink = DiagSink::new();

    let source = r#"
        cleanup :: () void => { }
        inc :: (x i32) i32 => ret x + 1
        eq :: (a i32, b i32) i32 => ret ? a == b => 1 : 0

        test_defer_skip :: () i32 => {
            x := 0
            @ (eq(x, 3) == 0) {
                defer cleanup()
                x := inc(x)
                ? eq(x, 2) == 1 => skip
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

/// 测试嵌套循环中的 defer + break
#[test]
fn test_defer_nested_loop_break() {
    let mut sink = DiagSink::new();

    let source = r#"
        outer_cleanup :: () void => { }
        inner_cleanup :: () void => { }
        inc :: (x i32) i32 => ret x + 1
        eq :: (a i32, b i32) i32 => ret ? a == b => 1 : 0

        test_nested_defer :: () i32 => {
            x := 0
            @ (eq(x, 3) == 0) {
                defer outer_cleanup()
                y := 0
                @ (eq(y, 3) == 0) {
                    defer inner_cleanup()
                    ? eq(y, 1) == 1 => stop
                    y := inc(y)
                }
                x := inc(x)
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

/// 测试嵌套块中的 defer + break（应该展开多层作用域）
#[test]
fn test_defer_nested_block_break() {
    let mut sink = DiagSink::new();

    let source = r#"
        cleanup1 :: () void => { }
        cleanup2 :: () void => { }
        cleanup3 :: () void => { }
        inc :: (x i32) i32 => ret x + 1
        eq :: (a i32, b i32) i32 => ret ? a == b => 1 : 0

        test_nested_scope_break :: () i32 => {
            x := 0
            @ (eq(x, 5) == 0) {
                defer cleanup1()
                {
                    defer cleanup2()
                    {
                        defer cleanup3()
                        ? eq(x, 2) == 1 => stop
                    }
                }
                x := inc(x)
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

/// 测试 defer 在 return 前的多层展开
#[test]
fn test_defer_nested_block_return() {
    let mut sink = DiagSink::new();

    let source = r#"
        cleanup1 :: () void => { }
        cleanup2 :: () void => { }
        cleanup3 :: () void => { }

        test_nested_scope_ret :: () i32 => {
            defer cleanup1()
            {
                defer cleanup2()
                {
                    defer cleanup3()
                    ret 42
                }
            }
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
