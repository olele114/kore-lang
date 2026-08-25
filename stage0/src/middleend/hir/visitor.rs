//! HIR 遍历器（Visitor 模式）。
//!
//! 用于遍历 HIR 节点，支持 HIR 验证、转换、分析等 pass。

use super::*;

/// HIR 遍历器 trait
pub trait HirVisitor: Sized {
    fn visit_module(&mut self, module: &HirModule) {
        walk_module(self, module);
    }

    fn visit_function(&mut self, func: &HirFunction) {
        walk_function(self, func);
    }

    fn visit_struct(&mut self, s: &HirStruct) {
        walk_struct(self, s);
    }

    fn visit_union(&mut self, u: &HirUnion) {
        walk_union(self, u);
    }

    fn visit_body(&mut self, body: &HirBody) {
        walk_body(self, body);
    }

    fn visit_block(&mut self, block: &HirBlock) {
        walk_block(self, block);
    }

    fn visit_stmt(&mut self, stmt: &HirStmt) {
        walk_stmt(self, stmt);
    }

    fn visit_terminator(&mut self, term: &HirTerminator) {
        walk_terminator(self, term);
    }

    fn visit_place(&mut self, place: &HirPlace) {
        walk_place(self, place);
    }

    fn visit_rvalue(&mut self, rvalue: &HirRvalue) {
        walk_rvalue(self, rvalue);
    }

    fn visit_operand(&mut self, operand: &HirOperand) {
        walk_operand(self, operand);
    }

    fn visit_type(&mut self, ty: &ty::HirType) {
        walk_type(self, ty);
    }
}

// ────────────────────────────────────────────────────────────────────────────────
// 默认遍历实现（深度优先）
// ────────────────────────────────────────────────────────────────────────────────

pub fn walk_module<V: HirVisitor>(visitor: &mut V, module: &HirModule) {
    for func in &module.functions {
        visitor.visit_function(func);
    }
    for s in &module.structs {
        visitor.visit_struct(s);
    }
    for u in &module.unions {
        visitor.visit_union(u);
    }
}

pub fn walk_function<V: HirVisitor>(visitor: &mut V, func: &HirFunction) {
    for param in &func.params {
        visitor.visit_type(&param.ty);
    }
    visitor.visit_type(&func.ret_type);
    if let Some(body) = &func.body {
        visitor.visit_body(body);
    }
}

pub fn walk_struct<V: HirVisitor>(visitor: &mut V, s: &HirStruct) {
    for field in &s.fields {
        visitor.visit_type(&field.ty);
    }
}

pub fn walk_union<V: HirVisitor>(visitor: &mut V, u: &HirUnion) {
    for variant in &u.variants {
        if let Some(ty) = &variant.payload {
            visitor.visit_type(ty);
        }
    }
}

pub fn walk_body<V: HirVisitor>(visitor: &mut V, body: &HirBody) {
    for local in &body.locals {
        visitor.visit_type(&local.ty);
    }
    for block in &body.blocks {
        visitor.visit_block(block);
    }
}

pub fn walk_block<V: HirVisitor>(visitor: &mut V, block: &HirBlock) {
    for stmt in &block.stmts {
        visitor.visit_stmt(stmt);
    }
    visitor.visit_terminator(&block.terminator);
}

pub fn walk_stmt<V: HirVisitor>(visitor: &mut V, stmt: &HirStmt) {
    match stmt {
        HirStmt::Assign { lhs, rhs, .. } => {
            visitor.visit_place(lhs);
            visitor.visit_rvalue(rhs);
        }
        HirStmt::Call { dest, func, args, .. } => {
            if let Some(dest) = dest {
                visitor.visit_place(dest);
            }
            visitor.visit_operand(func);
            for arg in args {
                visitor.visit_operand(arg);
            }
        }
        HirStmt::Drop { place, .. } => {
            visitor.visit_place(place);
        }
    }
}

pub fn walk_terminator<V: HirVisitor>(visitor: &mut V, term: &HirTerminator) {
    match term {
        HirTerminator::Goto(_) => {}
        HirTerminator::Return(Some(operand)) => {
            visitor.visit_operand(operand);
        }
        HirTerminator::Return(None) => {}
        HirTerminator::Switch { discr, .. } => {
            visitor.visit_operand(discr);
        }
        HirTerminator::Unreachable => {}
    }
}

pub fn walk_place<V: HirVisitor>(visitor: &mut V, place: &HirPlace) {
    match place {
        HirPlace::Local(_) => {}
        HirPlace::Field { base, .. } => {
            visitor.visit_place(base);
        }
        HirPlace::Index { base, index } => {
            visitor.visit_place(base);
            visitor.visit_operand(index);
        }
        HirPlace::Deref(base) => {
            visitor.visit_place(base);
        }
    }
}

pub fn walk_rvalue<V: HirVisitor>(visitor: &mut V, rvalue: &HirRvalue) {
    match rvalue {
        HirRvalue::Use(operand) => {
            visitor.visit_operand(operand);
        }
        HirRvalue::BinaryOp { lhs, rhs, .. } => {
            visitor.visit_operand(lhs);
            visitor.visit_operand(rhs);
        }
        HirRvalue::UnaryOp { operand, .. } => {
            visitor.visit_operand(operand);
        }
        HirRvalue::Ref { place, .. } => {
            visitor.visit_place(place);
        }
        HirRvalue::Deref(operand) => {
            visitor.visit_operand(operand);
        }
        HirRvalue::Aggregate { fields, kind } => {
            for field in fields {
                visitor.visit_operand(field);
            }
            match kind {
                AggregateKind::Array(ty, _) => {
                    visitor.visit_type(ty);
                }
                _ => {}
            }
        }
        HirRvalue::Discriminant(place) => {
            visitor.visit_place(place);
        }
        HirRvalue::ExtractPayload { place, .. } => {
            visitor.visit_place(place);
        }
        HirRvalue::ArrayToSlice { array, .. } => {
            visitor.visit_operand(array);
        }
    }
}

pub fn walk_operand<V: HirVisitor>(visitor: &mut V, operand: &HirOperand) {
    match operand {
        HirOperand::Const(_) => {}
        HirOperand::Place(place) => {
            visitor.visit_place(place);
        }
        HirOperand::FuncRef(_) => {}
    }
}

pub fn walk_type<V: HirVisitor>(visitor: &mut V, ty: &ty::HirType) {
    match ty {
        ty::HirType::Ptr { pointee, .. } => {
            visitor.visit_type(pointee);
        }
        ty::HirType::Array { elem, .. } => {
            visitor.visit_type(elem);
        }
        ty::HirType::FnPtr { params, ret } => {
            for param in params {
                visitor.visit_type(param);
            }
            visitor.visit_type(ret);
        }
        _ => {}
    }
}
