//! 字符串类型前端支持测试

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::driver::run_frontend;

#[test]
fn string_literal_basic() {
    let source = r#"
main :: () str => "hello"
"#;
    let mut sink = DiagSink::new();
    let result = run_frontend(FileId(0), source, &mut sink);

    assert!(!sink.has_errors(), "前端应正确解析字符串字面量");
    assert!(result.module.is_some());
}

#[test]
fn string_with_escapes() {
    let source = r#"
msg :: () str => "hello\nworld\t!"
"#;
    let mut sink = DiagSink::new();
    let result = run_frontend(FileId(0), source, &mut sink);

    if sink.has_errors() {
        eprintln!("Diagnostics:");
        for d in sink.peek() {
            eprintln!("  {:?}", d);
        }
    }
    assert!(!sink.has_errors(), "应正确处理转义序列");
    assert!(result.module.is_some());
}

#[test]
fn string_type_annotation() {
    let source = r#"
greet :: (name str) str => name
"#;
    let mut sink = DiagSink::new();
    let result = run_frontend(FileId(0), source, &mut sink);

    assert!(!sink.has_errors(), "应正确解析 str 类型标注");
    assert!(result.module.is_some());
}

#[test]
fn string_var_declaration() {
    let source = r#"
main :: () void => {
    ~msg := "test"
}
"#;
    let mut sink = DiagSink::new();
    let result = run_frontend(FileId(0), source, &mut sink);

    if sink.has_errors() {
        eprintln!("Diagnostics:");
        for d in sink.peek() {
            eprintln!("  {:?}", d);
        }
    }
    assert!(!sink.has_errors(), "应正确处理字符串变量声明");
    assert!(result.module.is_some());
}

#[test]
fn raw_string_literal() {
    let source = r#"
path :: () str => `C:\Users\test\file.txt`
"#;
    let mut sink = DiagSink::new();
    let result = run_frontend(FileId(0), source, &mut sink);

    if sink.has_errors() {
        eprintln!("Diagnostics:");
        for d in sink.peek() {
            eprintln!("  {:?}", d);
        }
    }
    assert!(!sink.has_errors(), "应正确处理原始字符串字面量");
    assert!(result.module.is_some());
}
