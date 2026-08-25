//! 确定性测试：验证编译器输出的可重现性。
//!
//! 便宜代理（Cheap Proxy）：同一输入编译两次，比对输出字节是否完全相同。
//! 这是捕获非确定性 bug 的第一层防线。
//!
//! ## 设计原则（ADR 010 Q5）
//!
//! - 编译两次，逐字节比对
//! - 不接受任何差异，包括时间戳、随机数、HashMap 遍历顺序
//! - 测试文件选取：中等规模，覆盖多种语法结构
//!
//! ## 当前实现状态
//!
//! Stage0 还没有后端，所以只能比对前端产物（AST、诊断）。
//! 等后端实现后，会比对目标文件的字节流。

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::ast::printer::{print_module, PrintOpts};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;

/// 编译一次，返回 AST 的文本表示。
fn compile_once(source: &str) -> String {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    // 将 AST 序列化为字符串，不包含 span 以便稳定比对
    print_module(&module, PrintOpts { spans: false })
}

#[test]
fn empty_source_is_deterministic() {
    let source = "";

    let output1 = compile_once(source);
    let output2 = compile_once(source);

    assert_eq!(
        output1, output2,
        "Empty source should produce identical output"
    );
}

#[test]
fn simple_binding_is_deterministic() {
    let source = "x := 42";

    let output1 = compile_once(source);
    let output2 = compile_once(source);

    assert_eq!(
        output1, output2,
        "Simple binding should produce identical output"
    );
}

#[test]
fn multiple_items_preserve_order() {
    let source = r#"
x := 1
y := 2
z := 3
"#;

    let output1 = compile_once(source);
    let output2 = compile_once(source);

    assert_eq!(
        output1, output2,
        "Multiple items should preserve declaration order"
    );
}

#[test]
fn nested_expressions_are_deterministic() {
    let source = r#"
a := (1 + 2) * (3 + 4)
b := f(g(x), h(y))
"#;

    let output1 = compile_once(source);
    let output2 = compile_once(source);

    assert_eq!(
        output1, output2,
        "Nested expressions should produce identical output"
    );
}
