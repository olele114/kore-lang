//! 端到端测试：错误处理（T ! E）

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
fn test_error_union_basic() {
    let source = r#"
divide :: (a i32, b i32) i32 ! str => {
    ? b == 0 => ret .Err("division by zero")
    ret .Ok(a / b)
}

main :: () i32 => {
    result := divide(10, 2)
    ret ? result is {
        .Ok(v) => v,
        .Err(_) => -1
    }
}
"#;

    let exe = compile_to_executable(source, "error_union_basic");
    let exit_code = run_executable(&exe);
    let _ = fs::remove_file(&exe);

    assert_eq!(exit_code, 5, "Expected 10/2 = 5");
}

#[test]
fn test_error_union_with_error() {
    let source = r#"
divide :: (a i32, b i32) i32 ! str => {
    ? b == 0 => ret .Err("division by zero")
    ret .Ok(a / b)
}

main :: () i32 => {
    result := divide(10, 0)
    ret ? result is {
        .Ok(_) => -1,
        .Err(_) => 42
    }
}
"#;

    let exe = compile_to_executable(source, "error_union_with_error");
    let exit_code = run_executable(&exe);
    let _ = fs::remove_file(&exe);

    assert_eq!(exit_code, 42, "Expected error case to return 42");
}

#[test]
fn test_error_propagation() {
    let source = r#"
divide :: (a i32, b i32) i32 ! str => {
    ? b == 0 => ret .Err("division by zero")
    ret .Ok(a / b)
}

safe_divide :: (x i32, y i32, z i32) i32 ! str => {
    result1 := divide(x, y)!
    result2 := divide(result1, z)!
    ret .Ok(result2)
}

main :: () i32 => {
    result := safe_divide(20, 2, 5)
    ret ? result is {
        .Ok(v) => v,
        .Err(_) => -1
    }
}
"#;

    let exe = compile_to_executable(source, "error_propagation");
    let exit_code = run_executable(&exe);
    let _ = fs::remove_file(&exe);

    assert_eq!(exit_code, 2, "Expected (20/2)/5 = 2");
}

#[test]
fn test_error_propagation_with_error() {
    let source = r#"
divide :: (a i32, b i32) i32 ! str => {
    ? b == 0 => ret .Err("division by zero")
    ret .Ok(a / b)
}

safe_divide :: (x i32, y i32, z i32) i32 ! str => {
    result1 := divide(x, y)!
    result2 := divide(result1, z)!
    ret .Ok(result2)
}

main :: () i32 => {
    result := safe_divide(20, 0, 5)
    ret ? result is {
        .Ok(_) => -1,
        .Err(_) => 99
    }
}
"#;

    let exe = compile_to_executable(source, "error_propagation_with_error");
    let exit_code = run_executable(&exe);
    let _ = fs::remove_file(&exe);

    assert_eq!(exit_code, 99, "Expected error to propagate and return 99");
}

#[test]
fn test_error_union_chaining() {
    let source = r#"
divide :: (a i32, b i32) i32 ! str => {
    ? b == 0 => ret .Err("division by zero")
    ret .Ok(a / b)
}

complex :: () i32 ! str => {
    a := divide(100, 10)!
    b := divide(a, 2)!
    c := divide(b, 5)!
    ret .Ok(c)
}

main :: () i32 => {
    result := complex()
    ret ? result is {
        .Ok(v) => v,
        .Err(_) => 0
    }
}
"#;

    let exe = compile_to_executable(source, "error_union_chaining");
    let exit_code = run_executable(&exe);
    let _ = fs::remove_file(&exe);

    assert_eq!(exit_code, 1, "Expected ((100/10)/2)/5 = 1");
}
