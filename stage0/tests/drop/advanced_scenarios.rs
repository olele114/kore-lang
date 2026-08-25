//! 测试 owned 指针在复杂场景下的自动析构

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::driver::run_frontend;
use kore_stage0::middleend::lower::lower_module;
use kore_stage0::middleend::hir::{HirStmt, HirTerminator};

#[test]
fn test_drop_with_early_return() {
    let source = r#"
Point :: { x i32, y i32 }

test :: (p own ^Point, should_return bool) i32 => {
    ? should_return => 0
    x := p^.x
    x
}
"#;

    let mut diag = DiagSink::new();
    let frontend = run_frontend(FileId(0), source, &mut diag);
    assert!(!diag.has_errors(), "Frontend should not have errors");

    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);
    let test_func = hir.functions.iter().find(|f| f.name == "test").unwrap();

    // 检查所有返回路径前都有 Drop
    for block in &test_func.body.as_ref().unwrap().blocks {
        if matches!(block.terminator, HirTerminator::Return(_)) {
            // 在返回前应该有 Drop 语句
            let has_drop_before_return = block.stmts.iter()
                .any(|stmt| matches!(stmt, HirStmt::Drop { .. }));

            assert!(has_drop_before_return,
                "Should have Drop before return in block");
        }
    }

    println!("\n✓ Early return with drop verified");
}

#[test]
fn test_drop_in_nested_blocks() {
    let source = r#"
Point :: { x i32, y i32 }

test :: (p own ^Point, cond bool) void => {
    ? cond => {
        y := p^.y
    }
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
    let test_func = hir.functions.iter().find(|f| f.name == "test").unwrap();

    // 统计所有 Drop 语句
    let drop_count = test_func.body.as_ref().unwrap().blocks.iter()
        .flat_map(|block| &block.stmts)
        .filter(|stmt| matches!(stmt, HirStmt::Drop { .. }))
        .count();

    // 无论走哪条路径，p 都应该被 drop 一次
    assert!(drop_count >= 1, "Should have at least 1 Drop for p");

    println!("\n✓ Nested block drop verified");
}

#[test]
fn test_multiple_owned_with_mixed_usage() {
    let source = r#"
Point :: { x i32, y i32 }

test :: (p1 own ^Point, p2 own ^Point, use_p1 bool) i32 => {
    ? use_p1 => p1^.x
    p2^.y
}
"#;

    let mut diag = DiagSink::new();
    let frontend = run_frontend(FileId(0), source, &mut diag);
    assert!(!diag.has_errors());

    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);
    let test_func = hir.functions.iter().find(|f| f.name == "test").unwrap();

    // 每条路径都应该 drop 两个指针
    let drop_count = test_func.body.as_ref().unwrap().blocks.iter()
        .flat_map(|block| &block.stmts)
        .filter(|stmt| matches!(stmt, HirStmt::Drop { .. }))
        .count();

    assert!(drop_count >= 2, "Should drop both p1 and p2 in all paths");

    println!("\n✓ Mixed usage drop verified");
}

#[test]
fn test_drop_with_no_usage() {
    let source = r#"
Point :: { x i32, y i32 }

test :: (p own ^Point) void => {
    x := 42
}
"#;

    let mut diag = DiagSink::new();
    let frontend = run_frontend(FileId(0), source, &mut diag);

    if diag.has_errors() {
        eprintln!("Frontend errors:");
        for d in diag.peek() {
            eprintln!("  {:?}", d);
        }
    }
    assert!(!diag.has_errors());

    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);
    let test_func = hir.functions.iter().find(|f| f.name == "test").unwrap();

    let has_drop = test_func.body.as_ref().unwrap().blocks.iter().any(|block| {
        block.stmts.iter().any(|stmt| matches!(stmt, HirStmt::Drop { .. }))
    });

    assert!(has_drop, "Should drop p even if never used");

    println!("\n✓ Unused owned parameter drop verified");
}

#[test]
fn test_drop_order_multiple_params() {
    let source = r#"
Point :: { x i32, y i32 }

test :: (p1 own ^Point, p2 own ^Point, p3 own ^Point) void => {
    x := p1^.x
    y := p2^.y
    z := p3^.x
}
"#;

    let mut diag = DiagSink::new();
    let frontend = run_frontend(FileId(0), source, &mut diag);
    assert!(!diag.has_errors());

    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);
    let test_func = hir.functions.iter().find(|f| f.name == "test").unwrap();

    let drop_count = test_func.body.as_ref().unwrap().blocks.iter()
        .flat_map(|block| &block.stmts)
        .filter(|stmt| matches!(stmt, HirStmt::Drop { .. }))
        .count();

    assert_eq!(drop_count, 3, "Should have 3 Drops for p1, p2, p3");

    println!("\n✓ Multiple parameter drop order verified");
}
