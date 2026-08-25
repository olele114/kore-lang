//! print/println 端到端测试

use std::process::Command;
use std::fs;

fn compile_and_run(source: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let temp_dir = std::env::temp_dir();
    let source_path = temp_dir.join(format!("test_io_{}.kore", timestamp));
    let ll_path = temp_dir.join(format!("test_io_{}.ll", timestamp));
    let exe_path = temp_dir.join(format!("test_io_{}", timestamp));

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

    // 使用 clang 编译 LLVM IR，链接运行时库
    let runtime_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime");
    let cmdline_c = runtime_dir.join("cmdline.c");

    let link = Command::new("clang")
        .arg(&ll_path)
        .arg(&cmdline_c)
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
fn test_println_single() {
    let source = r#"
main :: () void => {
    println("Hello, World!")
}
"#;
    let output = compile_and_run(source);
    assert_eq!(output.trim(), "Hello, World!");
}

#[test]
fn test_println_multiple() {
    let source = r#"
main :: () void => {
    println("Line 1")
    println("Line 2")
    println("Line 3")
}
"#;
    let output = compile_and_run(source);
    assert_eq!(output.trim(), "Line 1\nLine 2\nLine 3");
}

#[test]
fn test_print_no_newline() {
    let source = r#"
main :: () void => {
    print("Hello, ")
    print("World")
    println("!")
}
"#;
    let output = compile_and_run(source);
    assert_eq!(output.trim(), "Hello, World!");
}

#[test]
fn test_print_and_println_mix() {
    let source = r#"
main :: () void => {
    print("A")
    print("B")
    println("C")
    print("D")
    println("E")
}
"#;
    let output = compile_and_run(source);
    assert_eq!(output.trim(), "ABC\nDE");
}
