//! 冒烟测试：验证编译器基本功能不崩溃。
//!
//! 冒烟测试覆盖最基础的编译路径：
//! - 空文件不报错
//! - 简单有效代码能通过前端
//! - 已知错误能产生预期诊断

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;
use kore_stage0::driver::verify_test_annotations;

#[test]
fn empty_file_compiles_without_error() {
    let source = "";
    let mut sink = DiagSink::new();

    let tokens = tokenize(FileId(0), source, &mut sink);
    let _module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(sink.err_count(), 0);
}

#[test]
fn simple_binding_compiles() {
    let source = "x :: () i32 => 42";
    let mut sink = DiagSink::new();

    let tokens = tokenize(FileId(0), source, &mut sink);
    let _module = parse(FileId(0), tokens, &mut sink);

    assert_eq!(sink.err_count(), 0);
}

#[test]
fn unclosed_string_produces_expected_diagnostic() {
    // 未闭合字符串会吞掉本行其后的全部内容，因此无法用同行 --~ 注解标注，
    // 这里直接断言诊断码。
    let source = "x :: () str => \"unclosed";
    let mut sink = DiagSink::new();

    let tokens = tokenize(FileId(0), source, &mut sink);
    let _module = parse(FileId(0), tokens, &mut sink);

    let diags = sink.finish();
    assert!(
        diags.iter().any(|d| d.code == 4002),
        "应产生 E4002 未闭合的字符串字面量，实际：{:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
}

#[test]
fn annotated_diagnostic_matches_via_verifier() {
    // 字符串已闭合，注解成为独立 comment token，可被校验器识别。
    let source = r#"x :: () str => "\q"  --~ E2002"#;
    let mut sink = DiagSink::new();

    let tokens = tokenize(FileId(0), source, &mut sink);
    let _module = parse(FileId(0), tokens.clone(), &mut sink);

    let diags = sink.finish();
    let result = verify_test_annotations(source, &tokens, &diags);

    assert!(result.is_pass(), "注解应匹配诊断，实际：{:?}", result);
}
