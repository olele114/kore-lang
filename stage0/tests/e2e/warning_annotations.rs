//! 测试注解系统对警告级别诊断的支持。
//!
//! 验证 `--~ W` 前缀能够正确匹配警告诊断。

use kore_stage0::diag::{Diagnostic, DiagLoc, DiagSink, FileId, Severity, Span};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::driver::verify_test_annotations;

#[test]
fn warning_annotation_matches_warning_diagnostic() {
    let source = "x :: () i32 => 42  --~ W9999";
    let mut sink = DiagSink::new();

    let tokens = tokenize(FileId(0), source, &mut sink);

    // 手动插入一个警告诊断
    sink.emit(Diagnostic::new(
        Severity::Warning,
        9999,
        "测试警告",
        DiagLoc::At(Span { file: FileId(0), lo: 19, hi: 24 }),
    ));

    let diags = sink.finish();

    let result = verify_test_annotations(source, &tokens, &diags);
    assert!(result.is_pass(), "警告注解应该匹配警告诊断");
}

#[test]
fn error_annotation_does_not_match_warning() {
    let source = "x :: () i32 => 42  --~ E9999";
    let mut sink = DiagSink::new();

    let tokens = tokenize(FileId(0), source, &mut sink);

    // 插入警告，但注解期望错误
    sink.emit(Diagnostic::new(
        Severity::Warning,
        9999,
        "测试警告",
        DiagLoc::At(Span { file: FileId(0), lo: 19, hi: 24 }),
    ));

    let diags = sink.finish();

    let result = verify_test_annotations(source, &tokens, &diags);
    assert!(!result.is_pass(), "错误注解不应该匹配警告诊断");
}

#[test]
fn note_annotation_matches_note_diagnostic() {
    let source = "x :: () i32 => 42  --~ I8888";
    let mut sink = DiagSink::new();

    let tokens = tokenize(FileId(0), source, &mut sink);

    sink.emit(Diagnostic::new(
        Severity::Note,
        8888,
        "测试提示",
        DiagLoc::At(Span { file: FileId(0), lo: 19, hi: 24 }),
    ));

    let diags = sink.finish();

    let result = verify_test_annotations(source, &tokens, &diags);
    assert!(result.is_pass(), "提示注解应该匹配提示诊断");
}
