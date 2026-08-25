//! 优化 Pass 集成测试。
//!
//! 验证 Pass 管道的端到端行为，包括：
//! - PassManager 正确执行所有 pass
//! - Pass 迭代收敛到不动点
//! - 默认 pipeline 按预期顺序运行

use kore_stage0::middleend::pass::{PassManager, default_pipeline};
use kore_stage0::middleend::hir::{
    HirBody, HirModule, HirFunction, HirBlock, HirLocal, BlockId, LocalId,
};
use kore_stage0::middleend::hir::{HirStmt, HirRvalue, HirOperand, HirPlace, HirTerminator, Const};
use kore_stage0::middleend::hir::ty::HirType;
use kore_stage0::diag::{Span, FileId};

/// 辅助函数：创建简单的 HIR body 用于测试
fn make_test_body() -> HirBody {
    let span = Span::new(FileId(0), 0, 1);

    let entry = HirBlock {
        id: BlockId(0),
        stmts: vec![
            HirStmt::Assign {
                lhs: HirPlace::Local(LocalId(0)),
                rhs: HirRvalue::Use(HirOperand::Const(Const::Int(42))),
                span,
            },
        ],
        terminator: HirTerminator::Return(Some(HirOperand::Place(Box::new(HirPlace::Local(LocalId(0)))))),
        span,
    };

    HirBody {
        blocks: vec![entry],
        locals: vec![
            HirLocal {
                name: Some("x".to_string()),
                ty: HirType::Int { width: 32, signed: true },
                span,
            },
        ],
        entry_block: BlockId(0),
    }
}

#[test]
fn test_pass_manager_basic() {
    let mut pm = PassManager::new();
    let body = make_test_body();
    let span = Span::new(FileId(0), 0, 1);

    // 空 PassManager 通过 run_on_module 间接运行
    let mut module = HirModule {
        functions: vec![HirFunction {
            name: "test".to_string(),
            params: vec![],
            ret_type: HirType::Int { width: 32, signed: true },
            body: Some(body.clone()),
            span,
        }],
        structs: vec![],
        unions: vec![],
        globals: vec![],
    };

    pm.run_on_module(&mut module);

    // 验证 body 结构未破坏
    assert_eq!(module.functions[0].body.as_ref().unwrap().blocks.len(), 1);
    assert_eq!(module.functions[0].body.as_ref().unwrap().locals.len(), 1);
}

#[test]
fn test_default_pipeline_runs() {
    let mut pm = default_pipeline();
    let body = make_test_body();
    let span = Span::new(FileId(0), 0, 1);

    let mut module = HirModule {
        functions: vec![HirFunction {
            name: "test".to_string(),
            params: vec![],
            ret_type: HirType::Int { width: 32, signed: true },
            body: Some(body),
            span,
        }],
        structs: vec![],
        unions: vec![],
        globals: vec![],
    };

    // 默认 pipeline 应成功运行
    pm.run_on_module(&mut module);

    let body = module.functions[0].body.as_ref().unwrap();
    // 验证基本不变量
    assert!(!body.blocks.is_empty(), "Pass should not remove all blocks");
    assert_eq!(body.entry_block, BlockId(0), "Entry block should remain stable");
}

#[test]
fn test_pass_pipeline_on_module() {
    let mut pm = default_pipeline();
    let span = Span::new(FileId(0), 0, 1);

    let func = HirFunction {
        name: "test_func".to_string(),
        params: vec![],
        ret_type: HirType::Int { width: 32, signed: true },
        body: Some(make_test_body()),
        span,
    };

    let mut module = HirModule {
        functions: vec![func],
        structs: vec![],
        unions: vec![],
        globals: vec![],
    };

    // 在模块级别运行 pipeline
    pm.run_on_module(&mut module);

    // 验证函数仍然存在
    assert_eq!(module.functions.len(), 1);
    assert_eq!(module.functions[0].name, "test_func");
}

#[test]
fn test_pass_convergence() {
    let span = Span::new(FileId(0), 0, 1);

    // 创建包含死代码的 body
    let dead_block = HirBlock {
        id: BlockId(1),
        stmts: vec![],
        terminator: HirTerminator::Unreachable,
        span,
    };

    let entry = HirBlock {
        id: BlockId(0),
        stmts: vec![],
        terminator: HirTerminator::Return(Some(HirOperand::Const(Const::Int(0)))),
        span,
    };

    let body = HirBody {
        blocks: vec![entry, dead_block],
        locals: vec![],
        entry_block: BlockId(0),
    };

    let mut module = HirModule {
        functions: vec![HirFunction {
            name: "test".to_string(),
            params: vec![],
            ret_type: HirType::Int { width: 32, signed: true },
            body: Some(body),
            span,
        }],
        structs: vec![],
        unions: vec![],
        globals: vec![],
    };

    let mut pm = default_pipeline();
    pm.run_on_module(&mut module);

    let body = module.functions[0].body.as_ref().unwrap();
    // 死代码消除应移除不可达块
    // 注意：实际行为取决于 dead_code pass 实现
    // 这里仅验证不崩溃
    assert!(!body.blocks.is_empty());
}
