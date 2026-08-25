//! 模块系统端到端测试。

use kore_stage0::{
    diag::{DiagSink, FileId, Severity},
    driver::pipeline::run_frontend,
};
use std::fs;
use std::path::PathBuf;

/// 测试单文件编译（不使用模块系统）。
#[test]
fn test_single_file_compilation() {
    let mut sink = DiagSink::new();

    let main_path = PathBuf::from("tests/fixtures/modules/main.kore");
    let source = fs::read_to_string(&main_path)
        .expect("无法读取测试文件");

    let output = run_frontend(FileId(0), &source, &mut sink);

    // 检查是否有错误
    let diags = sink.peek();
    let errors: Vec<_> = diags.iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !errors.is_empty() {
        eprintln!("编译错误:");
        for diag in errors {
            eprintln!("  {}", diag.msg);
        }
        panic!("编译失败");
    }

    assert!(output.module.is_some(), "单文件编译应该生成 AST");
    assert!(output.symbols.is_some(), "单文件编译应该生成符号表");
}

/// 测试未定义模块错误。
#[test]
#[ignore] // TODO: 当前 resolve 阶段尚未实现 use 语句的模块查找，暂时跳过此测试
fn test_undefined_module_error() {
    let mut sink = DiagSink::new();

    // 创建临时测试文件
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("test_undefined_module.kore");

    let source = "use nonexistent\n\nmain :: () i32 => {\n    ret 0\n}\n";
    fs::write(&test_file, source)
        .expect("写入测试文件失败");

    let _output = run_frontend(FileId(0), source, &mut sink);

    // 应该有错误
    let diags = sink.peek();
    let errors: Vec<_> = diags.iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(!errors.is_empty(), "应该有未定义模块错误");

    // 检查错误消息包含 "未定义的模块"
    let has_undefined_module_error = errors.iter()
        .any(|d| d.msg.contains("未定义的模块") || d.msg.contains("文件不存在"));

    assert!(has_undefined_module_error, "应该包含未定义模块错误消息，实际错误：{:?}",
        errors.iter().map(|d| &d.msg).collect::<Vec<_>>());

    // 清理
    let _ = fs::remove_file(test_file);
}
