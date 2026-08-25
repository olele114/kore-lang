//! AST → HIR 降级集成测试。

use crate::diag::{DiagSink, FileId};
use crate::frontend::lexer::tokenize;
use crate::frontend::parser::parse;
use crate::frontend::resolve::Resolver;
use crate::frontend::typecheck::TypeChecker;
use crate::middleend::lower::lower_module;
use crate::middleend::hir::{HirModule, HirFunction};

/// 过滤内置函数，仅返回用户定义的函数
fn user_functions(module: &HirModule) -> Vec<&HirFunction> {
    const BUILTINS: &[&str] = &["print", "println", "eprint", "eprintln", "read_file", "write_file"];
    module.functions.iter()
        .filter(|f| !BUILTINS.contains(&f.name.as_str()))
        .collect()
}

fn lower_source(source: &str) -> (crate::middleend::hir::HirModule, DiagSink) {
    let mut sink = DiagSink::new();
    let file_id = FileId(0);

    // 词法分析
    let tokens = tokenize(file_id, source, &mut sink);
    if sink.has_errors() {
        return (crate::middleend::hir::HirModule {
            functions: vec![],
            structs: vec![],
            unions: vec![],
            globals: vec![],
        }, sink);
    }

    // 语法分析
    let ast = parse(file_id, tokens, &mut sink);
    if sink.has_errors() {
        return (crate::middleend::hir::HirModule {
            functions: vec![],
            structs: vec![],
            unions: vec![],
            globals: vec![],
        }, sink);
    }

    // 名称解析
    let resolver = Resolver::new(&mut sink);
    let symtab = resolver.resolve(&ast);
    if sink.has_errors() {
        return (crate::middleend::hir::HirModule {
            functions: vec![],
            structs: vec![],
            unions: vec![],
            globals: vec![],
        }, sink);
    }

    // 类型检查 - 使用独立作用域来结束 checker 的生命周期
    let type_ctx = {
        let mut checker = TypeChecker::new(&symtab, &mut sink);
        checker.check_module(&ast);
        // 克隆 TypeContext（类型信息相对较小）
        checker.type_context().clone()
    };

    // 检查类型检查阶段的错误
    if sink.has_errors() {
        return (crate::middleend::hir::HirModule {
            functions: vec![],
            structs: vec![],
            unions: vec![],
            globals: vec![],
        }, sink);
    }

    // HIR 降级
    let hir = lower_module(&ast, &symtab, &type_ctx, &mut sink);

    (hir, sink)
}

#[test]
fn test_empty_module() {
    let (hir, sink) = lower_source("");

    assert!(!sink.has_errors(), "空模块不应产生错误");
    assert_eq!(user_functions(&hir).len(), 0);
    assert_eq!(hir.structs.len(), 0);
    assert_eq!(hir.unions.len(), 0);
}

#[test]
fn test_simple_function() {
    let source = r#"
        add :: (x i32, y i32) i32 => {
            ret x + y
        }
    "#;

    let (hir, sink) = lower_source(source);

    if sink.has_errors() {
        for diag in sink.peek() {
            eprintln!("{:?}", diag);
        }
    }

    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1, "应该有一个用户函数");

    let func = user_funcs[0];
    assert_eq!(func.name, "add");
    assert_eq!(func.params.len(), 2);

    let body = func.body.as_ref().expect("函数应该有函数体");
    assert!(!body.blocks.is_empty(), "函数体应该有基本块");
}

#[test]
fn test_struct_definition() {
    let source = r#"
        Point :: { x i32, y i32 }
    "#;

    let (hir, sink) = lower_source(source);

    assert!(!sink.has_errors());
    assert_eq!(hir.structs.len(), 1);

    let s = &hir.structs[0];
    assert_eq!(s.name, "Point");
    assert_eq!(s.fields.len(), 2);
}

#[test]
fn test_variable_declaration() {
    let source = r#"
        test :: () void => {
            x := 42
        }
    "#;

    let (hir, sink) = lower_source(source);

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);

    let func = user_funcs[0];
    let body = func.body.as_ref().unwrap();

    // 应该有至少一个局部变量（x）
    assert!(!body.locals.is_empty());
}

#[test]
fn test_branch_statement() {
    let source = r#"
        test :: (x i32) i32 => {
            ret ? {
                x > 0 => 1,
                _ => 0
            }
        }
    "#;

    let (hir, sink) = lower_source(source);

    if sink.has_errors() {
        for diag in sink.peek() {
            eprintln!("{:?}", diag);
        }
    }

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);

    let func = user_funcs[0];
    let body = func.body.as_ref().unwrap();

    // Branch 应该产生多个基本块
    assert!(body.blocks.len() >= 2, "Branch 应该产生至少 2 个块");
}

#[test]
fn test_loop_statement() {
    let source = r#"
        test :: () void => {
            @ {
                stop
            }
        }
    "#;

    let (hir, sink) = lower_source(source);

    if sink.has_errors() {
        for diag in sink.peek() {
            eprintln!("{:?}", diag);
        }
    }

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);

    let func = user_funcs[0];
    let body = func.body.as_ref().unwrap();

    // loop 应该产生多个块
    assert!(body.blocks.len() >= 2);
}

#[test]
fn test_binary_operations() {
    let source = r#"
        test :: (a i32, b i32) i32 => {
            c := a + b
            d := c * 2
            ret d
        }
    "#;

    let (hir, sink) = lower_source(source);

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);

    let func = user_funcs[0];
    let body = func.body.as_ref().unwrap();

    // 应该有局部变量 c 和 d
    assert!(body.locals.len() >= 2);
}

#[test]
fn test_function_call() {
    let source = r#"
        helper :: (x i32) i32 => x + 1

        test :: () i32 => {
            ret helper(41)
        }
    "#;

    let (hir, sink) = lower_source(source);

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 2);
}

// 结构体字面量语法暂未实现，跳过此测试
#[test]
#[ignore]
fn test_struct_instantiation() {
    let source = r#"
        Point :: { x i32, y i32 }

        make_point :: () Point => {
            p := Point.[x: 10, y: 20]
            ret p
        }
    "#;

    let (hir, sink) = lower_source(source);

    if sink.has_errors() {
        for diag in sink.peek() {
            eprintln!("{:?}", diag);
        }
    }

    assert!(!sink.has_errors());
    assert_eq!(hir.structs.len(), 1);
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);
}

#[test]
fn test_field_access() {
    let source = r#"
        Point :: { x i32, y i32 }

        get_x :: (p Point) i32 => {
            ret p.x
        }
    "#;

    let (hir, sink) = lower_source(source);

    if sink.has_errors() {
        for diag in sink.peek() {
            eprintln!("{:?}", diag);
        }
    }

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);
}

#[test]
fn test_pointer_operations() {
    let source = r#"
        test :: (p own ^i32) i32 => {
            ret p^
        }
    "#;

    let (hir, sink) = lower_source(source);

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);
}

#[test]
fn test_match_expression() {
    let source = r#"
        Option :: .Some(i32) | .None

        unwrap :: (opt Option) i32 => {
            ret ? opt is {
                .Some(x) => x,
                .None => 0
            }
        }
    "#;

    let (hir, sink) = lower_source(source);

    if sink.has_errors() {
        for diag in sink.peek() {
            eprintln!("{:?}", diag);
        }
    }

    assert!(!sink.has_errors());
    assert_eq!(hir.unions.len(), 1);
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);
}

#[test]
fn test_labeled_break() {
    let source = r#"
        test :: () void => {
            @outer @ {
                @ {
                    stop @outer
                }
            }
        }
    "#;

    let (hir, sink) = lower_source(source);

    if sink.has_errors() {
        for diag in sink.peek() {
            eprintln!("{:?}", diag);
        }
    }

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);
}

#[test]
fn test_labeled_skip() {
    let source = r#"
        test :: () void => {
            @outer @ {
                @ {
                    skip @outer
                }
            }
        }
    "#;

    let (hir, sink) = lower_source(source);

    if sink.has_errors() {
        for diag in sink.peek() {
            eprintln!("{:?}", diag);
        }
    }

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);
}

#[test]
fn test_multiple_functions() {
    let source = r#"
        add :: (x i32, y i32) i32 => x + y
        sub :: (x i32, y i32) i32 => x - y
        mul :: (x i32, y i32) i32 => x * y
    "#;

    let (hir, sink) = lower_source(source);

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 3);
}

#[test]
fn test_nested_blocks() {
    let source = r#"
        test :: () i32 => {
            x := 0
            {
                y := 1
                x = y
            }
            ret x
        }
    "#;

    let (hir, sink) = lower_source(source);

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);
}

#[test]
fn test_unary_operations() {
    let source = r#"
        test :: (x i32) i32 => {
            ret -x
        }
    "#;

    let (hir, sink) = lower_source(source);

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);
}

#[test]
fn test_comparison_operations() {
    let source = r#"
        test :: (x i32, y i32) bool => {
            ret x == y
        }
    "#;

    let (hir, sink) = lower_source(source);

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);
}

// Kore 使用 & 和 | 作为位运算，没有专门的逻辑运算符
#[test]
fn test_bitwise_operations() {
    let source = r#"
        test :: (a i32, b i32) i32 => {
            ret a & b
        }
    "#;

    let (hir, sink) = lower_source(source);

    if sink.has_errors() {
        for diag in sink.peek() {
            eprintln!("{:?}", diag);
        }
    }

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);
}

#[test]
fn test_error_union_type() {
    let source = r#"
        divide :: (a i32, b i32) i32 ! str => {
            ? b == 0 => ret .Err("division by zero")
            ret .Ok(a / b)
        }
    "#;

    let (hir, sink) = lower_source(source);

    if sink.has_errors() {
        for diag in sink.peek() {
            eprintln!("{:?}", diag);
        }
    }

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 1);

    let func = user_funcs[0];
    let body = func.body.as_ref().unwrap();

    // 错误联合应该产生多个基本块（成功和错误路径）
    assert!(body.blocks.len() >= 2);
}

#[test]
fn test_error_propagation() {
    let source = r#"
        divide :: (a i32, b i32) i32 ! str => {
            ? b == 0 => ret .Err("division by zero")
            ret .Ok(a / b)
        }

        safe_divide :: (x i32, y i32) i32 ! str => {
            result := divide(x, y)!
            ret .Ok(result)
        }
    "#;

    let (hir, sink) = lower_source(source);

    if sink.has_errors() {
        for diag in sink.peek() {
            eprintln!("{:?}", diag);
        }
    }

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 2);

    // safe_divide 函数应该有 Propagate 展开的控制流
    let safe_divide = user_funcs.iter()
        .find(|f| f.name == "safe_divide")
        .expect("应该找到 safe_divide 函数");

    let body = safe_divide.body.as_ref().unwrap();

    // Propagate 应该展开为：
    // 1. 调用 divide
    // 2. 检查 discriminant
    // 3. 错误分支：提前返回
    // 4. 成功分支：继续执行
    assert!(body.blocks.len() >= 3, "Propagate 应该产生至少 3 个块");
}

#[test]
fn test_error_chaining() {
    let source = r#"
        divide :: (a i32, b i32) i32 ! str => {
            ? b == 0 => ret .Err("division by zero")
            ret .Ok(a / b)
        }

        complex :: (x i32, y i32, z i32) i32 ! str => {
            result1 := divide(x, y)!
            result2 := divide(result1, z)!
            ret .Ok(result2)
        }
    "#;

    let (hir, sink) = lower_source(source);

    if sink.has_errors() {
        for diag in sink.peek() {
            eprintln!("{:?}", diag);
        }
    }

    assert!(!sink.has_errors());
    let user_funcs = user_functions(&hir);
    assert_eq!(user_funcs.len(), 2);

    let complex = user_funcs.iter()
        .find(|f| f.name == "complex")
        .expect("应该找到 complex 函数");

    let body = complex.body.as_ref().unwrap();

    // 两个 Propagate 调用应该产生更多基本块
    assert!(body.blocks.len() >= 5, "链式 Propagate 应该产生至少 5 个块");
}

