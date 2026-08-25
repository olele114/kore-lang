//! 端到端测试：数组功能

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
fn test_array_literal() {
    let source = r#"
main :: () i32 => {
    arr := [1, 2, 3]
    ret arr[0]
}
"#;

    let exe_path = compile_to_executable(source, "test_array_lit");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 1);
}

#[test]
fn test_array_index_access() {
    let source = r#"
main :: () i32 => {
    arr := [10, 20, 30, 40]
    ret arr[2]
}
"#;

    let exe_path = compile_to_executable(source, "test_array_idx");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 30);
}

#[test]
fn test_array_computation() {
    let source = r#"
main :: () i32 => {
    arr := [5, 10, 15]
    a := arr[0] + arr[1]
    b := arr[2] * 2
    ret a + b
}
"#;

    let exe_path = compile_to_executable(source, "test_array_comp");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 45); // (5+10) + (15*2) = 15 + 30 = 45
}

#[test]
fn test_array_as_param() {
    let source = r#"
get_second :: (arr [3]i32) i32 => {
    ret arr[1]
}

main :: () i32 => {
    data := [7, 14, 21]
    ret get_second(data)
}
"#;

    let exe_path = compile_to_executable(source, "test_array_param");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 14);
}

#[test]
fn test_nested_array_access() {
    let source = r#"
main :: () i32 => {
    arr := [1, 2, 3, 4, 5]
    idx := 3
    ret arr[idx]
}
"#;

    let exe_path = compile_to_executable(source, "test_nested_arr");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 4);
}

#[test]
fn test_slice_from_array() {
    let source = r#"
main :: () i32 => {
    arr := [1, 2, 3, 4, 5]
    s : []i32 = arr
    ret s[2]
}
"#;

    let exe_path = compile_to_executable(source, "test_slice_array");
    let exit_code = run_executable(&exe_path);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 3);
}
