//! 检查生成的 HIR 中 Drop 语句的位置和顺序

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::driver::run_frontend;
use kore_stage0::middleend::lower::lower_module;
use kore_stage0::middleend::hir::HirStmt;

#[test]
fn inspect_drop_order() {
    let source = r#"
Point :: { x i32, y i32 }

test :: (p1 own ^Point, p2 own ^Point) void => {
    a := p1^.x
    b := p2^.y
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

    // 打印所有语句
    println!("\nGenerated HIR for test function:");
    for (bid, block) in test_func.body.as_ref().unwrap().blocks.iter().enumerate() {
        println!("  Block {}:", bid);
        for (sid, stmt) in block.stmts.iter().enumerate() {
            match stmt {
                HirStmt::Drop { place, .. } => {
                    println!("    [{:2}] Drop {:?}", sid, place);
                }
                HirStmt::Assign { lhs, .. } => {
                    println!("    [{:2}] Assign to {:?}", sid, lhs);
                }
                _ => println!("    [{:2}] {:?}", sid, stmt),
            }
        }
        println!("    Terminator: {:?}", block.terminator);
    }

    // Drop 语句应该在返回之前，且顺序是 p2, p1（逆序）
    let drops: Vec<_> = test_func.body.as_ref().unwrap().blocks[0].stmts.iter()
        .filter_map(|stmt| match stmt {
            HirStmt::Drop { place, .. } => Some(place),
            _ => None,
        })
        .collect();

    assert_eq!(drops.len(), 2, "Should have exactly 2 Drop statements");
}
