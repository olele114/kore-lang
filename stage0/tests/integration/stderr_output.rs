//! 标准错误流输出测试

use std::process::Command;
use std::fs;

#[test]
fn stderr_separation() {
    // 创建测试源文件
    let source = r#"
main :: () void => {
    print("to stdout")
    eprint("to stderr")
    println("stdout newline")
    eprintln("stderr newline")
}
"#;

    let temp_dir = std::env::temp_dir();
    let source_path = temp_dir.join("test_stderr.kore");
    let output_path = temp_dir.join("test_stderr");

    fs::write(&source_path, source).expect("写入源文件失败");

    // 编译
    let compile = Command::new(env!("CARGO_BIN_EXE_korec"))
        .arg(&source_path)
        .arg("-o")
        .arg(&output_path)
        .output()
        .expect("编译失败");

    assert!(compile.status.success(), "编译应该成功");

    // 运行程序，分别捕获 stdout 和 stderr
    let run = Command::new(&output_path)
        .output()
        .expect("运行失败");

    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);

    // 验证 stdout
    assert!(stdout.contains("to stdout"), "stdout 应包含 print 输出");
    assert!(stdout.contains("stdout newline"), "stdout 应包含 println 输出");
    assert!(!stdout.contains("to stderr"), "stdout 不应包含 eprint 输出");

    // 验证 stderr
    assert!(stderr.contains("to stderr"), "stderr 应包含 eprint 输出");
    assert!(stderr.contains("stderr newline"), "stderr 应包含 eprintln 输出");
    assert!(!stderr.contains("to stdout"), "stderr 不应包含 print 输出");

    // 清理
    let _ = fs::remove_file(&source_path);
    let _ = fs::remove_file(&output_path);
}
