//! 不逃逸检查的端到端测试：源码 → 词法 → 语法 → 不逃逸检查。
//!
//! 覆盖 docs/spec/05-memory.md §2 的两条规则在真实源码上的表现：
//! - 移动后不可用（E5001）
//! - 借用指针不逃逸（E5002 / E5003）
//!
//! 与 escape::checker 的单元测试不同，这里的 AST 由真实 parser 产出，
//! 因此能捕获「检查器假设的 AST 形状与 parser 实际产出不一致」这类缺陷。

use kore_stage0::diag::{DiagSink, ErrorCode, FileId};
use kore_stage0::frontend::escape::EscapeChecker;
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;

/// 跑完整前端管线，返回不逃逸检查产出的错误码列表。
fn escape_codes(source: &str) -> Vec<u16> {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), source, &mut sink);
    let module = parse(FileId(0), tokens, &mut sink);

    // 前置 pass 必须干净，否则测的就不是不逃逸检查了。
    let front_diags = sink.finish();
    assert!(
        front_diags.is_empty(),
        "词法/语法阶段不应产生诊断，实际：{:?}",
        front_diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );

    let mut sink = DiagSink::new();
    EscapeChecker::new(&mut sink).check_module(&module);
    sink.finish().iter().map(|d| d.code).collect()
}

#[test]
fn clean_function_reports_nothing() {
    let codes = escape_codes("f :: (x own ^T) void => { y := x }");
    assert!(codes.is_empty(), "单次移动不应报错，实际：{codes:?}");
}

#[test]
fn use_after_move_reports_e5001() {
    // x 被移动给 y 之后再移动给 z → 移动后使用。
    let codes = escape_codes(
        "f :: (x own ^T) void => {
  y := x
  z := x
}",
    );
    assert!(
        codes.contains(&ErrorCode::UseAfterMove.as_u16()),
        "应报 E5001 移动后使用，实际：{codes:?}"
    );
}

#[test]
fn borrow_param_can_be_returned() {
    // 参数借用与调用者同寿，返回它是合法的。
    let codes = escape_codes("f :: (x ^T) ^T => { ret x }");
    assert!(codes.is_empty(), "参数借用可返回，实际：{codes:?}");
}

#[test]
fn local_borrow_return_reports_e5003() {
    // 函数内部的借用绑定活不过函数，返回它是逃逸。
    let codes = escape_codes(
        "f :: () ^T => {
  local : ^T = 0
  ret local
}",
    );
    assert!(
        codes.contains(&ErrorCode::BorrowEscapesToReturn.as_u16()),
        "应报 E5003 借用逃逸到返回值，实际：{codes:?}"
    );
}

#[test]
fn local_borrow_into_field_reports_e5002() {
    // 把局部借用写入字段：字段可能比借用来源长寿。
    let codes = escape_codes(
        "f :: (obj ^S) void => {
  b : ^T = 0
  obj.field = b
}",
    );
    assert!(
        codes.contains(&ErrorCode::BorrowEscapesToHeap.as_u16()),
        "应报 E5002 借用逃逸到堆，实际：{codes:?}"
    );
}

#[test]
fn plain_binding_is_not_checked() {
    // 无 own/^ 标注的绑定不受移动规则约束，重复使用合法。
    let codes = escape_codes(
        "f :: (x i32) void => {
  y := x
  z := x
}",
    );
    assert!(codes.is_empty(), "普通绑定不应报错，实际：{codes:?}");
}
