//! 不逃逸检查器。
//!
//! 实现两条流敏感规则（docs/spec/05-memory.md §2）：
//! 1. **移动后不可用**（E5001）— own ^T 绑定移动后再次使用是错误。
//! 2. **不逃逸**（E5002/E5003）— 借用指针不能存入比来源更长寿的位置。
//!
//! 类型信息来自语法层的 TypeExpr，无显式类型标注的绑定视为普通绑定
//! （保守处理：不对推断类型做假设）。

use crate::diag::{DiagSink, Diagnostic, DiagLoc, ErrorCode, Span};
use crate::frontend::ast::{Expr, Func, Item, Module, Param, Stmt, TypeExpr};
use super::context::{BindingInfo, BindingKind, EscapeContext};

/// 不逃逸检查器。每个函数独立运行一次。
pub struct EscapeChecker<'a> {
    ctx: EscapeContext,
    sink: &'a mut DiagSink,
}

impl<'a> EscapeChecker<'a> {
    pub fn new(sink: &'a mut DiagSink) -> Self {
        Self { ctx: EscapeContext::new(), sink }
    }

    /// 检查整个模块（遍历所有顶层函数）。
    pub fn check_module(&mut self, module: &Module) {
        for item in &module.items {
            if let Item::Func(f) = item {
                self.check_func(f);
            }
        }
    }

    /// 检查单个函数。
    pub fn check_func(&mut self, func: &Func) {
        // 重置上下文，每个函数独立分析。
        self.ctx = EscapeContext::new();

        // 注册参数（深度 0）。
        for param in &func.params {
            self.register_param(param);
        }

        // 检查函数体。
        self.check_expr(&func.body);
    }

    fn register_param(&mut self, param: &Param) {
        let kind = binding_kind_from_type(&param.ty);
        let info = BindingInfo { kind, state: super::context::OwnershipState::Live, depth: 0 };
        self.ctx.define(param.name.clone(), info);
    }

    fn check_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Block { stmts, .. } => {
                self.ctx.push_scope();
                for stmt in stmts {
                    self.check_stmt(stmt);
                }
                self.ctx.pop_scope();
            }

            // ret expr — 检查是否把借用指针作为返回值（E5003）。
            Expr::Ret(Some(val), span) => {
                self.check_expr(val);
                if let Expr::Path(segments, _) = val.as_ref()
                    && let Some(name) = segments.first()
                    && self.is_local_borrow(name)
                {
                    let msg = format!("借用指针 '{}' 不能作为返回值逃逸", name);
                    self.emit(ErrorCode::BorrowEscapesToReturn, msg, *span);
                }
            }

            Expr::Ret(None, _) => {}

            // Call — 参数传递视作移动（对 own 绑定）。
            Expr::Call { callee, args, .. } => {
                self.check_expr(callee);
                for arg in args {
                    self.check_expr(arg);
                    if let Expr::Path(segments, span) = arg
                        && let Some(name) = segments.first()
                    {
                        self.check_use(name, *span);
                        // 传参视为移动 own 绑定。
                        self.ctx.mark_moved(name, *span);
                    }
                }
            }

            // 对路径表达式直接检查是否是移动后使用。
            Expr::Path(segments, span) => {
                if let Some(name) = segments.first() {
                    self.check_use(name, *span);
                }
            }

            // 其他表达式：递归子节点。
            Expr::Binary { lhs, rhs, .. } => {
                self.check_expr(lhs);
                self.check_expr(rhs);
            }
            Expr::Unary { operand, .. } => self.check_expr(operand),
            Expr::Field { base, .. } => self.check_expr(base),
            Expr::Index { base, index, .. } => {
                self.check_expr(base);
                self.check_expr(index);
            }
            Expr::Deref(inner, _) => self.check_expr(inner),
            Expr::Propagate(inner, _) => self.check_expr(inner),
            Expr::Jmp { target, .. } => {
                if let Some(t) = target {
                    self.check_expr(t);
                }
            }
            // stop 不再携带值，仅作为控制流
            Expr::Stop { .. } => {}
            Expr::Loop { subject, body, .. } => {
                if let Some(s) = subject { self.check_expr(s); }
                self.check_expr(body);
            }
            Expr::Branch { scrutinee, arms, span } => {
                if let Some(s) = scrutinee { self.check_expr(s); }
                let base_snap = self.ctx.snapshot_moves();
                let mut arm_snaps = Vec::new();
                for arm in arms {
                    self.ctx.restore_snapshot(&base_snap);
                    self.ctx.push_scope();
                    self.inject_arm_bindings(&arm.pattern);
                    self.check_expr(&arm.body);
                    self.ctx.pop_scope();
                    arm_snaps.push(self.ctx.snapshot_moves());
                }
                // Join：任一分支移动了变量 → 分支后视作已移动（保守策略）。
                self.ctx.restore_snapshot(&base_snap);
                for snap in &arm_snaps {
                    for (name, state) in snap {
                        if matches!(state, super::context::OwnershipState::Moved(_)) {
                            self.ctx.mark_moved(name, *span);
                        }
                    }
                }
            }

            // 字面量、Nil、Skip、Stop(None)、Ret(None) 无需检查。
            _ => {}
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, ty, init, .. } => {
                // 先检查初始值（可能包含移动操作）。
                self.check_expr(init);

                // 如果初始值是路径，该路径的 own 绑定此时被移动。
                if let Expr::Path(segments, span) = init
                    && let Some(src) = segments.first()
                {
                    self.ctx.mark_moved(src, *span);
                }

                // 定义新绑定。
                let kind = ty.as_ref().map_or(BindingKind::Plain, binding_kind_from_type);
                let depth = self.ctx.depth();
                self.ctx.define(
                    name.clone(),
                    BindingInfo { kind, state: super::context::OwnershipState::Live, depth },
                );
            }

            Stmt::Assign { target, value, span } => {
                self.check_expr(value);

                // 如果赋值目标是字段访问，且值是借用绑定 → E5002（可能逃逸到堆）。
                if let Expr::Field { .. } = target
                    && let Expr::Path(segments, _) = value
                    && let Some(name) = segments.first()
                    && self.is_local_borrow(name)
                {
                    self.emit(
                        ErrorCode::BorrowEscapesToHeap,
                        format!("借用指针 '{}' 不能存入结构体字段（可能逃逸到堆）", name),
                        *span,
                    );
                }

                self.check_expr(target);
            }

            Stmt::Defer(expr, _) => self.check_expr(expr),
            Stmt::Expr(expr) => self.check_expr(expr),
        }
    }

    /// 检查对某个名字的使用：如果它是已移动的 own 绑定，报 E5001。
    fn check_use(&mut self, name: &str, span: Span) {
        let moved_at = match self.ctx.lookup(name) {
            Some(info) if info.kind == BindingKind::Own => match info.state {
                super::context::OwnershipState::Moved(move_span) => Some(move_span),
                super::context::OwnershipState::Live => None,
            },
            _ => None,
        };

        if let Some(move_span) = moved_at {
            self.emit(
                ErrorCode::UseAfterMove,
                format!("'{}' 在移动后被使用（首次移动位置：偏移 {}）", name, move_span.lo),
                span,
            );
        }
    }

    /// 判断某个名字是否是局部借用绑定（深度 > 0，即不是函数参数）。
    /// 保守策略：函数参数的借用（深度 0）不检查，因为它们与调用者同寿。
    fn is_local_borrow(&self, name: &str) -> bool {
        match self.ctx.lookup(name) {
            Some(info) => info.kind == BindingKind::Borrow && info.depth > 0,
            None => false,
        }
    }

    /// 把分支 arm 的 pattern 绑定注入当前作用域（depth 已由调用方 push）。
    fn inject_arm_bindings(&mut self, pattern: &crate::frontend::ast::Pattern) {
        use crate::frontend::ast::Pattern;
        let depth = self.ctx.depth();
        match pattern {
            Pattern::Bind(name, _) => {
                self.ctx.define(name.clone(), BindingInfo::plain(depth));
            }
            Pattern::Variant { bindings, .. } => {
                for name in bindings {
                    self.ctx.define(name.clone(), BindingInfo::plain(depth));
                }
            }
            _ => {}
        }
    }

    fn emit(&mut self, code: ErrorCode, msg: String, span: Span) {
        self.sink.emit(Diagnostic::error(code.as_u16(), msg, DiagLoc::At(span)));
    }
}

/// 从类型表达式推断绑定种类。
fn binding_kind_from_type(ty: &TypeExpr) -> BindingKind {
    match ty {
        TypeExpr::Own(..) => BindingKind::Own,
        TypeExpr::Borrow(..) => BindingKind::Borrow,
        _ => BindingKind::Plain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::FileId;

    fn s() -> Span { Span::new(FileId(0), 0, 4) }

    fn make_func(
        name: &str,
        params: Vec<Param>,
        ret: Option<TypeExpr>,
        body: Expr,
    ) -> Func {
        Func { is_public: false, name: name.into(), params, ret, err: None, body, span: s() }
    }

    fn int_lit() -> Expr { Expr::Int("1".into(), s()) }

    fn path(name: &str) -> Expr { Expr::Path(vec![name.into()], s()) }

    fn own_param(name: &str) -> Param {
        Param {
            name: name.into(),
            ty: TypeExpr::Own(Box::new(TypeExpr::Named("T".into(), s())), s()),
            is_mut: false,
            span: s(),
        }
    }

    fn borrow_param(name: &str) -> Param {
        Param {
            name: name.into(),
            ty: TypeExpr::Borrow(Box::new(TypeExpr::Named("T".into(), s())), s()),
            is_mut: false,
            span: s(),
        }
    }

    #[test]
    fn no_error_for_plain_function() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);
        let func = make_func("f", vec![], None, int_lit());
        checker.check_func(&func);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn use_after_move_reports_e5001() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        // f :: (x own ^T) void =>
        //   { y := x; z := x }   ← 第二次使用 x 是 use-after-move
        let body = Expr::Block {
            stmts: vec![
                Stmt::Let {
                    name: "y".into(),
                    is_mut: false,
                    ty: None,
                    init: path("x"),
                    span: s(),
                },
                Stmt::Let {
                    name: "z".into(),
                    is_mut: false,
                    ty: None,
                    init: path("x"),  // use after move
                    span: s(),
                },
            ],
            span: s(),
        };

        let func = make_func("f", vec![own_param("x")], None, body);
        checker.check_func(&func);
        assert!(sink.err_count() > 0);
        let diags = sink.finish();
        assert!(diags.iter().any(|d| d.code == ErrorCode::UseAfterMove.as_u16()));
    }

    #[test]
    fn single_use_of_own_is_fine() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Block {
            stmts: vec![
                Stmt::Let {
                    name: "y".into(),
                    is_mut: false,
                    ty: None,
                    init: path("x"),
                    span: s(),
                },
            ],
            span: s(),
        };

        let func = make_func("f", vec![own_param("x")], None, body);
        checker.check_func(&func);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn borrow_param_return_is_fine() {
        // 函数参数（深度 0）的借用可以返回，因为它与调用者同寿。
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Ret(Some(Box::new(path("x"))), s());
        let func = make_func(
            "f",
            vec![borrow_param("x")],
            Some(TypeExpr::Borrow(Box::new(TypeExpr::Named("T".into(), s())), s())),
            body,
        );
        checker.check_func(&func);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn local_borrow_return_reports_e5003() {
        // 函数内部创建的借用绑定（深度 1）ret 出去 → E5003。
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Block {
            stmts: vec![
                Stmt::Let {
                    name: "local".into(),
                    is_mut: false,
                    ty: Some(TypeExpr::Borrow(
                        Box::new(TypeExpr::Named("T".into(), s())),
                        s(),
                    )),
                    init: int_lit(),
                    span: s(),
                },
                Stmt::Expr(Expr::Ret(Some(Box::new(path("local"))), s())),
            ],
            span: s(),
        };

        let func = make_func(
            "f",
            vec![],
            Some(TypeExpr::Borrow(Box::new(TypeExpr::Named("T".into(), s())), s())),
            body,
        );
        checker.check_func(&func);
        assert!(sink.err_count() > 0);
        let diags = sink.finish();
        assert!(diags.iter().any(|d| d.code == ErrorCode::BorrowEscapesToReturn.as_u16()));
    }

    #[test]
    fn local_borrow_field_assign_reports_e5002() {
        // 把局部借用存入结构体字段 → E5002。
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Block {
            stmts: vec![
                Stmt::Let {
                    name: "b".into(),
                    is_mut: false,
                    ty: Some(TypeExpr::Borrow(
                        Box::new(TypeExpr::Named("T".into(), s())),
                        s(),
                    )),
                    init: int_lit(),
                    span: s(),
                },
                Stmt::Assign {
                    target: Expr::Field {
                        base: Box::new(path("obj")),
                        name: "field".into(),
                        span: s(),
                    },
                    value: path("b"),
                    span: s(),
                },
            ],
            span: s(),
        };

        let func = make_func("f", vec![], None, body);
        checker.check_func(&func);
        assert!(sink.err_count() > 0);
        let diags = sink.finish();
        assert!(diags.iter().any(|d| d.code == ErrorCode::BorrowEscapesToHeap.as_u16()));
    }

    #[test]
    fn call_moves_own_arg() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        // f :: (x own ^T) void => { g(x); h(x) }  // 第二次使用 x 报错
        let body = Expr::Block {
            stmts: vec![
                Stmt::Expr(Expr::Call {
                    callee: Box::new(path("g")),
                    args: vec![path("x")],
                    span: s(),
                }),
                Stmt::Expr(Expr::Call {
                    callee: Box::new(path("h")),
                    args: vec![path("x")],  // use after move
                    span: s(),
                }),
            ],
            span: s(),
        };

        let func = make_func("f", vec![own_param("x")], None, body);
        checker.check_func(&func);
        assert!(sink.err_count() > 0);
        let diags = sink.finish();
        assert!(diags.iter().any(|d| d.code == ErrorCode::UseAfterMove.as_u16()));
    }

    #[test]
    fn binary_expr_checks_operands() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Binary {
            lhs: Box::new(path("x")),
            op: "+".into(),
            rhs: Box::new(path("x")),
            span: s(),
        };

        let func = make_func("f", vec![own_param("x")], None, body);
        checker.check_func(&func);
        // 二元表达式不移动，所以不应报错
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn unary_expr_checks_operand() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Unary {
            op: "-".into(),
            operand: Box::new(path("x")),
            span: s(),
        };

        let func = make_func("f", vec![], None, body);
        checker.check_func(&func);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn field_access_checks_base() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Field {
            base: Box::new(path("x")),
            name: "field".into(),
            span: s(),
        };

        let func = make_func("f", vec![], None, body);
        checker.check_func(&func);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn index_checks_base_and_index() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Index {
            base: Box::new(path("arr")),
            index: Box::new(int_lit()),
            span: s(),
        };

        let func = make_func("f", vec![], None, body);
        checker.check_func(&func);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn deref_checks_operand() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Deref(Box::new(path("ptr")), s());
        let func = make_func("f", vec![], None, body);
        checker.check_func(&func);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn propagate_checks_operand() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Propagate(Box::new(path("result")), s());
        let func = make_func("f", vec![], None, body);
        checker.check_func(&func);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn jmp_checks_operand() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Jmp {
            target: Some(Box::new(path("target"))),
            label: None,
            span: s(),
        };
        let func = make_func("f", vec![], None, body);
        checker.check_func(&func);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn stop_is_control_flow_only() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        // stop 现在是控制流语句（类似 break），不携带值，不会触发借用逃逸
        let body = Expr::Block {
            stmts: vec![
                Stmt::Let {
                    name: "local".into(),
                    is_mut: false,
                    ty: Some(TypeExpr::Borrow(
                        Box::new(TypeExpr::Named("T".into(), s())),
                        s(),
                    )),
                    init: int_lit(),
                    span: s(),
                },
                Stmt::Expr(Expr::Stop { label: Some("label".to_string()), span: s() }),
            ],
            span: s(),
        };

        let func = make_func("f", vec![], None, body);
        checker.check_func(&func);
        assert_eq!(sink.err_count(), 0); // stop 不携带值，无借用逃逸
    }

    #[test]
    fn loop_with_subject_checks_both() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Loop {
            subject: Some(Box::new(path("x"))),
            body: Box::new(int_lit()),
            label: None,
            span: s(),
        };

        let func = make_func("f", vec![], None, body);
        checker.check_func(&func);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn branch_conservative_merge() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        // branch x { A => { y := x }, B => { } }; z := x
        // 因为任一分支移动了 x，分支后 x 视为已移动
        let body = Expr::Block {
            stmts: vec![
                Stmt::Expr(Expr::Branch {
                    scrutinee: Some(Box::new(path("val"))),
                    arms: vec![
                        crate::frontend::ast::Arm {
                            pattern: crate::frontend::ast::Pattern::Variant {
                                name: "A".into(),
                                bindings: vec![],
                                span: s(),
                            },
                            body: Expr::Block {
                                stmts: vec![Stmt::Let {
                                    name: "y".into(),
                                    is_mut: false,
                                    ty: None,
                                    init: path("x"),
                                    span: s(),
                                }],
                                span: s(),
                            },
                            span: s(),
                        },
                        crate::frontend::ast::Arm {
                            pattern: crate::frontend::ast::Pattern::Variant {
                                name: "B".into(),
                                bindings: vec![],
                                span: s(),
                            },
                            body: int_lit(),
                            span: s(),
                        },
                    ],
                    span: s(),
                }),
                Stmt::Let {
                    name: "z".into(),
                    is_mut: false,
                    ty: None,
                    init: path("x"),
                    span: s(),
                },
            ],
            span: s(),
        };

        let func = make_func("f", vec![own_param("x")], None, body);
        checker.check_func(&func);
        assert!(sink.err_count() > 0);
        let diags = sink.finish();
        assert!(diags.iter().any(|d| d.code == ErrorCode::UseAfterMove.as_u16()));
    }

    #[test]
    fn defer_checks_expr() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Block {
            stmts: vec![Stmt::Defer(path("cleanup"), s())],
            span: s(),
        };

        let func = make_func("f", vec![], None, body);
        checker.check_func(&func);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn check_module_processes_all_funcs() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let module = Module {
            items: vec![
                Item::Func(make_func("f", vec![], None, int_lit())),
                Item::Func(make_func("g", vec![], None, int_lit())),
            ],
            span: s(),
        };

        checker.check_module(&module);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn variant_pattern_injects_bindings() {
        let mut sink = DiagSink::new();
        let mut checker = EscapeChecker::new(&mut sink);

        let body = Expr::Branch {
            scrutinee: Some(Box::new(path("val"))),
            arms: vec![crate::frontend::ast::Arm {
                pattern: crate::frontend::ast::Pattern::Variant {
                    name: "Some".into(),
                    bindings: vec!["x".into()],
                    span: s(),
                },
                body: path("x"),  // x 应该可用
                span: s(),
            }],
            span: s(),
        };

        let func = make_func("f", vec![], None, body);
        checker.check_func(&func);
        assert_eq!(sink.err_count(), 0);
    }
}
