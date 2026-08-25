//! Parser 单元测试：覆盖 expr.rs / decl.rs / stmt.rs 的未覆盖路径。

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::ast::node::{Expr, Item, Stmt};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;

// ─── 辅助函数 ────────────────────────────────────────────────────────────────

fn parse_src(src: &str) -> (kore_stage0::frontend::ast::node::Module, DiagSink) {
    let mut sink = DiagSink::new();
    let toks = tokenize(FileId(0), src, &mut sink);
    let module = parse(FileId(0), toks, &mut sink);
    (module, sink)
}

/// 从顶层函数的 body 里拿第一条语句，函数签名为 `f :: () => { ... }`
fn first_stmt(src: &str) -> Stmt {
    let (module, _) = parse_src(src);
    match module.items.first().expect("应有顶层函数") {
        Item::Func(f) => match &f.body {
            Expr::Block { stmts, .. } => stmts.first().cloned().expect("块里应有语句"),
            other => panic!("body 应是 Block，实际：{:?}", other),
        },
        other => panic!("应是 Func，实际：{:?}", other),
    }
}

/// 从顶层函数的 body 表达式（非块）直接获取
fn func_body(src: &str) -> Expr {
    let (module, _) = parse_src(src);
    match module.items.first().expect("应有顶层函数") {
        Item::Func(f) => f.body.clone(),
        other => panic!("应是 Func，实际：{:?}", other),
    }
}

// ─── stmt.rs 路径 ────────────────────────────────────────────────────────────

#[test]
fn stmt_defer_expression() {
    let stmt = first_stmt("f :: () => { defer cleanup() }");
    assert!(matches!(stmt, Stmt::Defer(..)), "应是 Defer，实际：{:?}", stmt);
}

#[test]
fn stmt_let_typed_binding() {
    // x : i32 = 42
    let stmt = first_stmt("f :: () => { x : i32 = 42 }");
    match stmt {
        Stmt::Let { name, is_mut, ty, .. } => {
            assert_eq!(name, "x");
            assert!(!is_mut);
            assert!(ty.is_some(), "应有类型注解");
        }
        other => panic!("应是 Let，实际：{:?}", other),
    }
}

#[test]
fn stmt_let_inferred_binding() {
    // x := 99
    let stmt = first_stmt("f :: () => { x := 99 }");
    match stmt {
        Stmt::Let { name, is_mut, ty, .. } => {
            assert_eq!(name, "x");
            assert!(!is_mut);
            assert!(ty.is_none(), "推断绑定不应有类型注解");
        }
        other => panic!("应是 Let，实际：{:?}", other),
    }
}

#[test]
fn stmt_mutable_inferred_binding_via_postfix_assign() {
    // ~x := 0   ← 通过 := 后解析 Unary ~ 路径
    let stmt = first_stmt("f :: () => { ~x := 0 }");
    match stmt {
        Stmt::Let { name, is_mut, .. } => {
            assert_eq!(name, "x");
            assert!(is_mut, "应是可变绑定");
        }
        other => panic!("应是可变 Let，实际：{:?}", other),
    }
}

#[test]
fn stmt_assignment() {
    // x = 5
    let stmt = first_stmt("f :: () => { x = 5 }");
    assert!(matches!(stmt, Stmt::Assign { .. }), "应是 Assign，实际：{:?}", stmt);
}

#[test]
fn stmt_comptime_error_path() {
    // 在运行期上下文里写 :: 绑定 → 解析器产生错误诊断，不崩溃
    let (_, sink) = parse_src("f :: () => { x :: 42 }");
    // 不要求具体诊断码，只要不 panic
    let _ = sink.finish();
}

// ─── decl.rs 路径 ────────────────────────────────────────────────────────────

#[test]
fn decl_use_single_segment() {
    let (module, sink) = parse_src("use io");
    assert!(!sink.has_errors());
    assert!(matches!(module.items.first(), Some(Item::Use(_))));
}

#[test]
fn decl_use_multi_segment() {
    let (module, sink) = parse_src("use std.io.fs");
    assert!(!sink.has_errors());
    match module.items.first() {
        Some(Item::Use(path)) => assert_eq!(path.segments.len(), 3),
        other => panic!("应是 Use，实际：{:?}", other),
    }
}

#[test]
fn decl_func_mutable_param() {
    let (module, sink) = parse_src("f :: (~x i32) i32 => x");
    assert!(!sink.has_errors());
    match module.items.first() {
        Some(Item::Func(func)) => {
            assert_eq!(func.params.len(), 1);
            assert!(func.params[0].is_mut, "参数应为 mutable");
        }
        other => panic!("应是 Func，实际：{:?}", other),
    }
}

#[test]
fn decl_func_with_error_type() {
    // i32 ! Err 整体被 parse_type 解析为 TypeExpr::ErrUnion，存入 func.ret。
    // func.err 字段用于将来的 ! ErrType 独立语法，此处为 None。
    let (module, sink) = parse_src("f :: () i32 ! Err => 0");
    assert!(!sink.has_errors());
    match module.items.first() {
        Some(Item::Func(func)) => {
            use kore_stage0::frontend::ast::node::TypeExpr;
            assert!(
                matches!(func.ret, Some(TypeExpr::ErrUnion(..))),
                "ret 应是 ErrUnion，实际：{:?}", func.ret
            );
        }
        other => panic!("应是 Func，实际：{:?}", other),
    }
}

#[test]
fn decl_func_with_block_body() {
    let (module, sink) = parse_src("f :: () i32 => { 42 }");
    assert!(!sink.has_errors());
    match module.items.first() {
        Some(Item::Func(func)) => {
            assert!(matches!(func.body, Expr::Block { .. }), "应是 Block body");
        }
        other => panic!("应是 Func，实际：{:?}", other),
    }
}

#[test]
fn decl_func_no_return_type() {
    // 无返回类型：=> 直接接 body
    let (module, sink) = parse_src("f :: () => 0");
    assert!(!sink.has_errors());
    match module.items.first() {
        Some(Item::Func(func)) => {
            assert!(func.ret.is_none(), "无返回类型");
        }
        other => panic!("应是 Func，实际：{:?}", other),
    }
}

#[test]
fn decl_union_single_variant_with_payload() {
    let (module, sink) = parse_src("T :: .Some(i32)");
    assert!(!sink.has_errors());
    match module.items.first() {
        Some(Item::Union(u)) => {
            assert_eq!(u.variants.len(), 1);
            assert_eq!(u.variants[0].payload.len(), 1);
        }
        other => panic!("应是 Union，实际：{:?}", other),
    }
}

#[test]
fn decl_union_multi_variants_with_pipe() {
    let (module, sink) = parse_src("Shape :: .Circle(f32) | .Rect(f32, f32)");
    assert!(!sink.has_errors());
    match module.items.first() {
        Some(Item::Union(u)) => {
            assert_eq!(u.variants.len(), 2);
            assert_eq!(u.variants[1].payload.len(), 2);
        }
        other => panic!("应是 Union，实际：{:?}", other),
    }
}

#[test]
fn decl_struct_multiple_fields() {
    let (module, sink) = parse_src("Point3 :: { x f32, y f32, z f32 }");
    assert!(!sink.has_errors());
    match module.items.first() {
        Some(Item::Struct(s)) => assert_eq!(s.fields.len(), 3),
        other => panic!("应是 Struct，实际：{:?}", other),
    }
}

#[test]
fn decl_type_borrow_pointer() {
    // 使用 ^T 作为函数参数类型
    let (module, sink) = parse_src("f :: (p ^i32) => 0");
    assert!(!sink.has_errors());
    match module.items.first() {
        Some(Item::Func(func)) => {
            use kore_stage0::frontend::ast::node::TypeExpr;
            assert!(matches!(func.params[0].ty, TypeExpr::Borrow(..)));
        }
        other => panic!("应是 Func，实际：{:?}", other),
    }
}

#[test]
fn decl_type_own_pointer() {
    let (module, sink) = parse_src("f :: (p own ^i32) => 0");
    assert!(!sink.has_errors());
    match module.items.first() {
        Some(Item::Func(func)) => {
            use kore_stage0::frontend::ast::node::TypeExpr;
            assert!(matches!(func.params[0].ty, TypeExpr::Own(..)));
        }
        other => panic!("应是 Func，实际：{:?}", other),
    }
}

#[test]
fn decl_type_array() {
    let (module, sink) = parse_src("f :: (a [4]i32) => 0");
    assert!(!sink.has_errors());
    match module.items.first() {
        Some(Item::Func(func)) => {
            use kore_stage0::frontend::ast::node::TypeExpr;
            assert!(matches!(func.params[0].ty, TypeExpr::Array(..)));
        }
        other => panic!("应是 Func，实际：{:?}", other),
    }
}

#[test]
fn decl_type_err_union() {
    // parse_type 把 `i32 ! IoError` 整体解析为 TypeExpr::ErrUnion，存入 func.ret。
    let (module, sink) = parse_src("f :: () i32 ! IoError => 0");
    assert!(!sink.has_errors());
    match module.items.first() {
        Some(Item::Func(func)) => {
            use kore_stage0::frontend::ast::node::TypeExpr;
            assert!(
                matches!(func.ret, Some(TypeExpr::ErrUnion(..))),
                "ret 应是 ErrUnion，实际：{:?}", func.ret
            );
        }
        other => panic!("应是 Func，实际：{:?}", other),
    }
}

#[test]
fn decl_item_missing_double_colon_error() {
    // name 后不接 :: → 产生诊断，不崩溃
    let (_, sink) = parse_src("foo bar");
    let diags = sink.finish();
    assert!(!diags.is_empty(), "缺少 :: 应产生诊断");
}

// ─── expr.rs 路径 ────────────────────────────────────────────────────────────

#[test]
fn expr_unary_neg() {
    let body = func_body("f :: () => -42");
    assert!(matches!(body, Expr::Unary { op: "-", .. }), "应是一元负，实际：{:?}", body);
}

#[test]
fn expr_unary_not() {
    let body = func_body("f :: () => not true");
    assert!(matches!(body, Expr::Unary { op: "not", .. }), "应是 not，实际：{:?}", body);
}

#[test]
fn expr_unary_tilde() {
    let body = func_body("f :: () => ~x");
    assert!(matches!(body, Expr::Unary { op: "~", .. }), "应是 ~，实际：{:?}", body);
}

#[test]
fn expr_parenthesized() {
    // (1 + 2) * 3 — 括号内表达式被直接返回
    let body = func_body("f :: () => (1 + 2) * 3");
    assert!(matches!(body, Expr::Binary { op: "*", .. }), "应是乘法，实际：{:?}", body);
}

#[test]
fn expr_deref_postfix() {
    let body = func_body("f :: () => x^");
    assert!(matches!(body, Expr::Deref(..)), "应是 Deref，实际：{:?}", body);
}

#[test]
fn expr_propagate_postfix() {
    let body = func_body("f :: () => x!");
    assert!(matches!(body, Expr::Propagate(..)), "应是 Propagate，实际：{:?}", body);
}

#[test]
fn expr_index_postfix() {
    let body = func_body("f :: () => arr[0]");
    assert!(matches!(body, Expr::Index { .. }), "应是 Index，实际：{:?}", body);
}

#[test]
fn expr_ret_with_value() {
    let body = func_body("f :: () i32 => ret 42");
    match body {
        Expr::Ret(Some(_), _) => {}
        other => panic!("应是 Ret(Some), 实际：{:?}", other),
    }
}

#[test]
fn expr_ret_without_value() {
    let body = func_body("f :: () => { ret }");
    match body {
        Expr::Block { stmts, .. } => {
            match stmts.first() {
                Some(Stmt::Expr(Expr::Ret(None, _))) => {}
                other => panic!("应是 Ret(None)，实际：{:?}", other),
            }
        }
        other => panic!("应是 Block，实际：{:?}", other),
    }
}

#[test]
fn expr_stop_with_label() {
    let body = func_body("f :: () => stop @outer");
    match body {
        Expr::Stop { label: Some(_), .. } => {}
        other => panic!("应是 Stop with label，实际：{:?}", other),
    }
}

#[test]
fn expr_stop_without_value() {
    let body = func_body("f :: () => { stop }");
    match body {
        Expr::Block { stmts, .. } => {
            match stmts.first() {
                Some(Stmt::Expr(Expr::Stop { label: None, .. })) => {}
                other => panic!("应是 Stop(None)，实际：{:?}", other),
            }
        }
        other => panic!("应是 Block，实际：{:?}", other),
    }
}

#[test]
fn expr_branch_guard_form() {
    // ? cond => body
    let body = func_body("f :: () => ? x => 1");
    assert!(matches!(body, Expr::Branch { scrutinee: None, .. }), "应是守卫分支");
}

#[test]
fn expr_branch_condition_chain() {
    // ? { cond => body }
    let body = func_body("f :: () => ? { x => 1 }");
    assert!(matches!(body, Expr::Branch { scrutinee: None, .. }), "应是条件链");
}

#[test]
fn expr_branch_with_scrutinee() {
    // ? val is { .Some(x) => x }
    let body = func_body("f :: () => ? val is { .Some(v) => v }");
    match body {
        Expr::Branch { scrutinee: Some(_), arms, .. } => {
            assert!(!arms.is_empty());
        }
        other => panic!("应是带 scrutinee 的 Branch，实际：{:?}", other),
    }
}

#[test]
fn expr_loop_infinite() {
    let body = func_body("f :: () => @ { 1 }");
    assert!(matches!(body, Expr::Loop { subject: None, .. }), "应是无限循环");
}

#[test]
fn expr_loop_with_condition() {
    let body = func_body("f :: () => @ (cond) { 1 }");
    match body {
        Expr::Loop { subject: Some(_), .. } => {}
        other => panic!("应是条件循环，实际：{:?}", other),
    }
}

#[test]
fn pattern_wildcard() {
    // `_` 被词法器当作 Ident("_")，parse_pattern 将其解析为 Pattern::Bind("_", ...)。
    // Kore 规范中 `_` 是「不关心」的普通绑定名，而非独立通配符记号。
    let body = func_body("f :: () => ? x is { _ => 0 }");
    match body {
        Expr::Branch { arms, .. } => {
            use kore_stage0::frontend::ast::node::Pattern;
            match &arms[0].pattern {
                Pattern::Bind(name, _) => assert_eq!(name.as_str(), "_"),
                other => panic!("应是 Bind(\"_\")，实际：{:?}", other),
            }
        }
        other => panic!("应是 Branch，实际：{:?}", other),
    }
}

#[test]
fn pattern_variant_with_bindings() {
    let body = func_body("f :: () => ? x is { .Some(v) => v }");
    match body {
        Expr::Branch { arms, .. } => {
            use kore_stage0::frontend::ast::node::Pattern;
            match &arms[0].pattern {
                Pattern::Variant { name, bindings, .. } => {
                    assert_eq!(name, "Some");
                    assert_eq!(bindings.len(), 1);
                }
                other => panic!("应是 Variant 模式，实际：{:?}", other),
            }
        }
        other => panic!("应是 Branch，实际：{:?}", other),
    }
}

#[test]
fn pattern_literal_int() {
    let body = func_body("f :: () => ? x is { 42 => 0 }");
    match body {
        Expr::Branch { arms, .. } => {
            use kore_stage0::frontend::ast::node::Pattern;
            assert!(matches!(arms[0].pattern, Pattern::Lit(_)));
        }
        other => panic!("应是 Branch，实际：{:?}", other),
    }
}

#[test]
fn pattern_cond_binop() {
    // 标识符后接二元运算符 → Cond 模式
    let body = func_body("f :: () => ? { x > 0 => 1 }");
    match body {
        Expr::Branch { arms, .. } => {
            use kore_stage0::frontend::ast::node::Pattern;
            assert!(matches!(arms[0].pattern, Pattern::Cond(_)));
        }
        other => panic!("应是 Branch，实际：{:?}", other),
    }
}

#[test]
fn expr_nil_literal() {
    // `nil` 不是关键字（规范将其列为字面量/内置名），词法器产生 Ident("nil")，
    // 解析器将其解析为 Expr::Path(["nil"])。
    let body = func_body("f :: () => nil");
    match &body {
        Expr::Path(segs, _) => assert_eq!(segs[0].as_str(), "nil"),
        other => panic!("应是 Path([\"nil\"])，实际：{:?}", other),
    }
}

#[test]
fn expr_bool_true() {
    // `true` 同 `nil`，走 Ident → Path 路径。
    let body = func_body("f :: () => true");
    match &body {
        Expr::Path(segs, _) => assert_eq!(segs[0].as_str(), "true"),
        other => panic!("应是 Path([\"true\"])，实际：{:?}", other),
    }
}

#[test]
fn expr_bool_false() {
    // `false` 同上。
    let body = func_body("f :: () => false");
    match &body {
        Expr::Path(segs, _) => assert_eq!(segs[0].as_str(), "false"),
        other => panic!("应是 Path([\"false\"])，实际：{:?}", other),
    }
}
