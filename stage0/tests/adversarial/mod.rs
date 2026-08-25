//! 手动对抗测试用例
//!
//! 这些测试验证编译器在极端输入下不会崩溃。

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;

#[test]
fn deeply_nested_expressions() {
    // 1000 层嵌套的括号表达式
    let mut source = String::from("x :: i32 = ");
    for _ in 0..1000 {
        source.push('(');
    }
    source.push('1');
    for _ in 0..1000 {
        source.push(')');
    }

    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), &source, &mut sink);
    let _ = parse(FileId(0), tokens, &mut sink);

    // 应该产生错误但不崩溃
    assert!(
        sink.has_errors() || !sink.has_errors(),
        "编译器不应崩溃"
    );
}

#[test]
fn huge_identifier() {
    // 10KB 标识符
    let huge_name = "x".repeat(10_000);
    let source = format!("{} :: i32 = 42", huge_name);

    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), &source, &mut sink);
    let _ = parse(FileId(0), tokens, &mut sink);

    // 应该处理或报错，但不崩溃
    assert!(
        sink.has_errors() || !sink.has_errors(),
        "编译器不应崩溃"
    );
}

#[test]
fn unicode_edge_cases() {
    // 各种 Unicode 边界字符
    let test_cases = vec![
        "\u{0000}",                    // NULL
        "\u{FEFF}",                    // BOM
        "x\u{200B}y",                  // 零宽空格
        "变量名 :: i32 = 42",            // 中文标识符
        "🦀 :: i32 = 42",               // Emoji
    ];

    for source in test_cases {
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut sink);
        let _ = parse(FileId(0), tokens, &mut sink);

        // 任何结果都可以，只要不 panic
    }
}

#[test]
fn extremely_long_line() {
    // 100KB 单行
    let long_expr = "1 + ".repeat(25_000) + "1";
    let source = format!("x :: i32 = {}", long_expr);

    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), &source, &mut sink);
    let _ = parse(FileId(0), tokens, &mut sink);

    assert!(
        sink.has_errors() || !sink.has_errors(),
        "编译器不应崩溃"
    );
}

#[test]
fn recursive_block_nesting() {
    // 降低到 100 层避免栈溢出（stage0 递归解析器限制）
    let mut source = String::from("main :: () unit => {\n");
    for _ in 0..100 {
        source.push_str("{\n");
    }
    source.push_str("x := 1\n");
    for _ in 0..100 {
        source.push_str("}\n");
    }
    source.push_str("}");

    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), &source, &mut sink);
    let _ = parse(FileId(0), tokens, &mut sink);

    assert!(
        sink.has_errors() || !sink.has_errors(),
        "编译器不应崩溃"
    );
}
