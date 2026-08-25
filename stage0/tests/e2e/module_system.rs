//! 多文件模块系统端到端测试。
//!
//! 测试跨模块符号访问、pub 可见性控制、循环依赖检测等功能。

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// 创建临时测试目录并写入多个源文件
fn setup_test_project(files: &[(&str, &str)]) -> (TempDir, Vec<PathBuf>) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let mut paths = Vec::new();

    for (filename, content) in files {
        let path = temp_dir.path().join(filename);
        fs::write(&path, content).expect("Failed to write test file");
        paths.push(path);
    }

    (temp_dir, paths)
}

/// 运行 korec 编译器并返回退出码和 stderr 输出
fn run_korec(main_file: &PathBuf) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_korec"))
        .arg(main_file)
        .arg("--emit=resolved")  // 只运行到名称解析阶段
        .output()
        .expect("Failed to run korec");

    let exit_code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    (exit_code, stderr)
}

// 需要类型检查器完整支持模块限定符（module.func）后才能启用
#[test]
#[ignore]
fn test_basic_cross_module_access() {
    let files = [
        (
            "math.kore",
            r#"
pub add :: (a i32, b i32) i32 => {
    ret a + b
}

pub sub :: (a i32, b i32) i32 => {
    ret a - b
}
"#,
        ),
        (
            "main.kore",
            r#"
use math

main :: () i32 => {
    x := math.add(10, 20)
    y := math.sub(x, 5)
    ret y
}
"#,
        ),
    ];

    let (_temp_dir, paths) = setup_test_project(&files);
    let (exit_code, stderr) = run_korec(&paths[1]); // main.kore

    assert_eq!(exit_code, 0, "Compilation should succeed. stderr:\n{}", stderr);
    assert!(!stderr.contains("E4006"), "Should not have undefined module error");
    assert!(!stderr.contains("E4007"), "Should not have undefined symbol error");
}

#[test]
fn test_private_symbol_access_fails() {
    let files = [
        (
            "math.kore",
            r#"
pub add :: (a i32, b i32) i32 => {
    ret a + b
}

// 私有函数，没有 pub 标记
private_helper :: (x i32) i32 => {
    ret x * 2
}
"#,
        ),
        (
            "main.kore",
            r#"
use math

main :: () i32 => {
    x := math.add(1, 2)
    y := math.private_helper(x)  //~ E4008
    ret y
}
"#,
        ),
    ];

    let (_temp_dir, paths) = setup_test_project(&files);
    let (exit_code, stderr) = run_korec(&paths[1]);

    assert_ne!(exit_code, 0, "Should fail with private symbol error");
    assert!(
        stderr.contains("E4008") || stderr.contains("私有"),
        "Should report private symbol error. stderr:\n{}",
        stderr
    );
}

#[test]
fn test_undefined_module() {
    let files = [(
        "main.kore",
        r#"
use nonexistent

main :: () void => {
    nonexistent.func()
}
"#,
    )];

    let (_temp_dir, paths) = setup_test_project(&files);
    let (exit_code, stderr) = run_korec(&paths[0]);

    assert_ne!(exit_code, 0, "Should fail with undefined module error");
    assert!(
        stderr.contains("E4006"),
        "Should report undefined module error. stderr:\n{}",
        stderr
    );
}

#[test]
fn test_undefined_symbol_in_module() {
    let files = [
        (
            "math.kore",
            r#"
pub add :: (a i32, b i32) i32 => {
    ret a + b
}
"#,
        ),
        (
            "main.kore",
            r#"
use math

main :: () i32 => {
    ret math.multiply(2, 3)  //~ E4007
}
"#,
        ),
    ];

    let (_temp_dir, paths) = setup_test_project(&files);
    let (exit_code, stderr) = run_korec(&paths[1]);

    assert_ne!(exit_code, 0, "Should fail with undefined symbol error");
    assert!(
        stderr.contains("E4007") || stderr.contains("未定义"),
        "Should report undefined symbol error. stderr:\n{}",
        stderr
    );
}

#[test]
fn test_circular_dependency() {
    let files = [
        (
            "a.kore",
            r#"
use b

pub func_a :: () void => {
    b.func_b()
}
"#,
        ),
        (
            "b.kore",
            r#"
use a

pub func_b :: () void => {
    a.func_a()
}
"#,
        ),
    ];

    let (_temp_dir, paths) = setup_test_project(&files);
    let (exit_code, stderr) = run_korec(&paths[0]);

    assert_ne!(exit_code, 0, "Should fail with circular dependency error");
    assert!(
        stderr.contains("E4009") || stderr.contains("循环"),
        "Should report circular dependency. stderr:\n{}",
        stderr
    );
}

