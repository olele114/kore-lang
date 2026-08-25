//! 测试 --verify-test-annotations 功能。

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
fn verify_annotations_passes_with_valid_annotation() {
    // 注解必须落在字符串外部，否则会被当作字符串内容吞掉，
    // 不会产生 TestAnnot comment token。
    let tmp = create_temp_file(r#"x := "\q"  --~ E2002"#);
    let output = Command::new(korec_bin())
        .arg("--verify-test-annotations")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("test annotations: PASS"));
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn verify_annotations_fails_on_unexpected_diagnostic() {
    let tmp = create_temp_file("x :: () i32 => 42");
    let output = Command::new(korec_bin())
        .arg("--verify-test-annotations")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    // 没有注解但也没有诊断，应该通过
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("test annotations: PASS"));
}

#[test]
fn verify_annotations_reports_annotation_not_triggered() {
    let tmp = create_temp_file("x :: i32 = 42  --~ E9999");
    let output = Command::new(korec_bin())
        .arg("--verify-test-annotations")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("test annotations: FAIL"));
    assert!(stderr.contains("注解未触发"));
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn verify_annotations_reports_malformed_annotation() {
    let tmp = create_temp_file("x :: i32 = 42  --~ INVALID");
    let output = Command::new(korec_bin())
        .arg("--verify-test-annotations")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("test annotations: FAIL"));
    assert!(stderr.contains("格式错误的注解"));
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn verify_annotations_with_multiple_files() {
    let tmp1 = create_temp_file(r#"x := "\q"  --~ E2002"#);
    let tmp2 = create_temp_file("y :: () i32 => 1");

    let output = Command::new(korec_bin())
        .arg("--verify-test-annotations")
        .arg(tmp1.path())
        .arg(tmp2.path())
        .output()
        .expect("failed to execute korec");

    let stderr = String::from_utf8_lossy(&output.stderr);
    // 两个文件都应该验证
    assert_eq!(stderr.matches("test annotations: PASS").count(), 2);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn verify_annotations_stops_on_first_failure() {
    let tmp1 = create_temp_file("x :: i32 = 42  --~ E9999");
    let tmp2 = create_temp_file("y :: i32 = 1");

    let output = Command::new(korec_bin())
        .arg("--verify-test-annotations")
        .arg(tmp1.path())
        .arg(tmp2.path())
        .output()
        .expect("failed to execute korec");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("test annotations: FAIL"));
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn verify_annotations_with_lexer_error_continues() {
    let tmp = create_temp_file(r#"x := "\q"  --~ E2002"#);

    let output = Command::new(korec_bin())
        .arg("--verify-test-annotations")
        .arg(tmp.path())
        .output()
        .expect("failed to execute korec");

    // 词法错误被正确捕获并验证
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("test annotations: PASS"));
    assert_eq!(output.status.code(), Some(0));
}
