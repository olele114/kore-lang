//! 测试 AST visitor 模式。

use kore_stage0::frontend::ast::node::*;
use kore_stage0::frontend::ast::visitor::{Visitor, walk_expr, walk_item, walk_stmt};
use kore_stage0::diag::{FileId, Span};

/// 计数访问器，用于验证遍历是否到达所有节点
struct CountingVisitor {
    expr_count: usize,
    stmt_count: usize,
    item_count: usize,
}

impl CountingVisitor {
    fn new() -> Self {
        Self {
            expr_count: 0,
            stmt_count: 0,
            item_count: 0,
        }
    }
}

impl Visitor for CountingVisitor {
    fn visit_expr(&mut self, e: &Expr) {
        self.expr_count += 1;
        walk_expr(self, e);
    }

    fn visit_stmt(&mut self, s: &Stmt) {
        self.stmt_count += 1;
        walk_stmt(self, s);
    }

    fn visit_item(&mut self, _it: &Item) {
        self.item_count += 1;
        // 注意：不调用 walk_item，因为它会递归访问 expr/stmt
    }
}

#[test]
fn visitor_counts_simple_expression() {
    let span = Span::new(FileId(0), 0, 1);

    // 创建简单表达式: 1 + 2
    let expr = Expr::Binary {
        op: "+",
        lhs: Box::new(Expr::Int("1".into(), span)),
        rhs: Box::new(Expr::Int("2".into(), span)),
        span,
    };

    let mut visitor = CountingVisitor::new();
    visitor.visit_expr(&expr);

    // 应该访问 3 个表达式节点: Binary, Int(1), Int(2)
    assert_eq!(visitor.expr_count, 3);
}

#[test]
fn visitor_counts_block_with_statement() {
    let span = Span::new(FileId(0), 0, 1);

    // 创建语句块: { 42 }
    let block = Expr::Block {
        stmts: vec![
            Stmt::Expr(Expr::Int("42".into(), span)),
        ],
        span,
    };

    let mut visitor = CountingVisitor::new();
    visitor.visit_expr(&block);

    // Block expr + 内部 Int expr = 2
    assert_eq!(visitor.expr_count, 2);
    // 一个 Stmt::Expr
    assert_eq!(visitor.stmt_count, 1);
}

#[test]
fn visitor_traverses_nested_binary() {
    let span = Span::new(FileId(0), 0, 1);

    // { x = 1 + 2 }
    let block = Expr::Block {
        stmts: vec![
            Stmt::Assign {
                target: Expr::Path(vec!["x".into()], span),
                value: Expr::Binary {
                    op: "+",
                    lhs: Box::new(Expr::Int("1".into(), span)),
                    rhs: Box::new(Expr::Int("2".into(), span)),
                    span,
                },
                span,
            },
        ],
        span,
    };

    let mut visitor = CountingVisitor::new();
    visitor.visit_expr(&block);

    // Block(1) + Path(1) + Binary(1) + Int(1) + Int(1) = 5
    assert_eq!(visitor.expr_count, 5);
    // 一个 Stmt::Assign
    assert_eq!(visitor.stmt_count, 1);
}

#[test]
fn visitor_handles_function_items() {
    let span = Span::new(FileId(0), 0, 1);

    let module = Module {
        items: vec![
            Item::Func(Func { is_public: false,
                name: "test".into(),
                params: vec![],
                ret: None,
                err: None,
                body: Expr::Int("42".into(), span),
                span,
            }),
        ],
        span,
    };

    let mut visitor = CountingVisitor::new();
    visitor.visit_module(&module);

    assert_eq!(visitor.item_count, 1);
}

#[test]
fn visitor_handles_call_expressions() {
    let span = Span::new(FileId(0), 0, 1);

    // foo(1, 2)
    let call = Expr::Call {
        callee: Box::new(Expr::Path(vec!["foo".into()], span)),
        args: vec![
            Expr::Int("1".into(), span),
            Expr::Int("2".into(), span),
        ],
        span,
    };

    let mut visitor = CountingVisitor::new();
    visitor.visit_expr(&call);

    // Call(1) + Path(1) + Int(1) + Int(1) = 4
    assert_eq!(visitor.expr_count, 4);
}

#[test]
fn visitor_handles_let_statements() {
    let span = Span::new(FileId(0), 0, 1);

    let stmt = Stmt::Let {
        name: "x".into(),
        is_mut: false,
        ty: None,
        init: Expr::Int("10".into(), span),
        span,
    };

    let mut visitor = CountingVisitor::new();
    visitor.visit_stmt(&stmt);

    assert_eq!(visitor.stmt_count, 1);
    assert_eq!(visitor.expr_count, 1); // init expression
}

#[test]
fn visitor_handles_branch_expressions() {
    let span = Span::new(FileId(0), 0, 1);

    // if true { 1 } else { 2 }
    let branch = Expr::Branch {
        scrutinee: Some(Box::new(Expr::Bool(true, span))),
        arms: vec![
            Arm {
                pattern: Pattern::Wildcard(span),
                body: Expr::Int("1".into(), span),
                span,
            },
            Arm {
                pattern: Pattern::Wildcard(span),
                body: Expr::Int("2".into(), span),
                span,
            },
        ],
        span,
    };

    let mut visitor = CountingVisitor::new();
    visitor.visit_expr(&branch);

    // Branch(1) + Bool(1) + Int(1) + Int(1) = 4
    assert_eq!(visitor.expr_count, 4);
}

#[test]
fn visitor_handles_loop_expressions() {
    let span = Span::new(FileId(0), 0, 1);

    // loop { 42 }
    let loop_expr = Expr::Loop {
        subject: None,
        body: Box::new(Expr::Int("42".into(), span)),
        label: None,
        span,
    };

    let mut visitor = CountingVisitor::new();
    visitor.visit_expr(&loop_expr);

    // Loop(1) + Int(1) = 2
    assert_eq!(visitor.expr_count, 2);
}

#[test]
fn visitor_handles_field_access() {
    let span = Span::new(FileId(0), 0, 1);

    // obj.field
    let field = Expr::Field {
        base: Box::new(Expr::Path(vec!["obj".into()], span)),
        name: "field".into(),
        span,
    };

    let mut visitor = CountingVisitor::new();
    visitor.visit_expr(&field);

    // Field(1) + Path(1) = 2
    assert_eq!(visitor.expr_count, 2);
}

#[test]
fn visitor_handles_unary_expressions() {
    let span = Span::new(FileId(0), 0, 1);

    // -42
    let unary = Expr::Unary {
        op: "-",
        operand: Box::new(Expr::Int("42".into(), span)),
        span,
    };

    let mut visitor = CountingVisitor::new();
    visitor.visit_expr(&unary);

    // Unary(1) + Int(1) = 2
    assert_eq!(visitor.expr_count, 2);
}

#[test]
fn visitor_handles_return_expressions() {
    let span = Span::new(FileId(0), 0, 1);

    // ret 42
    let ret = Expr::Ret(Some(Box::new(Expr::Int("42".into(), span))), span);

    let mut visitor = CountingVisitor::new();
    visitor.visit_expr(&ret);

    // Ret(1) + Int(1) = 2
    assert_eq!(visitor.expr_count, 2);
}

#[test]
fn visitor_handles_defer_statements() {
    let span = Span::new(FileId(0), 0, 1);

    // defer cleanup()
    let defer = Stmt::Defer(
        Expr::Call {
            callee: Box::new(Expr::Path(vec!["cleanup".into()], span)),
            args: vec![],
            span,
        },
        span,
    );

    let mut visitor = CountingVisitor::new();
    visitor.visit_stmt(&defer);

    // 一个 Stmt::Defer
    assert_eq!(visitor.stmt_count, 1);
    // Call(1) + Path(1) = 2
    assert_eq!(visitor.expr_count, 2);
}

// ── 补充：覆盖尚未命中的 visitor 分支 ────────────────────────────────────

#[test]
fn visitor_handles_index_expressions() {
    let span = Span::new(FileId(0), 0, 1);
    let index = Expr::Index {
        base: Box::new(Expr::Path(vec!["arr".into()], span)),
        index: Box::new(Expr::Int("0".into(), span)),
        span,
    };
    let mut v = CountingVisitor::new();
    v.visit_expr(&index);
    // Index(1) + Path(1) + Int(1)
    assert_eq!(v.expr_count, 3);
}

#[test]
fn visitor_handles_deref_propagate_jmp() {
    let span = Span::new(FileId(0), 0, 1);
    let inner = Expr::Path(vec!["x".into()], span);

    let mut v = CountingVisitor::new();
    v.visit_expr(&Expr::Deref(Box::new(inner.clone()), span));
    assert_eq!(v.expr_count, 2);

    let mut v = CountingVisitor::new();
    v.visit_expr(&Expr::Propagate(Box::new(inner.clone()), span));
    assert_eq!(v.expr_count, 2);

    let mut v = CountingVisitor::new();
    v.visit_expr(&Expr::Jmp {
        target: Some(Box::new(inner.clone())),
        label: None,
        span,
    });
    assert_eq!(v.expr_count, 2);
}

#[test]
fn visitor_handles_skip() {
    let span = Span::new(FileId(0), 0, 1);
    let mut v = CountingVisitor::new();
    v.visit_expr(&Expr::Skip { label: None, span });
    assert_eq!(v.expr_count, 1);
}

#[test]
fn visitor_handles_stop() {
    let span = Span::new(FileId(0), 0, 1);

    let mut v = CountingVisitor::new();
    v.visit_expr(&Expr::Stop { label: Some("outer".to_string()), span });
    assert_eq!(v.expr_count, 1);

    let mut v = CountingVisitor::new();
    v.visit_expr(&Expr::Stop { label: None, span });
    assert_eq!(v.expr_count, 1);
}

#[test]
fn visitor_handles_ret_without_value() {
    let span = Span::new(FileId(0), 0, 1);
    let mut v = CountingVisitor::new();
    v.visit_expr(&Expr::Ret(None, span));
    assert_eq!(v.expr_count, 1);
}

#[test]
fn visitor_handles_loop_with_subject() {
    let span = Span::new(FileId(0), 0, 1);
    let loop_expr = Expr::Loop {
        subject: Some(Box::new(Expr::Path(vec!["iter".into()], span))),
        body: Box::new(Expr::Int("42".into(), span)),
        label: None,
        span,
    };
    let mut v = CountingVisitor::new();
    v.visit_expr(&loop_expr);
    // Loop(1) + Path(1) + Int(1)
    assert_eq!(v.expr_count, 3);
}

#[test]
fn visitor_handles_let_without_type_annotation() {
    let span = Span::new(FileId(0), 0, 1);
    let stmt = Stmt::Let {
        name: "x".into(),
        is_mut: false,
        ty: None,
        init: Expr::Int("1".into(), span),
        span,
    };
    let mut v = CountingVisitor::new();
    v.visit_stmt(&stmt);
    assert_eq!(v.stmt_count, 1);
    assert_eq!(v.expr_count, 1);
}

/// 专用遍历访问器：调用 walk_item 以覆盖 Struct/Union/Use 分支
struct WalkingVisitor {
    item_count: usize,
    type_count: usize,
}

impl WalkingVisitor {
    fn new() -> Self { Self { item_count: 0, type_count: 0 } }
}

impl Visitor for WalkingVisitor {
    fn visit_item(&mut self, it: &Item) {
        self.item_count += 1;
        walk_item(self, it);
    }
    fn visit_type(&mut self, _t: &TypeExpr) {
        self.type_count += 1;
    }
}

#[test]
fn walk_item_covers_struct() {
    let span = Span::new(FileId(0), 0, 1);
    let s = Item::Struct(StructDef { is_public: false,
        name: "Foo".into(),
        fields: vec![
            Field { name: "x".into(), ty: TypeExpr::Named("i32".into(), span), span },
        ],
        span,
    });
    let mut v = WalkingVisitor::new();
    v.visit_item(&s);
    assert_eq!(v.item_count, 1);
    assert_eq!(v.type_count, 1);
}

#[test]
fn walk_item_covers_union() {
    let span = Span::new(FileId(0), 0, 1);
    let u = Item::Union(UnionDef { is_public: false,
        name: "Bar".into(),
        variants: vec![Variant {
            name: "A".into(),
            payload: vec![TypeExpr::Named("i32".into(), span)],
            span,
        }],
        span,
    });
    let mut v = WalkingVisitor::new();
    v.visit_item(&u);
    assert_eq!(v.item_count, 1);
    assert_eq!(v.type_count, 1);
}

#[test]
fn walk_item_covers_use() {
    let span = Span::new(FileId(0), 0, 1);
    let u = Item::Use(UsePath { segments: vec!["std".into()], span });
    let mut v = WalkingVisitor::new();
    v.visit_item(&u);
    assert_eq!(v.item_count, 1);
    assert_eq!(v.type_count, 0);
}

#[test]
fn visitor_handles_assign_statement() {
    let span = Span::new(FileId(0), 0, 1);
    let stmt = Stmt::Assign {
        target: Expr::Path(vec!["x".into()], span),
        value: Expr::Int("1".into(), span),
        span,
    };
    let mut v = CountingVisitor::new();
    v.visit_stmt(&stmt);
    assert_eq!(v.stmt_count, 1);
    // Path(1) + Int(1) = 2
    assert_eq!(v.expr_count, 2);
}

#[test]
fn walk_item_covers_func_with_params_and_types() {
    let span = Span::new(FileId(0), 0, 1);
    let func = Item::Func(Func { is_public: false,
        name: "f".into(),
        params: vec![Param {
            name: "x".into(),
            ty: TypeExpr::Named("i32".into(), span),
            is_mut: false,
            span,
        }],
        ret: Some(TypeExpr::Named("i32".into(), span)),
        err: Some(TypeExpr::Named("Err".into(), span)),
        body: Expr::Int("0".into(), span),
        span,
    });
    let mut v = WalkingVisitor::new();
    v.visit_item(&func);
    // param type + ret type + err type = 3
    assert_eq!(v.type_count, 3);
}
