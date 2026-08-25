//! 端到端测试：联合类型功能

use std::process::Command;
use std::path::PathBuf;
use std::fs;

/// 辅助函数：编译 Kore 源码为可执行文件
fn compile_to_executable(source: &str, exe_name: &str) -> PathBuf {
    let tmp_dir = PathBuf::from("/data/data/com.termux/files/tmp");
    let source_path = tmp_dir.join(format!("{}.kore", exe_name));
    let exe_path = tmp_dir.join(exe_name);

    // 写入源文件
    fs::write(&source_path, source).expect("Failed to write source file");

    // 编译
    let output = Command::new(env!("CARGO_BIN_EXE_korec"))
        .arg("-o")
        .arg(&exe_path)
        .arg(&source_path)
        .output()
        .expect("Failed to run compiler");

    if !output.status.success() {
        panic!(
            "Compilation failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // 清理源文件
    let _ = fs::remove_file(&source_path);

    exe_path
}

/// 辅助函数：运行可执行文件并返回退出码
fn run_executable(exe_path: &PathBuf) -> i32 {
    let output = Command::new(exe_path)
        .output()
        .expect("Failed to run executable");

    output.status.code().unwrap_or(-1)
}

#[test]
fn test_simple_variant_construction() {
    let source = r#"
Result :: .Ok(i32) | .Err(i32)

main :: () i32 => {
    x := .Ok(42)
    ret 0
}
"#;

    let exe_path = compile_to_executable(source, "test_simple_variant");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 0);
}

#[test]
fn test_variant_with_payload() {
    let source = r#"
Result :: .Ok(i32) | .Err(i32)

main :: () i32 => {
    result := .Ok(100)
    ret 0
}
"#;

    let exe_path = compile_to_executable(source, "test_variant_payload");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 0);
}

#[test]
fn test_variant_match() {
    let source = r#"
Option :: .Some(i32) | .None

main :: () i32 => {
    opt := .Some(42)
    ret ? opt is {
        .Some(v) => v,
        .None => 0
    }
}
"#;

    let exe_path = compile_to_executable(source, "test_variant_match");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 42);
}

#[test]
fn test_variant_match_none() {
    let source = r#"
Option :: .Some(i32) | .None

main :: () i32 => {
    opt := .None
    ret ? opt is {
        .Some(v) => v,
        .None => 99
    }
}
"#;

    let exe_path = compile_to_executable(source, "test_variant_none");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 99);
}

#[test]
fn test_nested_union() {
    let source = r#"
Result :: .Ok(i32) | .Err(i32)
Nested :: .Inner(Result) | .Outer(i32)

main :: () i32 => {
    inner_result := .Ok(99)
    nested := .Inner(inner_result)
    ret 0
}
"#;

    let exe_path = compile_to_executable(source, "test_nested_union");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 0);
}
