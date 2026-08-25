//! AST printer 覆盖率补充测试。
//!
//! 现有测试只覆盖了 printer 的基础功能，此文件补充各类 AST 节点的打印。

use kore_stage0::diag::{FileId, Span};
use kore_stage0::frontend::ast::node::*;
use kore_stage0::frontend::ast::printer::{print_module, PrintOpts};

fn sp() -> Span {
    Span { file: FileId(0), lo: 0, hi: 1 }
}

#[test]
fn prints_function_with_params_and_return() {
    let func = Item::Func(Func {
        name: "add".into(),
        params: vec![
            Param { name: "x".into(), ty: TypeExpr::Named("i32".into(), sp()), is_mut: false, span: sp() },
            Param { name: "y".into(), ty: TypeExpr::Named("i32".into(), sp()), is_mut: true, span: sp() },
        ],
        ret: Some(TypeExpr::Named("i32".into(), sp())),
        err: None,
        body: Expr::Int("42".into(), sp()),
        span: sp(),
        is_public: false,
    });

    let module = Module { items: vec![func], span: sp() };
    let output = print_module(&module, PrintOpts::default());

    assert!(output.contains("add"));
    assert!(output.contains("i32"));
}

#[test]
fn prints_function_with_error_type() {
    let func = Item::Func(Func {
        name: "fallible".into(),
        params: vec![],
        ret: Some(TypeExpr::Named("i32".into(), sp())),
        err: Some(TypeExpr::Named("Error".into(), sp())),
        body: Expr::Int("0".into(), sp()),
        span: sp(),
        is_public: false,
    });

    let module = Module { items: vec![func], span: sp() };
    let output = print_module(&module, PrintOpts::default());

    assert!(output.contains("Error"));
}

#[test]
fn prints_struct_with_fields() {
    let s = Item::Struct(StructDef {
        name: "Point".into(),
        fields: vec![
            Field { name: "x".into(), ty: TypeExpr::Named("f64".into(), sp()), span: sp() },
            Field { name: "y".into(), ty: TypeExpr::Named("f64".into(), sp()), span: sp() },
        ],
        span: sp(),
        is_public: false,
    });

    let module = Module { items: vec![s], span: sp() };
    let output = print_module(&module, PrintOpts::default());

    assert!(output.contains("Point"));
    assert!(output.contains("f64"));
}

#[test]
fn prints_union_with_variants() {
    let u = Item::Union(UnionDef {
        name: "Result".into(),
        variants: vec![
            Variant {
                name: "Ok".into(),
                payload: vec![TypeExpr::Named("i32".into(), sp())],
                span: sp(),
            },
            Variant {
                name: "Err".into(),
                payload: vec![TypeExpr::Named("String".into(), sp())],
                span: sp(),
            },
        ],
        span: sp(),
        is_public: false,
    });

    let module = Module { items: vec![u], span: sp() };
    let output = print_module(&module, PrintOpts::default());

    assert!(output.contains("Result"));
    assert!(output.contains("Ok"));
    assert!(output.contains("Err"));
}

#[test]
fn prints_use_statement() {
    let u = Item::Use(UsePath {
        segments: vec!["std".into(), "io".into(), "print".into()],
        span: sp(),
    });

    let module = Module { items: vec![u], span: sp() };
    let output = print_module(&module, PrintOpts::default());

    assert!(output.contains("std"));
    assert!(output.contains("io"));
    assert!(output.contains("print"));
}

#[test]
fn prints_let_statement_with_type() {
    let stmt = Stmt::Let {
        name: "x".into(),
        is_mut: true,
        ty: Some(TypeExpr::Named("i32".into(), sp())),
        init: Expr::Int("42".into(), sp()),
        span: sp(),
    };

    let func = Item::Func(Func {
        name: "test".into(),
        params: vec![],
        ret: None,
        err: None,
        body: Expr::Block { stmts: vec![stmt], span: sp() },
        span: sp(),
        is_public: false,
    });

    let module = Module { items: vec![func], span: sp() };
    let output = print_module(&module, PrintOpts::default());

    assert!(output.contains("i32"));
}

#[test]
fn prints_assign_statement() {
    let stmt = Stmt::Assign {
        target: Expr::Path(vec!["x".into()], sp()),
        value: Expr::Int("10".into(), sp()),
        span: sp(),
    };

    let func = Item::Func(Func {
        name: "test".into(),
        params: vec![],
        ret: None,
        err: None,
        body: Expr::Block { stmts: vec![stmt], span: sp() },
        span: sp(),
        is_public: false,
    });

    let module = Module { items: vec![func], span: sp() };
    let output = print_module(&module, PrintOpts::default());

    assert!(output.contains("test"));
}

#[test]
fn prints_defer_statement() {
    let stmt = Stmt::Defer(Expr::Path(vec!["cleanup".into()], sp()), sp());

    let func = Item::Func(Func {
        name: "test".into(),
        params: vec![],
        ret: None,
        err: None,
        body: Expr::Block { stmts: vec![stmt], span: sp() },
        span: sp(),
        is_public: false,
    });

    let module = Module { items: vec![func], span: sp() };
    let output = print_module(&module, PrintOpts::default());

    assert!(output.contains("cleanup"));
}

#[test]
fn prints_all_literal_types() {
    let body = Expr::Block {
        stmts: vec![
            Stmt::Expr(Expr::Int("42".into(), sp())),
            Stmt::Expr(Expr::Float("3.14".into(), sp())),
            Stmt::Expr(Expr::Str("hello".into(), sp())),
            Stmt::Expr(Expr::Bool(true, sp())),
            Stmt::Expr(Expr::Nil(sp())),
        ],
        span: sp(),
    };

    let func = Item::Func(Func {
        name: "test".into(),
        params: vec![],
        ret: None,
        err: None,
        body,
        span: sp(),
        is_public: false,
    });

    let module = Module { items: vec![func], span: sp() };
    let output = print_module(&module, PrintOpts::default());

    assert!(output.contains("42"));
    assert!(output.contains("3.14"));
    assert!(output.contains("hello"));
    assert!(output.contains("true"));
}

#[test]
fn prints_branch_with_scrutinee() {
    let branch = Expr::Branch {
        scrutinee: Some(Box::new(Expr::Path(vec!["x".into()], sp()))),
        arms: vec![
            Arm {
                pattern: Pattern::Variant {
                    name: "Some".into(),
                    bindings: vec!["v".into()],
                    span: sp(),
                },
                body: Expr::Path(vec!["v".into()], sp()),
                span: sp(),
            },
            Arm {
                pattern: Pattern::Variant {
                    name: "None".into(),
                    bindings: vec![],
                    span: sp(),
                },
                body: Expr::Int("0".into(), sp()),
                span: sp(),
            },
        ],
        span: sp(),
    };

    let func = Item::Func(Func {
        name: "test".into(),
        params: vec![],
        ret: None,
        err: None,
        body: branch,
        span: sp(),
        is_public: false,
    });

    let module = Module { items: vec![func], span: sp() };
    let output = print_module(&module, PrintOpts::default());

    assert!(output.contains("test"));
}

// ── 补充：覆盖尚未命中的 printer 分支 ────────────────────────────────────

fn make_func(body: Expr) -> Module {
    let func = Item::Func(Func {
        name: "t".into(),
        params: vec![],
        ret: None,
        err: None,
        body,
        span: sp(),
        is_public: false,
    });
    Module { items: vec![func], span: sp() }
}

fn make_branch(pat: Pattern) -> Module {
    let branch = Expr::Branch {
        scrutinee: None,
        arms: vec![Arm { pattern: pat, body: Expr::Int("1".into(), sp()), span: sp() }],
        span: sp(),
    };
    make_func(branch)
}

#[test]
fn prints_index_expression() {
    let body = Expr::Index {
        base: Box::new(Expr::Path(vec!["arr".into()], sp())),
        index: Box::new(Expr::Int("0".into(), sp())),
        span: sp(),
    };
    let out = print_module(&make_func(body), PrintOpts::default());
    assert!(out.contains("index"));
}

#[test]
fn prints_deref_expression() {
    let body = Expr::Deref(Box::new(Expr::Path(vec!["p".into()], sp())), sp());
    let out = print_module(&make_func(body), PrintOpts::default());
    assert!(out.contains("deref"));
}

#[test]
fn prints_propagate_expression() {
    let body = Expr::Propagate(Box::new(Expr::Path(vec!["r".into()], sp())), sp());
    let out = print_module(&make_func(body), PrintOpts::default());
    assert!(out.contains("propagate"));
}

#[test]
fn prints_jmp_expression() {
    let body = Expr::Jmp {
        target: Some(Box::new(Expr::Path(vec!["lbl".into()], sp()))),
        label: None,
        span: sp(),
    };
    let out = print_module(&make_func(body), PrintOpts::default());
    assert!(out.contains("jmp"));
}

#[test]
fn prints_skip_expression() {
    let out = print_module(&make_func(Expr::Skip { label: None, span: sp() }), PrintOpts::default());
    assert!(out.contains("skip"));
}

#[test]
fn prints_ret_without_value() {
    let out = print_module(&make_func(Expr::Ret(None, sp())), PrintOpts::default());
    assert!(out.contains("ret"));
}

#[test]
fn prints_stop_with_label() {
    let body = Expr::Stop { label: Some("outer".to_string()), span: sp() };
    let out = print_module(&make_func(body), PrintOpts::default());
    assert!(out.contains("stop @outer"));
}

#[test]
fn prints_stop_without_label() {
    let out = print_module(&make_func(Expr::Stop { label: None, span: sp() }), PrintOpts::default());
    assert!(out.contains("stop"));
}

#[test]
fn prints_pattern_bind() {
    let out = print_module(&make_branch(Pattern::Bind("x".into(), sp())), PrintOpts::default());
    assert!(out.contains("pat-bind"));
}

#[test]
fn prints_pattern_lit() {
    let out = print_module(
        &make_branch(Pattern::Lit(Box::new(Expr::Int("42".into(), sp())))),
        PrintOpts::default(),
    );
    assert!(out.contains("pat-lit"));
}

#[test]
fn prints_pattern_wildcard() {
    let out = print_module(&make_branch(Pattern::Wildcard(sp())), PrintOpts::default());
    assert!(out.contains("pat-wildcard"));
}

#[test]
fn prints_pattern_cond() {
    let out = print_module(
        &make_branch(Pattern::Cond(Box::new(Expr::Bool(true, sp())))),
        PrintOpts::default(),
    );
    assert!(out.contains("pat-cond"));
}

#[test]
fn prints_loop_without_subject() {
    let body = Expr::Loop {
        subject: None,
        body: Box::new(Expr::Skip { label: None, span: sp() }),
        label: None,
        span: sp(),
    };
    let out = print_module(&make_func(body), PrintOpts::default());
    assert!(out.contains("loop"));
    assert!(out.contains("skip"));
}

#[test]
fn prints_loop_with_subject() {
    let body = Expr::Loop {
        subject: Some(Box::new(Expr::Path(vec!["it".into()], sp()))),
        body: Box::new(Expr::Skip { label: None, span: sp() }),
        label: None,
        span: sp(),
    };
    let out = print_module(&make_func(body), PrintOpts::default());
    assert!(out.contains("loop"));
    assert!(out.contains("it"));
}

#[test]
fn prints_call_with_args() {
    let body = Expr::Call {
        callee: Box::new(Expr::Path(vec!["foo".into()], sp())),
        args: vec![Expr::Int("1".into(), sp()), Expr::Int("2".into(), sp())],
        span: sp(),
    };
    let out = print_module(&make_func(body), PrintOpts::default());
    assert!(out.contains("call"));
    assert!(out.contains("foo"));
}

#[test]
fn prints_field_access() {
    let body = Expr::Field {
        base: Box::new(Expr::Path(vec!["obj".into()], sp())),
        name: "x".into(),
        span: sp(),
    };
    let out = print_module(&make_func(body), PrintOpts::default());
    assert!(out.contains("field x"));
    assert!(out.contains("obj"));
}

#[test]
fn prints_unary_expression() {
    let body = Expr::Unary {
        op: "-",
        operand: Box::new(Expr::Int("1".into(), sp())),
        span: sp(),
    };
    let out = print_module(&make_func(body), PrintOpts::default());
    assert!(out.contains("unary -"));
}

#[test]
fn prints_binary_expression() {
    let body = Expr::Binary {
        op: "+",
        lhs: Box::new(Expr::Int("1".into(), sp())),
        rhs: Box::new(Expr::Int("2".into(), sp())),
        span: sp(),
    };
    let out = print_module(&make_func(body), PrintOpts::default());
    assert!(out.contains("binary +"));
}

#[test]
fn prints_ret_with_value() {
    let body = Expr::Ret(Some(Box::new(Expr::Int("0".into(), sp()))), sp());
    let out = print_module(&make_func(body), PrintOpts::default());
    assert!(out.contains("ret"));
    assert!(out.contains("int 0"));
}

#[test]
fn prints_borrow_type() {
    let func = Item::Func(Func {
        name: "g".into(),
        params: vec![Param {
            name: "p".into(),
            ty: TypeExpr::Borrow(Box::new(TypeExpr::Named("i32".into(), sp())), sp()),
            is_mut: false,
            span: sp(),
        }],
        ret: None,
        err: None,
        body: Expr::Nil(sp()),
        span: sp(),
        is_public: false,
    });
    let out = print_module(&Module { items: vec![func], span: sp() }, PrintOpts::default());
    assert!(out.contains("^i32"));
}

#[test]
fn prints_let_without_type_annotation() {
    let stmt = Stmt::Let {
        name: "x".into(),
        is_mut: false,
        ty: None,
        init: Expr::Int("5".into(), sp()),
        span: sp(),
    };
    let body = Expr::Block { stmts: vec![stmt], span: sp() };
    let out = print_module(&make_func(body), PrintOpts::default());
    assert!(out.contains("let x _"));
}
