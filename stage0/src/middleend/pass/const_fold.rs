//! 常量折叠 Pass。
//!
//! 在编译时计算常量表达式，减少运行时计算。
//! stage0 实现简单的整数常量折叠。

use crate::middleend::hir::{BinOp, Const, HirBody, HirOperand, HirRvalue, HirStmt, UnOp};

use super::Pass;

/// 常量折叠 Pass
pub struct ConstantFolding;

impl Pass for ConstantFolding {
    fn name(&self) -> &str {
        "constant-folding"
    }

    fn run_on_body(&mut self, body: &mut HirBody) -> bool {
        let mut changed = false;

        for block in &mut body.blocks {
            for stmt in &mut block.stmts {
                if let HirStmt::Assign { rhs, .. } = stmt {
                    if let Some(folded) = try_fold_rvalue(rhs) {
                        *rhs = folded;
                        changed = true;
                    }
                }
            }
        }

        changed
    }
}

/// 尝试折叠 Rvalue
fn try_fold_rvalue(rvalue: &HirRvalue) -> Option<HirRvalue> {
    match rvalue {
        HirRvalue::BinaryOp { op, lhs, rhs } => {
            try_fold_binary_op(*op, lhs, rhs)
        }
        HirRvalue::UnaryOp { op, operand } => {
            try_fold_unary_op(*op, operand)
        }
        _ => None,
    }
}

/// 尝试折叠二元操作
fn try_fold_binary_op(
    op: BinOp,
    lhs: &HirOperand,
    rhs: &HirOperand,
) -> Option<HirRvalue> {
    // 只处理两个操作数都是常量的情况
    let lhs_val = extract_const_int(lhs)?;
    let rhs_val = extract_const_int(rhs)?;

    let result = match op {
        BinOp::Add => lhs_val.checked_add(rhs_val)?,
        BinOp::Sub => lhs_val.checked_sub(rhs_val)?,
        BinOp::Mul => lhs_val.checked_mul(rhs_val)?,
        BinOp::Div => {
            if rhs_val == 0 {
                return None; // 除零不折叠
            }
            lhs_val.checked_div(rhs_val)?
        }
        BinOp::Rem => {
            if rhs_val == 0 {
                return None;
            }
            lhs_val.checked_rem(rhs_val)?
        }
        BinOp::Eq => return Some(HirRvalue::Use(HirOperand::Const(Const::Bool(lhs_val == rhs_val)))),
        BinOp::Ne => return Some(HirRvalue::Use(HirOperand::Const(Const::Bool(lhs_val != rhs_val)))),
        BinOp::Lt => return Some(HirRvalue::Use(HirOperand::Const(Const::Bool(lhs_val < rhs_val)))),
        BinOp::Le => return Some(HirRvalue::Use(HirOperand::Const(Const::Bool(lhs_val <= rhs_val)))),
        BinOp::Gt => return Some(HirRvalue::Use(HirOperand::Const(Const::Bool(lhs_val > rhs_val)))),
        BinOp::Ge => return Some(HirRvalue::Use(HirOperand::Const(Const::Bool(lhs_val >= rhs_val)))),
        BinOp::LogicAnd => return Some(HirRvalue::Use(HirOperand::Const(Const::Bool(lhs_val != 0 && rhs_val != 0)))),
        BinOp::LogicOr => return Some(HirRvalue::Use(HirOperand::Const(Const::Bool(lhs_val != 0 || rhs_val != 0)))),
        _ => return None, // 其他操作不折叠
    };

    Some(HirRvalue::Use(HirOperand::Const(Const::Int(result))))
}

/// 尝试折叠一元操作
fn try_fold_unary_op(
    op: UnOp,
    operand: &HirOperand,
) -> Option<HirRvalue> {
    let val = extract_const_int(operand)?;

    let result = match op {
        UnOp::Neg => val.checked_neg()?,
        UnOp::Not => if val == 0 { 1 } else { 0 },
        UnOp::BitNot => !val,
    };

    Some(HirRvalue::Use(HirOperand::Const(Const::Int(result))))
}

/// 从 Operand 中提取常量整数
fn extract_const_int(operand: &HirOperand) -> Option<i128> {
    match operand {
        HirOperand::Const(Const::Int(val)) => Some(*val),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleend::hir::*;
    use crate::diag::{Span, FileId};

    #[test]
    fn test_fold_add() {
        let mut body = HirBody {
            blocks: vec![HirBlock {
                id: BlockId(0),
                stmts: vec![HirStmt::Assign {
                    lhs: HirPlace::Local(LocalId(0)),
                    rhs: HirRvalue::BinaryOp {
                        op: BinOp::Add,
                        lhs: HirOperand::Const(Const::Int(10)),
                        rhs: HirOperand::Const(Const::Int(20)),
                    },
                    span: Span::new(FileId(0), 0, 0),
                }],
                terminator: HirTerminator::Return(None),
                span: Span::new(FileId(0), 0, 0),
            }],
            locals: vec![],
            entry_block: BlockId(0),
        };

        let mut pass = ConstantFolding;
        let changed = pass.run_on_body(&mut body);

        assert!(changed);
        if let HirStmt::Assign { rhs, .. } = &body.blocks[0].stmts[0] {
            assert!(matches!(rhs, HirRvalue::Use(HirOperand::Const(Const::Int(30)))));
        }
    }

    #[test]
    fn test_fold_comparison() {
        let mut body = HirBody {
            blocks: vec![HirBlock {
                id: BlockId(0),
                stmts: vec![HirStmt::Assign {
                    lhs: HirPlace::Local(LocalId(0)),
                    rhs: HirRvalue::BinaryOp {
                        op: BinOp::Lt,
                        lhs: HirOperand::Const(Const::Int(5)),
                        rhs: HirOperand::Const(Const::Int(10)),
                    },
                    span: Span::new(FileId(0), 0, 0),
                }],
                terminator: HirTerminator::Return(None),
                span: Span::new(FileId(0), 0, 0),
            }],
            locals: vec![],
            entry_block: BlockId(0),
        };

        let mut pass = ConstantFolding;
        let changed = pass.run_on_body(&mut body);

        assert!(changed);
        if let HirStmt::Assign { rhs, .. } = &body.blocks[0].stmts[0] {
            assert!(matches!(rhs, HirRvalue::Use(HirOperand::Const(Const::Bool(true)))));
        }
    }
}
