//! 文件 I/O 端到端测试

use std::process::Command;
use std::fs;

fn compile_and_run(source: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let temp_dir = std::env::temp_dir();
    let source_path = temp_dir.join(format!("test_io_{}.kore", timestamp));
    let ll_path = temp_dir.join(format!("test_io_{}.ll", timestamp));
    let exe_path = temp_dir.join(format!("test_io_{}", timestamp));
    let runtime_c = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("runtime/kore_runtime.c");

    // 写入源码
    fs::write(&source_path, source).expect("Failed to write source");

    // 编译到 LLVM IR
    let compile = Command::new(env!("CARGO_BIN_EXE_korec"))
        .arg(&source_path)
        .arg("--emit=llvm-ir")
        .arg("-o")
        .arg(&ll_path)
        .output()
        .expect("Failed to compile");

    if !compile.status.success() {
        panic!("Compilation failed:\n{}", String::from_utf8_lossy(&compile.stderr));
    }

    // 使用 clang 编译 LLVM IR 并链接运行时
    let link = Command::new("clang")
        .arg(&ll_path)
        .arg(&runtime_c)
        .arg("-o")
        .arg(&exe_path)
        .output()
        .expect("Failed to link");

    if !link.status.success() {
        panic!("Linking failed:\n{}", String::from_utf8_lossy(&link.stderr));
    }

    // 运行可执行文件
    let run = Command::new(&exe_path)
        .output()
        .expect("Failed to run");

    String::from_utf8_lossy(&run.stdout).to_string()
}

#[test]
fn test_write_and_read_file() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join(format!("kore_test_{}.txt", timestamp));
    let test_file_str = test_file.to_str().unwrap();

    let source = format!(r#"
main :: () void => {{
    write_file("{}", "Hello from Kore!")
    content := read_file("{}")
    println(content)
}}
"#, test_file_str, test_file_str);

    let output = compile_and_run(&source);
    assert_eq!(output.trim(), "Hello from Kore!");

    // 清理测试文件
    let _ = fs::remove_file(&test_file);
}

#[test]
fn test_write_multiline_content() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join(format!("kore_test_{}.txt", timestamp));
    let test_file_str = test_file.to_str().unwrap();

    let source = format!(r#"
main :: () void => {{
    write_file("{}", "Line 1\nLine 2\nLine 3")
    content := read_file("{}")
    print(content)
}}
"#, test_file_str, test_file_str);

    let output = compile_and_run(&source);
    assert_eq!(output.trim(), "Line 1\nLine 2\nLine 3");

    // 清理测试文件
    let _ = fs::remove_file(&test_file);
}
