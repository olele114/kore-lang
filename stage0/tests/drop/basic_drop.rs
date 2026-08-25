//! 测试基本的 owned 指针自动析构

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::driver::run_frontend;
use kore_stage0::middleend::lower::lower_module;
use kore_stage0::middleend::hir::HirStmt;

#[test]
fn test_owned_ptr_drop_inserted() {
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
    }
    assert!(!diag.has_errors(), "Frontend should not have errors");

    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);

    // 查找 test 函数
    let test_func = hir.functions.iter().find(|f| f.name == "test").unwrap();

    // 检查函数体中是否有 Drop 语句
    let has_drop = test_func.body.as_ref().unwrap().blocks.iter().any(|block| {
        block.stmts.iter().any(|stmt| matches!(stmt, HirStmt::Drop { .. }))
    });

    assert!(has_drop, "Should have Drop statement for owned pointer 'p'");
}

#[test]
fn test_multiple_owned_params() {
    let source = r#"
Point :: { x i32, y i32 }

test :: (p1 own ^Point, p2 own ^Point) void => {
    x := p1^.x
    y := p2^.y
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
    assert!(!diag.has_errors(), "Frontend should not have errors");

    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);

    // 查找 test 函数
    let test_func = hir.functions.iter().find(|f| f.name == "test").unwrap();

    // 统计 Drop 语句数量
    let drop_count = test_func.body.as_ref().unwrap().blocks.iter()
        .flat_map(|block| &block.stmts)
        .filter(|stmt| matches!(stmt, HirStmt::Drop { .. }))
        .count();

    // 应该有 2 个 Drop：p1 和 p2
    assert_eq!(drop_count, 2, "Should have 2 Drop statements for p1 and p2");
}

#[test]
fn test_borrowed_ptr_no_drop() {
    let source = r#"
Point :: { x i32, y i32 }

test :: (p ^Point) void => {
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
    }
    assert!(!diag.has_errors(), "Frontend should not have errors");

    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut diag);

    // 查找 test 函数
    let test_func = hir.functions.iter().find(|f| f.name == "test").unwrap();

    // 检查是否没有 Drop 语句（borrowed 指针不需要 drop）
    let has_drop = test_func.body.as_ref().unwrap().blocks.iter().any(|block| {
        block.stmts.iter().any(|stmt| matches!(stmt, HirStmt::Drop { .. }))
    });

    assert!(!has_drop, "Should not have Drop statement for borrowed pointer 'p'");
}
