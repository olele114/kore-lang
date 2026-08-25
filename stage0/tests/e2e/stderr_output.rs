//! 测试标准错误流输出功能

use std::process::Command;
use std::fs;

fn compile_and_run_split(source: &str) -> (String, String, std::process::ExitStatus) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let temp_dir = std::env::temp_dir();
    let source_path = temp_dir.join(format!("test_stderr_{}.kore", timestamp));
    let exe_path = temp_dir.join(format!("test_stderr_{}", timestamp));

    // 写入源码
    fs::write(&source_path, source).expect("Failed to write source");

    // 编译
    let compile = Command::new(env!("CARGO_BIN_EXE_korec"))
        .arg(&source_path)
        .arg("-o")
        .arg(&exe_path)
        .output()
        .expect("Failed to compile");

    if !compile.status.success() {
        panic!("Compilation failed:\n{}", String::from_utf8_lossy(&compile.stderr));
    }

    // 运行可执行文件，分别捕获 stdout 和 stderr
    let run = Command::new(&exe_path)
        .output()
        .expect("Failed to run");

    let stdout = String::from_utf8_lossy(&run.stdout).to_string();
    let stderr = String::from_utf8_lossy(&run.stderr).to_string();

    // 清理临时文件
    let _ = fs::remove_file(&source_path);
    let _ = fs::remove_file(&exe_path);

    (stdout, stderr, run.status)
}

#[test]
fn test_eprint_basic() {
    let source = r#"
main :: () i32 => {
    eprint("Error message")
    ret 0
}
"#;

    let (stdout, stderr, status) = compile_and_run_split(source);
    assert!(status.success());
    assert_eq!(stdout, "");
    assert_eq!(stderr, "Error message");
}

#[test]
fn test_eprintln_basic() {
    let source = r#"
main :: () i32 => {
    eprintln("Error with newline")
    ret 0
}
"#;

    let (stdout, stderr, status) = compile_and_run_split(source);
    assert!(status.success());
    assert_eq!(stdout, "");
    assert_eq!(stderr, "Error with newline\n");
}

#[test]
fn test_stderr_stdout_separation() {
    let source = r#"
main :: () i32 => {
    print("stdout1")
    eprint("stderr1")
    println("stdout2")
    eprintln("stderr2")
    ret 0
}
"#;

    let (stdout, stderr, status) = compile_and_run_split(source);
    assert!(status.success());
    assert_eq!(stdout, "stdout1stdout2\n");
    assert_eq!(stderr, "stderr1stderr2\n");
}

#[test]
fn test_multiple_eprint() {
    let source = r#"
main :: () i32 => {
    eprint("Error: ")
    eprint("file not found")
    eprintln("")
    ret 0
}
"#;

    let (stdout, stderr, status) = compile_and_run_split(source);
    assert!(status.success());
    assert_eq!(stdout, "");
    assert_eq!(stderr, "Error: file not found\n");
}
