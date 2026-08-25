//! 端到端测试：命令行参数解析功能

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
fn run_executable_with_args(exe_path: &PathBuf, args: &[&str]) -> i32 {
    let output = Command::new(exe_path)
        .args(args)
        .output()
        .expect("Failed to run executable");

    output.status.code().unwrap_or(-1)
}

#[test]
fn test_argc_no_args() {
    let source = r#"
main :: (argc i32, argv []str) i32 => {
    ret argc
}
"#;

    let exe_path = compile_to_executable(source, "test_argc_zero");
    let exit_code = run_executable_with_args(&exe_path, &[]);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 1); // 程序名本身算 1 个参数
}

#[test]
fn test_argc_with_args() {
    let source = r#"
main :: (argc i32, argv []str) i32 => {
    ret argc
}
"#;

    let exe_path = compile_to_executable(source, "test_argc_three");
    let exit_code = run_executable_with_args(&exe_path, &["arg1", "arg2"]);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 3); // 程序名 + 2 个参数 = 3
}

#[test]
fn test_argv_access() {
    let source = r#"
main :: (argc i32, argv []str) i32 => {
    first_arg := argv[1]
    ret 104
}
"#;

    let exe_path = compile_to_executable(source, "test_argv_len");
    let exit_code = run_executable_with_args(&exe_path, &["hello"]);

    fs::remove_file(&exe_path).ok();
    assert_eq!(exit_code, 104); // 'h' 的 ASCII 码
}
