//! AST 遍历。resolve / typecheck / borrow / lower 都要走一遍树，遍历顺序
//! 写四遍必然分叉，所以只写一份。
//!
//! 默认方法只负责往下走。实现者覆盖关心的节点，其余交给 `walk_*`。

use super::node::*;

pub trait Visitor {
    fn visit_module(&mut self, m: &Module) {
        walk_module(self, m);
    }
    fn visit_item(&mut self, it: &Item) {
        walk_item(self, it);
    }
    fn visit_stmt(&mut self, s: &Stmt) {
        walk_stmt(self, s);
    }
    fn visit_expr(&mut self, e: &Expr) {
        walk_expr(self, e);
    }
    /// 类型与模式是叶子层，默认不再下钻。
    fn visit_type(&mut self, _t: &TypeExpr) {}
    fn visit_pattern(&mut self, _p: &Pattern) {}
}

pub fn walk_module<V: Visitor + ?Sized>(v: &mut V, m: &Module) {
    for it in &m.items {
        v.visit_item(it);
    }
}

pub fn walk_item<V: Visitor + ?Sized>(v: &mut V, it: &Item) {
    match it {
        Item::Func(f) => {
            for p in &f.params {
                v.visit_type(&p.ty);
            }
            if let Some(t) = &f.ret {
                v.visit_type(t);
            }
            if let Some(t) = &f.err {
                v.visit_type(t);
            }
            v.visit_expr(&f.body);
        }
        Item::Struct(s) => {
            for f in &s.fields {
                v.visit_type(&f.ty);
            }
        }
        Item::Union(u) => {
            for var in &u.variants {
                for t in &var.payload {
                    v.visit_type(t);
                }
            }
        }
        Item::Use(_) => {}
    }
}

pub fn walk_stmt<V: Visitor + ?Sized>(v: &mut V, s: &Stmt) {
    match s {
        Stmt::Let { ty, init, .. } => {
            if let Some(t) = ty {
                v.visit_type(t);
            }
            v.visit_expr(init);
        }
        Stmt::Assign { target, value, .. } => {
            v.visit_expr(target);
            v.visit_expr(value);
        }
        Stmt::Defer(e, _) => v.visit_expr(e),
        Stmt::Expr(e) => v.visit_expr(e),
    }
}

pub fn walk_expr<V: Visitor + ?Sized>(v: &mut V, e: &Expr) {
    match e {
        Expr::Int(..)
        | Expr::Float(..)
        | Expr::Str(..)
        | Expr::Bool(..)
        | Expr::Nil(_)
        | Expr::Path(..)
        | Expr::Skip { .. } => {}
        Expr::Branch { scrutinee, arms, .. } => {
            if let Some(s) = scrutinee {
                v.visit_expr(s);
            }
            for arm in arms {
                v.visit_pattern(&arm.pattern);
                v.visit_expr(&arm.body);
            }
        }
        Expr::Loop { subject, body, .. } => {
            if let Some(s) = subject {
                v.visit_expr(s);
            }
            v.visit_expr(body);
        }
        Expr::Call { callee, args, .. } => {
            v.visit_expr(callee);
            for a in args {
                v.visit_expr(a);
            }
        }
        Expr::Field { base, .. } => v.visit_expr(base),
        Expr::Index { base, index, .. } => {
            v.visit_expr(base);
            v.visit_expr(index);
        }
        Expr::Deref(inner, _) | Expr::Propagate(inner, _) => v.visit_expr(inner),
        Expr::Jmp { target, .. } => {
            if let Some(t) = target {
                v.visit_expr(t);
            }
        }
        Expr::Ret(e, _) => {
            if let Some(e) = e {
                v.visit_expr(e);
            }
        }
        Expr::Stop { label: _, span: _ } => {}
        Expr::Unary { operand, .. } => v.visit_expr(operand),
        Expr::Binary { lhs, rhs, .. } => {
            v.visit_expr(lhs);
            v.visit_expr(rhs);
        }
        Expr::Block { stmts, .. } => {
            for s in stmts {
                v.visit_stmt(s);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_name, expr) in fields {
                v.visit_expr(expr);
            }
        }
        Expr::ArrayLit { elements, .. } => {
            for elem in elements {
                v.visit_expr(elem);
            }
        }
        Expr::VariantConstructor { payload, .. } => {
            if let Some(p) = payload {
                v.visit_expr(p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{FileId, Span};

    struct Counter {
        exprs: usize,
    }

    impl Visitor for Counter {
        fn visit_expr(&mut self, e: &Expr) {
            self.exprs += 1;
            walk_expr(self, e);
        }
    }

    #[test]
    fn walk_reaches_nested_exprs() {
        let s = Span::new(FileId(0), 0, 1);
        // { x = 1 + 2 } —— 块 1 + 赋值目标 1 + 二元 1 + 两个字面量 2 = 5
        let body = Expr::Block {
            stmts: vec![Stmt::Assign {
                target: Expr::Path(vec!["x".into()], s),
                value: Expr::Binary {
                    op: "+",
                    lhs: Box::new(Expr::Int("1".into(), s)),
                    rhs: Box::new(Expr::Int("2".into(), s)),
                    span: s,
                },
                span: s,
            }],
            span: s,
        };
        let m = Module {
            items: vec![Item::Func(Func {
                is_public: false,
                name: "main".into(),
                params: Vec::new(),
                ret: None,
                err: None,
                body,
                span: s,
            })],
            span: s,
        };
        let mut c = Counter { exprs: 0 };
        c.visit_module(&m);
        assert_eq!(c.exprs, 5);
    }
}
