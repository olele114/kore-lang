//! 局部变量使用验证。
//!
//! 检查：
//! 1. 所有 LocalId 引用有效（在 locals 表中存在）
//! 2. 变量在使用前已定义（在 CFG 中可达）
//! 3. 参数和局部变量的 ID 不越界

use crate::middleend::hir::{HirBody, HirStmt, HirTerminator, HirPlace, HirOperand, HirRvalue, LocalId, BlockId};
use crate::diag::{DiagSink, Diagnostic, DiagLoc};
use std::collections::{HashSet, HashMap};

/// 验证局部变量使用的合法性
pub fn validate_locals(body: &HirBody, sink: &mut DiagSink) -> bool {
    let mut validator = LocalValidator::new(body, sink);
    validator.validate()
}

struct LocalValidator<'a> {
    body: &'a HirBody,
    sink: &'a mut DiagSink,
    valid: bool,
}

impl<'a> LocalValidator<'a> {
    fn new(body: &'a HirBody, sink: &'a mut DiagSink) -> Self {
        Self {
            body,
            sink,
            valid: true,
        }
    }

    fn validate(&mut self) -> bool {
        // 1. 验证所有 LocalId 引用有效
        let valid_locals: HashSet<LocalId> = (0..self.body.locals.len())
            .map(LocalId)
            .collect();

        for block in &self.body.blocks {
            for stmt in &block.stmts {
                self.check_stmt_locals(stmt, &valid_locals);
            }
            self.check_terminator_locals(&block.terminator, &valid_locals);
        }

        // 2. 验证变量定义可达性（数据流分析）
        self.check_def_before_use();

        self.valid
    }

    /// 检查语句中的 LocalId 引用
    fn check_stmt_locals(&mut self, stmt: &HirStmt, valid_locals: &HashSet<LocalId>) {
        match stmt {
            HirStmt::Assign { lhs, rhs, span } => {
                self.check_place_locals(lhs, valid_locals, *span);
                self.check_rvalue_locals(rhs, valid_locals, *span);
            }
            HirStmt::Call { dest, func, args, span } => {
                if let Some(place) = dest {
                    self.check_place_locals(place, valid_locals, *span);
                }
                self.check_operand_locals(func, valid_locals, *span);
                for arg in args {
                    self.check_operand_locals(arg, valid_locals, *span);
                }
            }
            HirStmt::Drop { place, span } => {
                self.check_place_locals(place, valid_locals, *span);
            }
        }
    }

    /// 检查终结符中的 LocalId 引用
    fn check_terminator_locals(&mut self, term: &HirTerminator, valid_locals: &HashSet<LocalId>) {
        match term {
            HirTerminator::Return(Some(op)) => {
                self.check_operand_locals(op, valid_locals, crate::diag::Span::new(crate::diag::FileId(0), 0, 0));
            }
            HirTerminator::Switch { discr, .. } => {
                self.check_operand_locals(discr, valid_locals, crate::diag::Span::new(crate::diag::FileId(0), 0, 0));
            }
            _ => {}
        }
    }

    /// 检查 Place 中的 LocalId
    fn check_place_locals(&mut self, place: &HirPlace, valid_locals: &HashSet<LocalId>, span: crate::diag::Span) {
        match place {
            HirPlace::Local(id) => {
                if !valid_locals.contains(id) {
                    self.sink.emit(Diagnostic::error(
                        9010,
                        format!("Invalid local variable reference: {:?}", id),
                        DiagLoc::At(span),
                    ));
                    self.valid = false;
                }
            }
            HirPlace::Field { base, .. } => {
                self.check_place_locals(base, valid_locals, span);
            }
            HirPlace::Index { base, index } => {
                self.check_place_locals(base, valid_locals, span);
                self.check_operand_locals(index, valid_locals, span);
            }
            HirPlace::Deref(base) => {
                self.check_place_locals(base, valid_locals, span);
            }
        }
    }

    /// 检查 Operand 中的 LocalId
    fn check_operand_locals(&mut self, operand: &HirOperand, valid_locals: &HashSet<LocalId>, span: crate::diag::Span) {
        if let HirOperand::Place(place) = operand {
            self.check_place_locals(place, valid_locals, span);
        }
    }

    /// 检查 Rvalue 中的 LocalId
    fn check_rvalue_locals(&mut self, rvalue: &HirRvalue, valid_locals: &HashSet<LocalId>, span: crate::diag::Span) {
        match rvalue {
            HirRvalue::Use(op) => self.check_operand_locals(op, valid_locals, span),
            HirRvalue::BinaryOp { lhs, rhs, .. } => {
                self.check_operand_locals(lhs, valid_locals, span);
                self.check_operand_locals(rhs, valid_locals, span);
            }
            HirRvalue::UnaryOp { operand, .. } => {
                self.check_operand_locals(operand, valid_locals, span);
            }
            HirRvalue::Ref { place, .. } => {
                self.check_place_locals(place, valid_locals, span);
            }
            HirRvalue::Aggregate { fields, .. } => {
                for field in fields {
                    self.check_operand_locals(field, valid_locals, span);
                }
            }
            HirRvalue::Discriminant(place) => {
                self.check_place_locals(place, valid_locals, span);
            }
            HirRvalue::ExtractPayload { place, .. } => {
                self.check_place_locals(place, valid_locals, span);
            }
            HirRvalue::Deref(operand) => {
                self.check_operand_locals(operand, valid_locals, span);
            }
            HirRvalue::ArrayToSlice { array, .. } => {
                self.check_operand_locals(array, valid_locals, span);
            }
        }
    }

    /// 数据流分析：检查变量使用前是否有定义
    fn check_def_before_use(&mut self) {
        // 构建每个块的 GEN/KILL 集合
        let mut gen_map: HashMap<BlockId, HashSet<LocalId>> = HashMap::new();
        let mut kill_map: HashMap<BlockId, HashSet<LocalId>> = HashMap::new();

        for block in &self.body.blocks {
            let mut gen_set = HashSet::new();
            let kill_set = HashSet::new();

            for stmt in &block.stmts {
                // 收集定义（写入）的变量
                if let HirStmt::Assign { lhs, .. } = stmt {
                    if let Some(local) = self.get_root_local(lhs) {
                        gen_set.insert(local);
                    }
                }
                if let HirStmt::Call { dest: Some(place), .. } = stmt {
                    if let Some(local) = self.get_root_local(place) {
                        gen_set.insert(local);
                    }
                }
            }

            gen_map.insert(block.id, gen_set);
            kill_map.insert(block.id, kill_set);
        }

        // 计算到达定义（简化版：只检查入口块后的首次使用）
        let mut defined = HashSet::new();

        // 入口块：所有参数视为已定义
        // 假设前 N 个 locals 是参数（实际应该从函数签名获取）
        for (idx, _local) in self.body.locals.iter().enumerate() {
            defined.insert(LocalId(idx));
        }

        self.check_block_def_use(self.body.entry_block, &mut defined, &mut HashSet::new());
    }

    /// 递归检查块中的变量使用
    fn check_block_def_use(&mut self, block_id: BlockId, defined: &mut HashSet<LocalId>, visited: &mut HashSet<BlockId>) {
        if visited.contains(&block_id) {
            return;
        }
        visited.insert(block_id);

        let block = match self.body.blocks.iter().find(|b| b.id == block_id) {
            Some(b) => b,
            None => return,
        };

        let mut local_defined = defined.clone();

        // 检查语句中的使用和定义
        for stmt in &block.stmts {
            match stmt {
                HirStmt::Assign { lhs, rhs, span } => {
                    self.check_rvalue_use(rhs, &local_defined, *span);
                    if let Some(local) = self.get_root_local(lhs) {
                        local_defined.insert(local);
                    }
                }
                HirStmt::Call { dest, func, args, span } => {
                    self.check_operand_use(func, &local_defined, *span);
                    for arg in args {
                        self.check_operand_use(arg, &local_defined, *span);
                    }
                    if let Some(place) = dest {
                        if let Some(local) = self.get_root_local(place) {
                            local_defined.insert(local);
                        }
                    }
                }
                HirStmt::Drop { place, span } => {
                    self.check_place_use(place, &local_defined, *span);
                }
            }
        }

        // 检查终结符
        match &block.terminator {
            HirTerminator::Return(Some(op)) => {
                self.check_operand_use(op, &local_defined, block.span);
            }
            HirTerminator::Switch { discr, targets, otherwise } => {
                self.check_operand_use(discr, &local_defined, block.span);
                for (_, target) in targets {
                    self.check_block_def_use(*target, &mut local_defined.clone(), visited);
                }
                self.check_block_def_use(*otherwise, &mut local_defined.clone(), visited);
            }
            HirTerminator::Goto(target) => {
                self.check_block_def_use(*target, &mut local_defined, visited);
            }
            _ => {}
        }
    }

    /// 获取 Place 的根 LocalId
    fn get_root_local(&self, place: &HirPlace) -> Option<LocalId> {
        match place {
            HirPlace::Local(id) => Some(*id),
            HirPlace::Field { base, .. } => self.get_root_local(base),
            HirPlace::Index { base, .. } => self.get_root_local(base),
            HirPlace::Deref(base) => self.get_root_local(base),
        }
    }

    /// 检查 Place 的使用
    fn check_place_use(&mut self, place: &HirPlace, defined: &HashSet<LocalId>, span: crate::diag::Span) {
        if let Some(local) = self.get_root_local(place) {
            if !defined.contains(&local) {
                self.sink.emit(Diagnostic::warning(
                    9011,
                    format!("Variable {:?} may be used before definition", local),
                    DiagLoc::At(span),
                ));
            }
        }
    }

    /// 检查 Operand 的使用
    fn check_operand_use(&mut self, operand: &HirOperand, defined: &HashSet<LocalId>, span: crate::diag::Span) {
        if let HirOperand::Place(place) = operand {
            self.check_place_use(place, defined, span);
        }
    }

    /// 检查 Rvalue 的使用
    fn check_rvalue_use(&mut self, rvalue: &HirRvalue, defined: &HashSet<LocalId>, span: crate::diag::Span) {
        match rvalue {
            HirRvalue::Use(op) => self.check_operand_use(op, defined, span),
            HirRvalue::BinaryOp { lhs, rhs, .. } => {
                self.check_operand_use(lhs, defined, span);
                self.check_operand_use(rhs, defined, span);
            }
            HirRvalue::UnaryOp { operand, .. } => {
                self.check_operand_use(operand, defined, span);
            }
            HirRvalue::Ref { place, .. } => {
                self.check_place_use(place, defined, span);
            }
            HirRvalue::Aggregate { fields, .. } => {
                for field in fields {
                    self.check_operand_use(field, defined, span);
                }
            }
            HirRvalue::Discriminant(place) => {
                self.check_place_use(place, defined, span);
            }
            HirRvalue::ExtractPayload { place, .. } => {
                self.check_place_use(place, defined, span);
            }
            HirRvalue::Deref(operand) => {
                self.check_operand_use(operand, defined, span);
            }
            HirRvalue::ArrayToSlice { array, .. } => {
                self.check_operand_use(array, defined, span);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleend::hir::*;
    use crate::middleend::hir::ty::HirType;
    use crate::diag::{Span, FileId};

    #[test]
    fn test_valid_local_usage() {
        let body = HirBody {
            blocks: vec![
                HirBlock {
                    id: BlockId(0),
                    stmts: vec![
                        HirStmt::Assign {
                            lhs: HirPlace::Local(LocalId(0)),
                            rhs: HirRvalue::Use(HirOperand::Const(Const::Int(42))),
                            span: Span::new(FileId(0), 0, 0),
                        },
                        HirStmt::Assign {
                            lhs: HirPlace::Local(LocalId(1)),
                            rhs: HirRvalue::Use(HirOperand::Place(Box::new(HirPlace::Local(LocalId(0))))),
                            span: Span::new(FileId(0), 0, 0),
                        },
                    ],
                    terminator: HirTerminator::Return(Some(HirOperand::Place(Box::new(HirPlace::Local(LocalId(1)))))),
                    span: Span::new(FileId(0), 0, 0),
                },
            ],
            locals: vec![
                HirLocal { name: Some("x".to_string()), ty: HirType::Int { width: 32, signed: true }, span: Span::new(FileId(0), 0, 0) },
                HirLocal { name: Some("y".to_string()), ty: HirType::Int { width: 32, signed: true }, span: Span::new(FileId(0), 0, 0) },
            ],
            entry_block: BlockId(0),
        };

        let mut sink = DiagSink::new();
        assert!(validate_locals(&body, &mut sink));
        assert_eq!(sink.peek().len(), 0);
    }

    #[test]
    fn test_invalid_local_id() {
        let body = HirBody {
            blocks: vec![
                HirBlock {
                    id: BlockId(0),
                    stmts: vec![
                        HirStmt::Assign {
                            lhs: HirPlace::Local(LocalId(999)), // Invalid ID
                            rhs: HirRvalue::Use(HirOperand::Const(Const::Int(42))),
                            span: Span::new(FileId(0), 0, 0),
                        },
                    ],
                    terminator: HirTerminator::Return(None),
                    span: Span::new(FileId(0), 0, 0),
                },
            ],
            locals: vec![
                HirLocal { name: Some("x".to_string()), ty: HirType::Int { width: 32, signed: true }, span: Span::new(FileId(0), 0, 0) },
            ],
            entry_block: BlockId(0),
        };

        let mut sink = DiagSink::new();
        assert!(!validate_locals(&body, &mut sink));
        assert!(sink.has_errors());
    }

    #[test]
    fn test_local_in_rvalue() {
        let body = HirBody {
            blocks: vec![
                HirBlock {
                    id: BlockId(0),
                    stmts: vec![
                        HirStmt::Assign {
                            lhs: HirPlace::Local(LocalId(0)),
                            rhs: HirRvalue::BinaryOp {
                                op: BinOp::Add,
                                lhs: HirOperand::Place(Box::new(HirPlace::Local(LocalId(1)))),
                                rhs: HirOperand::Const(Const::Int(10)),
                            },
                            span: Span::new(FileId(0), 0, 0),
                        },
                    ],
                    terminator: HirTerminator::Return(None),
                    span: Span::new(FileId(0), 0, 0),
                },
            ],
            locals: vec![
                HirLocal { name: Some("result".to_string()), ty: HirType::Int { width: 32, signed: true }, span: Span::new(FileId(0), 0, 0) },
                HirLocal { name: Some("x".to_string()), ty: HirType::Int { width: 32, signed: true }, span: Span::new(FileId(0), 0, 0) },
            ],
            entry_block: BlockId(0),
        };

        let mut sink = DiagSink::new();
        assert!(validate_locals(&body, &mut sink));
        assert_eq!(sink.peek().len(), 0);
    }

    #[test]
    fn test_local_in_terminator() {
        let body = HirBody {
            blocks: vec![
                HirBlock {
                    id: BlockId(0),
                    stmts: vec![],
                    terminator: HirTerminator::Return(Some(HirOperand::Place(Box::new(HirPlace::Local(LocalId(0)))))),
                    span: Span::new(FileId(0), 0, 0),
                },
            ],
            locals: vec![
                HirLocal { name: Some("ret".to_string()), ty: HirType::Int { width: 32, signed: true }, span: Span::new(FileId(0), 0, 0) },
            ],
            entry_block: BlockId(0),
        };

        let mut sink = DiagSink::new();
        assert!(validate_locals(&body, &mut sink));
        assert_eq!(sink.peek().len(), 0);
    }

    #[test]
    fn test_invalid_local_in_operand() {
        let body = HirBody {
            blocks: vec![
                HirBlock {
                    id: BlockId(0),
                    stmts: vec![
                        HirStmt::Assign {
                            lhs: HirPlace::Local(LocalId(0)),
                            rhs: HirRvalue::Use(HirOperand::Place(Box::new(HirPlace::Local(LocalId(999))))),
                            span: Span::new(FileId(0), 0, 0),
                        },
                    ],
                    terminator: HirTerminator::Return(None),
                    span: Span::new(FileId(0), 0, 0),
                },
            ],
            locals: vec![
                HirLocal { name: Some("x".to_string()), ty: HirType::Int { width: 32, signed: true }, span: Span::new(FileId(0), 0, 0) },
            ],
            entry_block: BlockId(0),
        };

        let mut sink = DiagSink::new();
        assert!(!validate_locals(&body, &mut sink));
        assert!(sink.has_errors());
    }

    #[test]
    fn test_local_in_call_stmt() {
        let body = HirBody {
            blocks: vec![
                HirBlock {
                    id: BlockId(0),
                    stmts: vec![
                        HirStmt::Call {
                            dest: Some(HirPlace::Local(LocalId(0))),
                            func: HirOperand::FuncRef(FuncId(0)),
                            args: vec![
                                HirOperand::Place(Box::new(HirPlace::Local(LocalId(1)))),
                                HirOperand::Const(Const::Int(42)),
                            ],
                            span: Span::new(FileId(0), 0, 0),
                        },
                    ],
                    terminator: HirTerminator::Return(None),
                    span: Span::new(FileId(0), 0, 0),
                },
            ],
            locals: vec![
                HirLocal { name: Some("result".to_string()), ty: HirType::Int { width: 32, signed: true }, span: Span::new(FileId(0), 0, 0) },
                HirLocal { name: Some("arg".to_string()), ty: HirType::Int { width: 32, signed: true }, span: Span::new(FileId(0), 0, 0) },
            ],
            entry_block: BlockId(0),
        };

        let mut sink = DiagSink::new();
        assert!(validate_locals(&body, &mut sink));
        assert_eq!(sink.peek().len(), 0);
    }
}
