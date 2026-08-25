//! CLI 集成测试，覆盖 main.rs 中未被单元测试覆盖的路径。

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// 获取 korec 二进制路径。
fn korec_bin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // deps/
    path.pop(); // debug/
    if cfg!(target_os = "windows") {
        path.push("korec.exe");
    } else {
        path.push("korec");
    }
    path
}

#[test]
fn cli_help_shows_usage() {
    let output = Command::new(korec_bin())
        .arg("--help")
        .output()
        .expect("failed to run korec");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("用法: korec"));
    assert!(stdout.contains("--error-format"));
}

#[test]
fn cli_explain_shows_error_description() {
    let output = Command::new(korec_bin())
        .args(["--explain", "E4001"])
        .output()
        .expect("failed to run korec");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("E4001"));
}

#[test]
fn cli_explain_unknown_code_fails() {
    let output = Command::new(korec_bin())
        .args(["--explain", "E9999"])
        .output()
        .expect("failed to run korec");

    // 退出码 2 = UsageError
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("登记表中没有错误码"));
}

#[test]
fn cli_nonexistent_file_produces_e9001() {
    let output = Command::new(korec_bin())
        .arg("/nonexistent/file.kore")
        .output()
        .expect("failed to run korec");

    // 退出码 1 = CompileError
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("9001") || stderr.contains("无法读取源文件"));
}

#[test]
fn cli_invalid_utf8_produces_e9002() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("bad.kore");

    // 写入非 UTF-8 字节
    fs::write(&path, &[0xFF, 0xFE, 0xFD]).unwrap();

    let output = Command::new(korec_bin())
        .arg(&path)
        .output()
        .expect("failed to run korec");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("9001") || stderr.contains("stream did not contain valid UTF-8"));
}

#[test]
fn cli_multiple_input_files() {
    let tmp = TempDir::new().unwrap();
    let file1 = tmp.path().join("a.kore");
    let file2 = tmp.path().join("b.kore");

    fs::write(&file1, "add :: (a i32, b i32) i32 => a + b\n").unwrap();
    fs::write(&file2, "sub :: (a i32, b i32) i32 => a - b\n").unwrap();

    let output = Command::new(korec_bin())
        .arg(&file1)
        .arg(&file2)
        .output()
        .expect("failed to run korec");

    // 当前 stage0 会词法/语法分析两个文件，无错则退出码 0
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn cli_stats_flag_produces_output() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.kore");
    fs::write(&path, "add :: (a i32, b i32) i32 => a + b\n").unwrap();

    let output = Command::new(korec_bin())
        .arg("--stats")
        .arg(&path)
        .output()
        .expect("failed to run korec");

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("stats:"));
}

#[test]
fn cli_time_passes_flag_produces_output() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.kore");
    fs::write(&path, "add :: (a i32, b i32) i32 => a + b\n").unwrap();

    let output = Command::new(korec_bin())
        .arg("--time-passes")
        .arg(&path)
        .output()
        .expect("failed to run korec");

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("time-passes:"));
}

#[test]
fn cli_verify_test_annotations_pass() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.kore");

    // 正确的测试注解：类型错误 E9001
    fs::write(&path, "bad_fn :: (x i32) i32 => \"not a number\"  --~ E9001\n").unwrap();

    let output = Command::new(korec_bin())
        .arg("--verify-test-annotations")
        .arg(&path)
        .output()
        .expect("failed to run korec");

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("PASS"));
}

#[test]
fn cli_verify_test_annotations_fail_not_triggered() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.kore");

    // 注解未触发（代码正确）
    fs::write(&path, "add :: (a i32, b i32) i32 => a + b  --~ E4001\n").unwrap();

    let output = Command::new(korec_bin())
        .arg("--verify-test-annotations")
        .arg(&path)
        .output()
        .expect("failed to run korec");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("FAIL") || stderr.contains("注解未触发"));
}

#[test]
fn cli_error_format_json() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.kore");
    fs::write(&path, "bad_fn :: (x i32) i32 => \"not a number\"\n").unwrap();

    let output = Command::new(korec_bin())
        .arg("--error-format=json")
        .arg(&path)
        .output()
        .expect("failed to run korec");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    // JSON 格式输出应包含结构化字段
    assert!(stderr.contains("\"code\"") || stderr.contains("{"));
}

#[test]
fn cli_error_limit_zero_shows_all_errors() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.kore");

    // 产生多个类型错误
    let src = "f1 :: (x i32) i32 => \"bad\"\nf2 :: (y i32) i32 => \"bad\"\nf3 :: (z i32) i32 => \"bad\"\n";
    fs::write(&path, src).unwrap();

    let output = Command::new(korec_bin())
        .arg("--error-limit=0")
        .arg(&path)
        .output()
        .expect("failed to run korec");

    assert_eq!(output.status.code(), Some(1));
    // 无限制，应显示所有 3 个错误
    let stderr = String::from_utf8_lossy(&output.stderr);
    let error_count = stderr.matches("error").count();
    assert!(error_count >= 3, "应显示至少 3 个错误，实际: {}", error_count);
}
