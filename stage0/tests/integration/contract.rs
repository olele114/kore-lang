//! 契约断言集成测试。
//!
//! 验证契约断言提取和验证在完整编译流程中的行为。

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::driver::{extract_contract_assertions, verify_contract_assertions};
use kore_stage0::frontend::lexer::tokenize;

#[test]
fn extract_and_verify_valid_assertions() {
    let source = r#"
f :: () void => g()  --= tailcall g
x := load(ptr)  --= volatile-load u32
store(ptr, val)  --= volatile-store u64
"#;

    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), source, &mut sink);

    // 提取契约断言
    let assertions = extract_contract_assertions(source, &tokens)
        .expect("Should extract assertions without errors");

    assert_eq!(assertions.len(), 3);

    // 验证断言种类
    let unrecognized = verify_contract_assertions(&assertions, &mut sink);

    assert_eq!(unrecognized, 0, "All assertion kinds should be recognized");
    assert_eq!(sink.err_count(), 0, "No errors should be reported");
}

#[test]
fn unrecognized_assertion_kind_reports_error() {
    let source = "x := 42  --= custom-optimization";

    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), source, &mut sink);

    let assertions = extract_contract_assertions(source, &tokens)
        .expect("Should extract assertion structure");

    assert_eq!(assertions.len(), 1);

    // 验证应该报告无法识别的断言种类
    let unrecognized = verify_contract_assertions(&assertions, &mut sink);

    assert_eq!(unrecognized, 1);
    assert_eq!(sink.err_count(), 1);

    let diags = sink.finish();
    assert_eq!(diags[0].code, 9002);
}

#[test]
fn empty_assertion_rejected_during_extraction() {
    let source = "x := 42  --=";

    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), source, &mut sink);

    let result = extract_contract_assertions(source, &tokens);

    assert!(result.is_err(), "Empty assertion should produce error");

    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, 9001);
}

#[test]
fn mixed_valid_and_invalid_assertions() {
    let source = r#"
f :: () void => g()  --= tailcall g
x := 42  --= unknown-kind
y := load(ptr)  --= volatile-load u32
"#;

    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), source, &mut sink);

    let assertions = extract_contract_assertions(source, &tokens)
        .expect("Should extract all assertions");

    assert_eq!(assertions.len(), 3);

    let unrecognized = verify_contract_assertions(&assertions, &mut sink);

    assert_eq!(unrecognized, 1, "Only 'unknown-kind' should be unrecognized");
    assert_eq!(sink.err_count(), 1);
}
