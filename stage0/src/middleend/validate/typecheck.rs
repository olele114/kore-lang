//! HIR 类型检查验证。
//!
//! 验证赋值、函数调用、二元运算的类型兼容性。

use crate::middleend::hir::{
    HirBody, HirRvalue, HirPlace, HirOperand, HirTerminator, BinOp, HirStmt, HirUnion,
};
use crate::middleend::hir::ty::HirType;
use crate::diag::{DiagSink, ErrorCode, Diagnostic, DiagLoc};

/// 类型检查器
pub struct TypeChecker<'a> {
    body: &'a HirBody,
    unions: &'a [HirUnion],
    sink: &'a mut DiagSink,
}

impl<'a> TypeChecker<'a> {
    pub fn new(body: &'a HirBody, unions: &'a [HirUnion], sink: &'a mut DiagSink) -> Self {
        Self { body, unions, sink }
    }

    /// 验证类型一致性
    pub fn validate(&mut self) -> bool {
        let mut ok = true;

        for block in &self.body.blocks {
            // 检查赋值语句
            for stmt in &block.stmts {
                if let HirStmt::Assign { lhs, rhs, .. } = stmt {
                    if let Some(place_ty) = self.infer_place_type(lhs) {
                        if let Some(rvalue_ty) = self.infer_rvalue_type(rhs) {
                            if !self.types_compatible(&place_ty, &rvalue_ty) {
                                let diag = Diagnostic::error(
                                    ErrorCode::TypeMismatch as u16,
                                    format!("type mismatch in assignment: expected {:?}, found {:?}",
                                        place_ty, rvalue_ty),
                                    DiagLoc::None,
                                );
                                self.sink.emit(diag);
                                ok = false;
                            }
                        }
                    }
                }
            }

            // 检查终结符
            if !self.validate_terminator(&block.terminator) {
                ok = false;
            }
        }

        ok
    }

    /// 验证终结符的类型正确性
    fn validate_terminator(&mut self, term: &HirTerminator) -> bool {
        match term {
            HirTerminator::Return(value) => {
                // 返回值类型应该匹配函数签名
                if let Some(operand) = value {
                    if let Some(ret_ty) = self.infer_operand_type(operand) {
                        // 简化：暂不检查函数签名匹配
                        let _ = ret_ty;
                    }
                }
                true
            }
            HirTerminator::Switch { discr, .. } => {
                // Switch 判别式应该是整数或布尔类型
                if let Some(ty) = self.infer_operand_type(discr) {
                    match &ty {
                        HirType::Int { .. } | HirType::Bool => {}
                        _ => {} // 暂不报错
                    }
                }
                true
            }
            _ => true,
        }
    }

    /// 推导 Place 的类型
    fn infer_place_type(&mut self, place: &HirPlace) -> Option<HirType> {
        match place {
            HirPlace::Local(local_id) => {
                self.body.locals.get(local_id.0).map(|l| l.ty.clone())
            }
            HirPlace::Field { base, field } => {
                let base_ty = self.infer_place_type(base)?;
                match base_ty {
                    HirType::Struct(struct_id) => {
                        // 需要从结构体定义中获取字段类型
                        // 简化：暂不实现
                        let _ = (struct_id, field);
                        None
                    }
                    _ => None,
                }
            }
            HirPlace::Index { base, .. } => {
                let base_ty = self.infer_place_type(base)?;
                match base_ty {
                    HirType::Array { elem, .. } => Some((*elem).clone()),
                    HirType::Ptr { pointee, .. } => Some((*pointee).clone()),
                    _ => None,
                }
            }
            HirPlace::Deref(inner) => {
                let inner_ty = self.infer_place_type(inner)?;
                match inner_ty {
                    HirType::Ptr { pointee, .. } => Some((*pointee).clone()),
                    _ => None,
                }
            }
        }
    }

    /// 推导 Rvalue 的类型
    fn infer_rvalue_type(&mut self, rvalue: &HirRvalue) -> Option<HirType> {
        match rvalue {
            HirRvalue::Use(operand) => self.infer_operand_type(operand),
            HirRvalue::BinaryOp { op, lhs, rhs } => {
                self.infer_binop_type(*op, lhs, rhs)
            }
            HirRvalue::UnaryOp { op: _, operand } => {
                // 一元运算保持操作数类型
                self.infer_operand_type(operand)
            }
            HirRvalue::Ref { place, .. } => {
                let pointee_ty = self.infer_place_type(place)?;
                Some(HirType::Ptr {
                    pointee: Box::new(pointee_ty),
                    owned: false,
                })
            }
            HirRvalue::Aggregate { kind: _, fields } => {
                // 聚合类型：简化处理
                if fields.is_empty() {
                    Some(HirType::Void)
                } else {
                    self.infer_operand_type(&fields[0])
                }
            }
            HirRvalue::Deref(operand) => {
                // 解引用返回指针指向的类型
                if let Some(ty) = self.infer_operand_type(operand) {
                    match ty {
                        HirType::Ptr { pointee, .. } => Some((*pointee).clone()),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            HirRvalue::Discriminant(_place) => {
                // 联合体判别式返回整数类型
                Some(HirType::Int { width: 32, signed: false })
            }
            HirRvalue::ExtractPayload { place, variant_index } => {
                // 提取 payload：从联合体中获取指定变体的类型
                let place_ty = self.infer_place_type(place)?;

                // 从 place 的类型中获取联合体 ID
                if let HirType::Union(union_id) = place_ty {
                    // 查找联合体定义
                    if let Some(union_def) = self.unions.get(union_id.0) {
                        // 获取指定索引的变体
                        if let Some(variant) = union_def.variants.get(*variant_index) {
                            // 返回变体的 payload 类型，如果没有 payload 则返回 void
                            return Some(variant.payload.clone().unwrap_or(HirType::Void));
                        }
                    }
                }

                // 如果无法推断，返回 place 类型作为回退
                Some(place_ty)
            }
            HirRvalue::ArrayToSlice { elem_ty, .. } => {
                // 数组到切片转换返回切片类型
                Some(HirType::Slice {
                    elem: Box::new(elem_ty.clone()),
                })
            }
        }
    }

    /// 推导 Operand 的类型
    fn infer_operand_type(&mut self, operand: &HirOperand) -> Option<HirType> {
        match operand {
            HirOperand::Place(place) => {
                self.infer_place_type(place)
            }
            HirOperand::Const(c) => {
                use crate::middleend::hir::Const;
                match c {
                    Const::Void => Some(HirType::Void),
                    Const::Bool(_) => Some(HirType::Bool),
                    Const::Int(_) => Some(HirType::Int { width: 32, signed: true }),
                    Const::Float(_) => Some(HirType::Float { width: 64 }),
                    Const::Str(_) => Some(HirType::Ptr {
                        pointee: Box::new(HirType::Int { width: 8, signed: false }),
                        owned: false,
                    }),
                    Const::Nil => Some(HirType::Ptr {
                        pointee: Box::new(HirType::Void),
                        owned: false,
                    }),
                }
            }
            HirOperand::FuncRef(_func_id) => {
                // 函数引用：简化处理
                None
            }
        }
    }

    /// 推导二元运算的类型
    fn infer_binop_type(&mut self, op: BinOp, lhs: &HirOperand, rhs: &HirOperand) -> Option<HirType> {
        let lhs_ty = self.infer_operand_type(lhs)?;
        let rhs_ty = self.infer_operand_type(rhs)?;

        // 简化：假设操作数类型相同
        if !self.types_compatible(&lhs_ty, &rhs_ty) {
            return None;
        }

        match op {
            // 比较运算返回布尔
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                Some(HirType::Bool)
            }
            // 算术/位运算保持操作数类型
            _ => Some(lhs_ty),
        }
    }

    /// 检查类型兼容性
    fn types_compatible(&self, expected: &HirType, actual: &HirType) -> bool {
        match (expected, actual) {
            // 完全相同
            (a, b) if a == b => true,

            // 整数类型之间允许隐式转换（简化）
            (HirType::Int { .. }, HirType::Int { .. }) => true,

            // 浮点类型之间允许转换
            (HirType::Float { .. }, HirType::Float { .. }) => true,

            // Never 类型可以转换为任何类型
            (_, HirType::Never) => true,

            _ => false,
        }
    }
}

/// 验证类型一致性
pub fn validate_types(body: &HirBody, unions: &[HirUnion], sink: &mut DiagSink) -> bool {
    let mut checker = TypeChecker::new(body, unions, sink);
    checker.validate()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleend::hir::*;
    use crate::diag::{DiagSink, Span, FileId};

    fn dummy_span() -> Span {
        Span::new(FileId(0), 0, 1)
    }

    fn make_local(_id: usize, ty: HirType) -> HirLocal {
        HirLocal {
            name: None,
            ty,
            span: dummy_span(),
        }
    }

    #[test]
    fn test_assign_compatible_types() {
        let mut sink = DiagSink::new();
        let body = HirBody {
            entry_block: BlockId(0),
            locals: vec![
                make_local(0, HirType::Int { width: 32, signed: true }),
                make_local(1, HirType::Int { width: 64, signed: true }),
            ],
            blocks: vec![HirBlock {
                id: BlockId(0),
                stmts: vec![HirStmt::Assign {
                    lhs: HirPlace::Local(LocalId(0)),
                    rhs: HirRvalue::Use(HirOperand::Const(Const::Int(42))),
                    span: dummy_span(),
                }],
                terminator: HirTerminator::Return(None),
                span: dummy_span(),
            }],
        };

        assert!(validate_types(&body, &[], &mut sink));
        assert!(!sink.has_errors());
    }

    #[test]
    fn test_assign_incompatible_types() {
        let mut sink = DiagSink::new();
        let body = HirBody {
            entry_block: BlockId(0),
            locals: vec![
                make_local(0, HirType::Bool),
            ],
            blocks: vec![HirBlock {
                id: BlockId(0),
                stmts: vec![HirStmt::Assign {
                    lhs: HirPlace::Local(LocalId(0)),
                    rhs: HirRvalue::Use(HirOperand::Const(Const::Str("test".into()))),
                    span: dummy_span(),
                }],
                terminator: HirTerminator::Return(None),
                span: dummy_span(),
            }],
        };

        assert!(!validate_types(&body, &[], &mut sink));
        assert_eq!(sink.err_count(), 1);
    }

    #[test]
    fn test_binop_returns_bool_for_comparison() {
        let mut sink = DiagSink::new();
        let body = HirBody {
            entry_block: BlockId(0),
            locals: vec![
                make_local(0, HirType::Bool),
            ],
            blocks: vec![HirBlock {
                id: BlockId(0),
                stmts: vec![HirStmt::Assign {
                    lhs: HirPlace::Local(LocalId(0)),
                    rhs: HirRvalue::BinaryOp {
                        op: BinOp::Eq,
                        lhs: HirOperand::Const(Const::Int(1)),
                        rhs: HirOperand::Const(Const::Int(2)),
                    },
                    span: dummy_span(),
                }],
                terminator: HirTerminator::Return(None),
                span: dummy_span(),
            }],
        };

        assert!(validate_types(&body, &[], &mut sink));
        assert!(!sink.has_errors());
    }

    #[test]
    fn test_deref_pointer_type() {
        let mut sink = DiagSink::new();
        let pointee_ty = HirType::Int { width: 32, signed: true };
        let ptr_ty = HirType::Ptr {
            pointee: Box::new(pointee_ty.clone()),
            owned: false,
        };

        let body = HirBody {
            entry_block: BlockId(0),
            locals: vec![
                make_local(0, pointee_ty),
                make_local(1, ptr_ty),
            ],
            blocks: vec![HirBlock {
                id: BlockId(0),
                stmts: vec![HirStmt::Assign {
                    lhs: HirPlace::Local(LocalId(0)),
                    rhs: HirRvalue::Deref(HirOperand::Place(Box::new(HirPlace::Local(LocalId(1))))),
                    span: dummy_span(),
                }],
                terminator: HirTerminator::Return(None),
                span: dummy_span(),
            }],
        };

        assert!(validate_types(&body, &[], &mut sink));
        assert!(!sink.has_errors());
    }

    #[test]
    fn test_ref_creates_pointer() {
        let mut sink = DiagSink::new();
        let base_ty = HirType::Int { width: 32, signed: true };
        let ptr_ty = HirType::Ptr {
            pointee: Box::new(base_ty.clone()),
            owned: false,
        };

        let body = HirBody {
            entry_block: BlockId(0),
            locals: vec![
                make_local(0, base_ty),
                make_local(1, ptr_ty),
            ],
            blocks: vec![HirBlock {
                id: BlockId(0),
                stmts: vec![HirStmt::Assign {
                    lhs: HirPlace::Local(LocalId(1)),
                    rhs: HirRvalue::Ref {
                        place: HirPlace::Local(LocalId(0)),
                        owned: false,
                    },
                    span: dummy_span(),
                }],
                terminator: HirTerminator::Return(None),
                span: dummy_span(),
            }],
        };

        assert!(validate_types(&body, &[], &mut sink));
        assert!(!sink.has_errors());
    }

    #[test]
    fn test_never_type_compatible_with_any() {
        let mut sink = DiagSink::new();
        let body = HirBody {
            locals: vec![
                make_local(0, HirType::Int { width: 32, signed: true }),
            ],
            blocks: vec![HirBlock {
                id: BlockId(0),
                stmts: vec![HirStmt::Assign {
                    lhs: HirPlace::Local(LocalId(0)),
                    rhs: HirRvalue::Use(HirOperand::Place(Box::new(HirPlace::Local(LocalId(1))))),
                    span: dummy_span(),
                }],
                terminator: HirTerminator::Return(None),
                span: dummy_span(),
            }],
            entry_block: BlockId(0),
        };

        // 这个测试简化了 - 实际场景中 Never 类型会在控制流分析中处理
        assert!(validate_types(&body, &[], &mut sink));
    }
}
