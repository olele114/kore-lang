//! 测试 CLI 参数解析和各种选项组合。
//!
//! 通过实际调用 korec 二进制来验证参数解析的行为。

use std::process::Command;
use std::path::PathBuf;

fn korec_bin() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target");
    path.push("debug");
    path.push("korec");
    path
}

fn create_temp_file(content: &str) -> tempfile::NamedTempFile {
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

#[test]
fn help_flag_returns_zero() {
    let output = Command::new(korec_bin())
        .arg("--help")
        .output()
        .expect("failed to execute korec");

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("用法"));
}

#[test]
fn no_args_shows_help() {
    let output = Command::new(korec_bin())
        .output()
        .expect("failed to execute korec");

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("用法"));
}

#[test]
fn unknown_option_exits_with_usage_error() {
    let tmp = create_temp_file("x :: i32 = 42");
    let output = Command::new(korec_bin())
        .arg("--unknown-flag")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("未知选项"));
}

#[test]
fn missing_input_file_is_error() {
    let output = Command::new(korec_bin())
        .arg("--stats")
        .output()
        .expect("failed to execute korec");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("未指定源文件"));
}

#[test]
fn multiple_emit_without_output_dir_is_error() {
    let tmp = create_temp_file("x :: i32 = 42");
    let output = Command::new(korec_bin())
        .arg("--emit=ast")
        .arg("--emit=tokens")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("必须用 `-o`"));
}

#[test]
fn invalid_emit_stage_is_error() {
    let tmp = create_temp_file("x :: i32 = 42");
    let output = Command::new(korec_bin())
        .arg("--emit=hir")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("未知的 --emit 阶段"));
}

#[test]
fn error_format_json_is_accepted() {
    let tmp = create_temp_file("x :: i32 = 42 --~ E9999");
    let output = Command::new(korec_bin())
        .arg("--error-format=json")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    // 实际上当前阶段可能没有错误，但至少验证参数被接受了
    assert!(output.status.code() == Some(0) || output.status.code() == Some(1));
}

#[test]
fn error_format_short_is_accepted() {
    let tmp = create_temp_file("x :: i32 = 42");
    let output = Command::new(korec_bin())
        .arg("--error-format=short")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    assert!(output.status.code() == Some(0) || output.status.code() == Some(1));
}

#[test]
fn invalid_error_format_is_error() {
    let tmp = create_temp_file("x :: i32 = 42");
    let output = Command::new(korec_bin())
        .arg("--error-format=xml")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("未知的 --error-format"));
}

#[test]
fn error_limit_zero_is_accepted() {
    let tmp = create_temp_file("x :: i32 = 42");
    let output = Command::new(korec_bin())
        .arg("--error-limit=0")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    assert!(output.status.code() == Some(0) || output.status.code() == Some(1));
}

#[test]
fn error_limit_non_numeric_is_error() {
    let tmp = create_temp_file("x :: i32 = 42");
    let output = Command::new(korec_bin())
        .arg("--error-limit=abc")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("需要非负整数"));
}

#[test]
fn stats_flag_produces_stats() {
    let tmp = create_temp_file("x :: i32 = 42");
    let output = Command::new(korec_bin())
        .arg("--stats")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("stats: tokens"));
    assert!(stderr.contains("stats: errors"));
}

#[test]
fn time_passes_flag_produces_timing() {
    let tmp = create_temp_file("x :: i32 = 42");
    let output = Command::new(korec_bin())
        .arg("--time-passes")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("time-passes:"));
}

#[test]
fn explain_valid_code_returns_zero() {
    let output = Command::new(korec_bin())
        .arg("--explain")
        .arg("E4001")
        .output()
        .expect("failed to execute korec");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("E4001"));
}

#[test]
fn explain_without_code_is_error() {
    let output = Command::new(korec_bin())
        .arg("--explain")
        .output()
        .expect("failed to execute korec");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("缺少错误码"));
}

#[test]
fn explain_invalid_code_format_is_error() {
    let output = Command::new(korec_bin())
        .arg("--explain")
        .arg("Xabc")
        .output()
        .expect("failed to execute korec");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("无法识别的错误码"));
}

#[test]
fn explain_nonexistent_code_is_error() {
    let output = Command::new(korec_bin())
        .arg("--explain")
        .arg("E9")
        .output()
        .expect("failed to execute korec");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("没有错误码"));
}

#[test]
fn output_path_is_accepted() {
    let tmp = create_temp_file("x :: i32 = 42");
    let out_dir = tempfile::tempdir().unwrap();
    let output = Command::new(korec_bin())
        .arg("-o")
        .arg(out_dir.path())
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    // 参数被接受，即使当前没有产物输出
    assert!(output.status.code() == Some(0) || output.status.code() == Some(1));
}

#[test]
fn output_flag_without_path_is_error() {
    let tmp = create_temp_file("x :: i32 = 42");
    let output = Command::new(korec_bin())
        .arg("-o")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    // -o 后面跟的是文件路径而非输出路径，会导致缺少源文件
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn emit_spans_flag_is_accepted() {
    let tmp = create_temp_file("x :: i32 = 42");
    let output = Command::new(korec_bin())
        .arg("--emit-spans")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    assert!(output.status.code() == Some(0) || output.status.code() == Some(1));
}

#[test]
fn file_not_found_produces_diagnostic() {
    let output = Command::new(korec_bin())
        .arg("nonexistent.kore")
        .output()
        .expect("failed to execute korec");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("无法读取源文件") || stderr.contains("E9001"));
}

#[test]
fn multiple_input_files_are_accepted() {
    let tmp1 = create_temp_file("x :: i32 = 1");
    let tmp2 = create_temp_file("y :: i32 = 2");
    let output = Command::new(korec_bin())
        .arg(tmp1.path())
        .arg(tmp2.path())
        .output()
        .expect("failed to execute korec");

    assert!(output.status.code() == Some(0) || output.status.code() == Some(1));
}
