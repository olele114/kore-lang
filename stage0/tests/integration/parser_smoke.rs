//! Parser 集成测试：验证 lexer → parser 管道。

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::ast::node::{Expr, Item, Stmt};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;

#[test]
fn parse_empty_source() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "", &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert!(module.items.is_empty());
    assert!(!sink.has_errors());
}

#[test]
fn parse_simple_binding() {
    let mut sink = DiagSink::new();
    let source = "x :: () i32 => 42";
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    if let Some(Item::Func(f)) = module.items.first() {
        assert_eq!(f.name, "x");
    } else {
        panic!("Expected function item");
    }
    assert!(!sink.has_errors());
}

#[test]
fn parse_function_definition() {
    let mut sink = DiagSink::new();
    let source = "add :: (a i32, b i32) i32 => a + b";
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    if let Some(Item::Func(f)) = module.items.first() {
        assert_eq!(f.name, "add");
        assert_eq!(f.params.len(), 2);
    } else {
        panic!("Expected function item");
    }
    assert!(!sink.has_errors());
}

#[test]
fn parse_struct_definition() {
    let mut sink = DiagSink::new();
    let source = "Point :: { x f32, y f32 }";
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    if let Some(Item::Struct(s)) = module.items.first() {
        assert_eq!(s.name, "Point");
        assert_eq!(s.fields.len(), 2);
    } else {
        panic!("Expected struct item");
    }
    assert!(!sink.has_errors());
}

#[test]
fn parse_union_definition() {
    let mut sink = DiagSink::new();
    let source = "Result :: . Ok(i32) | . Err(str)";
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    if let Some(Item::Union(u)) = module.items.first() {
        assert_eq!(u.name, "Result");
        assert_eq!(u.variants.len(), 2);
    } else {
        panic!("Expected union item");
    }
    assert!(!sink.has_errors());
}

#[test]
fn parse_block_expression() {
    let mut sink = DiagSink::new();
    let source = r#"
main :: () void => {
    x := 1
    y := 2
    x + y
}
"#;
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    if let Some(Item::Func(f)) = module.items.first() {
        if let Expr::Block { stmts, .. } = &f.body {
            assert_eq!(stmts.len(), 3);
        } else {
            panic!("Expected block body");
        }
    }
    assert!(!sink.has_errors());
}

#[test]
fn parse_branch_guard() {
    let mut sink = DiagSink::new();
    let source = r#"
check :: (x i32) void => {
    ? x < 0 => ret nil
}
"#;
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    if let Some(Item::Func(f)) = module.items.first()
        && let Expr::Block { stmts, .. } = &f.body
    {
        if let Stmt::Expr(Expr::Branch { arms, .. }) = &stmts[0] {
            assert_eq!(arms.len(), 1);
        } else {
            panic!("Expected branch statement");
        }
    }
    assert!(!sink.has_errors());
}

#[test]
fn parse_conditional_chain() {
    let mut sink = DiagSink::new();
    let source = r#"
grade :: (s i32) str => ? {
    s >= 90 => "A"
    s >= 80 => "B"
    _ => "F"
}
"#;
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    if let Some(Item::Func(f)) = module.items.first() {
        if let Expr::Branch { arms, .. } = &f.body {
            assert_eq!(arms.len(), 3);
        } else {
            panic!("Expected branch expression");
        }
    }
    assert!(!sink.has_errors());
}

#[test]
fn parse_loop_infinite() {
    let mut sink = DiagSink::new();
    let source = r#"
run :: () void => @ {
    work()
}
"#;
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    if let Some(Item::Func(f)) = module.items.first() {
        matches!(f.body, Expr::Loop { .. });
    }
    assert!(!sink.has_errors());
}

#[test]
fn parse_loop_conditional() {
    let mut sink = DiagSink::new();
    let source = r#"
gcd :: (a u64, b u64) u64 => {
    ~x, ~y := a, b
    @ (y != 0) {
        x, y = y, x % y
    }
    ret x
}
"#;
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    assert!(!sink.has_errors());
}

#[test]
fn parse_binary_expressions() {
    let mut sink = DiagSink::new();
    let source = "calc :: () i32 => 1 + 2 * 3";
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    if let Some(Item::Func(f)) = module.items.first() {
        // 应该解析为 1 + (2 * 3)，因为 * 优先级更高
        if let Expr::Binary { op: "+", rhs, .. } = &f.body {
            matches!(**rhs, Expr::Binary { op: "*", .. });
        } else {
            panic!("Expected binary expression with correct precedence");
        }
    }
    assert!(!sink.has_errors());
}

#[test]
fn parse_postfix_operators() {
    let mut sink = DiagSink::new();
    let source = "deref :: (p ^i32) i32 => p^";
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    if let Some(Item::Func(f)) = module.items.first() {
        matches!(f.body, Expr::Deref(..));
    }
    assert!(!sink.has_errors());
}

#[test]
fn parse_function_call() {
    let mut sink = DiagSink::new();
    let source = "run :: () void => work(1, 2)";
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    if let Some(Item::Func(f)) = module.items.first() {
        if let Expr::Call { args, .. } = &f.body {
            assert_eq!(args.len(), 2);
        } else {
            panic!("Expected call expression");
        }
    }
    assert!(!sink.has_errors());
}

#[test]
fn parse_field_access() {
    let mut sink = DiagSink::new();
    let source = "get_x :: (p Point) f32 => p.x";
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    if let Some(Item::Func(f)) = module.items.first() {
        if let Expr::Field { name, .. } = &f.body {
            assert_eq!(name, "x");
        } else {
            panic!("Expected field access");
        }
    }
    assert!(!sink.has_errors());
}

#[test]
fn parse_control_flow_statements() {
    let mut sink = DiagSink::new();
    let source = r#"
loop_with_controls :: () void => @ {
    ? done() => stop
    ? skip_this() => skip
    work()
}
"#;
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(module.items.len(), 1);
    assert!(!sink.has_errors());
}
