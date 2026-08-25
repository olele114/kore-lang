//! 端到端测试：完整编译流程（源码 → 可执行文件）

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
fn test_simple_return() {
    let source = r#"
main :: () i32 => {
    ret 42
}
"#;

    let exe_path = compile_to_executable(source, "test_simple_ret");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 42);
}

#[test]
fn test_function_call() {
    let source = r#"
add :: (a i32, b i32) i32 => {
    ret a + b
}

main :: () i32 => {
    result := add(10, 32)
    ret result
}
"#;

    let exe_path = compile_to_executable(source, "test_func_call");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 42);
}

#[test]
fn test_nested_calls() {
    let source = r#"
add :: (a i32, b i32) i32 => {
    ret a + b
}

mul :: (x i32, y i32) i32 => {
    ret x * y
}

compute :: (m i32, n i32) i32 => {
    sum := add(m, n)
    product := mul(sum, 2)
    ret product
}

main :: () i32 => {
    result := compute(5, 4)
    ret result
}
"#;

    let exe_path = compile_to_executable(source, "test_nested");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 18); // (5 + 4) * 2 = 18
}

#[test]
fn test_arithmetic_ops() {
    let source = r#"
main :: () i32 => {
    a := 10 + 5
    b := a - 3
    c := b * 2
    d := c / 4
    ret d
}
"#;

    let exe_path = compile_to_executable(source, "test_arith");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 6); // ((10+5)-3)*2/4 = 12*2/4 = 6
}

#[test]
fn test_zero_return() {
    let source = r#"
main :: () i32 => {
    ret 0
}
"#;

    let exe_path = compile_to_executable(source, "test_zero");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 0);
}

#[test]
fn test_multiple_params() {
    let source = r#"
sum3 :: (a i32, b i32, c i32) i32 => {
    ret a + b + c
}

main :: () i32 => {
    ret sum3(10, 20, 7)
}
"#;

    let exe_path = compile_to_executable(source, "test_multi_params");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 37);
}
