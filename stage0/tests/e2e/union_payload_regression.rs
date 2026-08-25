//! 回归测试：联合类型 / 错误联合的降级缺陷
//!
//! 覆盖两个曾经的 lowering bug（`middleend/lower/control.rs`）：
//!
//! 1. **void 臂的 phi temp**：`? x is {...}` 的臂全部产出 void 时，降级仍会分配
//!    一个 `ty: Void` 的临时局部并向其写入。后端不为 void 局部分配栈空间，
//!    导致 codegen 报 "Symbol not found: local"。
//! 2. **错误联合 payload 绑定类型**：`T ! E` 不在 `union_defs` 中登记，
//!    `extract_pattern_bindings` 取不到 payload 类型就退化成 `i32`，
//!    把 `str` 的 16 字节胖指针按整数读出，打印成乱码。

use std::fs;
use std::process::Command;

/// 只编译，返回 (是否成功, stdout+stderr 合并输出)。用于断言应当被拒绝的源码。
fn compile_only(source: &str, tag: &str) -> (bool, String) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir();
    let stem = format!("union_regr_{}_{}", tag, timestamp);
    let source_path = temp_dir.join(format!("{}.kore", stem));
    let ll_path = temp_dir.join(format!("{}.ll", stem));

    fs::write(&source_path, source).expect("Failed to write source");

    let compile = Command::new(env!("CARGO_BIN_EXE_korec"))
        .arg(&source_path)
        .arg("--emit=llvm-ir")
        .arg("-o")
        .arg(&ll_path)
        .output()
        .expect("Failed to compile");

    let _ = fs::remove_file(&source_path);
    let _ = fs::remove_file(&ll_path);

    let mut combined = String::from_utf8_lossy(&compile.stdout).to_string();
    combined.push_str(&String::from_utf8_lossy(&compile.stderr));
    (compile.status.success(), combined)
}

/// 编译并运行源码，返回 (stdout, 退出码)。
fn compile_and_run(source: &str, tag: &str) -> (String, i32) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir();
    let stem = format!("union_regr_{}_{}", tag, timestamp);
    let source_path = temp_dir.join(format!("{}.kore", stem));
    let ll_path = temp_dir.join(format!("{}.ll", stem));
    let exe_path = temp_dir.join(&stem);

    fs::write(&source_path, source).expect("Failed to write source");

    let compile = Command::new(env!("CARGO_BIN_EXE_korec"))
        .arg(&source_path)
        .arg("--emit=llvm-ir")
        .arg("-o")
        .arg(&ll_path)
        .output()
        .expect("Failed to compile");

    if !compile.status.success() {
        panic!(
            "Compilation failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&compile.stdout),
            String::from_utf8_lossy(&compile.stderr)
        );
    }

    let runtime_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime");
    let link = Command::new("clang")
        .arg(&ll_path)
        .arg(runtime_dir.join("cmdline.c"))
        .arg("-o")
        .arg(&exe_path)
        .output()
        .expect("Failed to link");

    if !link.status.success() {
        panic!(
            "Linking failed:\n{}",
            String::from_utf8_lossy(&link.stderr)
        );
    }

    let run = Command::new(&exe_path).output().expect("Failed to run");

    let _ = fs::remove_file(&source_path);
    let _ = fs::remove_file(&ll_path);
    let _ = fs::remove_file(&exe_path);

    (
        String::from_utf8_lossy(&run.stdout).to_string(),
        run.status.code().unwrap_or(-1),
    )
}

/// Bug 1：具名联合上所有臂都产出 void（println 返回 void）。
/// 回归前：codegen 失败 "Symbol not found: local"。
#[test]
fn void_arms_on_named_union_compile_and_run() {
    let source = r#"
Result :: .Ok(i32) | .Err(i32)

main :: () i32 => {
    r := .Err(3)
    ? r is {
        .Ok(v) => println("ok"),
        .Err(e) => println("err")
    }
    ret 0
}
"#;

    let (stdout, code) = compile_and_run(source, "void_arms");
    assert_eq!(stdout.trim(), "err");
    assert_eq!(code, 0);
}

/// Bug 1 + 2：本地构造的具名联合，str payload 经 void 臂打印。
#[test]
fn named_union_str_payload_prints_intact() {
    let source = r#"
Result :: .Ok(i32) | .Err(str)

main :: () i32 => {
    r := .Err("boom")
    ? r is {
        .Ok(v) => println("ok"),
        .Err(e) => println(e)
    }
    ret 0
}
"#;

    let (stdout, code) = compile_and_run(source, "named_str");
    assert_eq!(stdout.trim(), "boom");
    assert_eq!(code, 0);
}

/// Bug 2：`i32 ! str` 的 err payload 跨函数边界返回后解构打印。
/// 回归前：绑定类型退化成 i32，输出整数乱码（如 -2015574200）。
#[test]
fn err_union_str_payload_across_function_boundary() {
    let source = r#"
pick :: (n i32) i32 ! str => {
    ? n <= 0 => ret .Err("boom")
    ret .Ok(n)
}

main :: () i32 => {
    bad := pick(0 - 5)
    ? bad is {
        .Ok(v) => println("ok"),
        .Err(e) => println(e)
    }
    ret 0
}
"#;

    let (stdout, code) = compile_and_run(source, "errunion_str");
    assert_eq!(stdout.trim(), "boom");
    assert_eq!(code, 0);
}

/// Bug 2 的加长版：长字符串能暴露 len 字段被截断的情况。
#[test]
fn err_union_long_str_payload_roundtrips() {
    let source = r#"
pick :: (n i32) i32 ! str => {
    ? n <= 0 => ret .Err("this-is-a-long-error-string")
    ret .Ok(n)
}

main :: () i32 => {
    bad := pick(0 - 5)
    ? bad is {
        .Ok(v) => println("unexpected ok"),
        .Err(e) => println(e)
    }
    ret 0
}
"#;

    let (stdout, code) = compile_and_run(source, "errunion_long_str");
    assert_eq!(stdout.trim(), "this-is-a-long-error-string");
    assert_eq!(code, 0);
}

/// 两条分支都走一遍：ok 分支不应误读 err payload。
#[test]
fn err_union_ok_branch_not_corrupted_by_err_payload() {
    let source = r#"
pick :: (n i32) i32 ! str => {
    ? n <= 0 => ret .Err("bad-input")
    ret .Ok(n)
}

main :: () i32 => {
    good := pick(7)
    ? good is {
        .Ok(v) => println("ok branch"),
        .Err(e) => println(e)
    }
    ret 0
}
"#;

    let (stdout, code) = compile_and_run(source, "errunion_ok_branch");
    assert_eq!(stdout.trim(), "ok branch");
    assert_eq!(code, 0);
}

/// void 臂与产值臂混用必须被类型检查拒绝。
///
/// 降级层只按首个臂的类型分配 phi temp，另一类臂的值会被静默丢弃：
/// 曾经 codegen 会报 "Symbol not found: local" 兜住这个漏洞，
/// 修掉 void 臂的 temp 分配后就变成了"编译通过但值消失"。
/// 必须在类型检查阶段报错，而不是产出错误的程序。
#[test]
fn mixed_void_and_value_arms_rejected_void_first() {
    let source = r#"
Result :: .Ok(i32) | .Err(i32)

main :: () i32 => {
    r := .Ok(1)
    x := ? r is {
        .Ok(v) => println("first-arm-void"),
        .Err(e) => 42
    }
    ret 0
}
"#;

    let (ok, out) = compile_only(source, "mixed_void_first");
    assert!(!ok, "void 臂与 i32 臂混用应当编译失败，实际却通过了:\n{}", out);
    assert!(
        out.contains("E4004"),
        "应当报类型不一致错误 E4004，实际输出:\n{}",
        out
    );
}

/// 同上，但臂顺序相反——首个臂产值、后续臂 void。
///
/// 单独测这个方向是因为 phi temp 的类型取自首个臂，
/// 两种顺序走的是不同的代码路径。
#[test]
fn mixed_void_and_value_arms_rejected_value_first() {
    let source = r#"
Result :: .Ok(i32) | .Err(i32)

main :: () i32 => {
    r := .Ok(1)
    x := ? r is {
        .Ok(v) => 42,
        .Err(e) => println("second-arm-void")
    }
    ret 0
}
"#;

    let (ok, out) = compile_only(source, "mixed_value_first");
    assert!(!ok, "i32 臂与 void 臂混用应当编译失败，实际却通过了:\n{}", out);
    assert!(
        out.contains("E4004"),
        "应当报类型不一致错误 E4004，实际输出:\n{}",
        out
    );
}

/// 语句位置的分支结果本就被丢弃，混用 void 臂与产值臂是合法的，不应误报 E4004。
///
/// 这是上面两个测试的反面约束：值位置必须拒绝，语句位置必须放行。
/// 两个 scrutinee 分别覆盖 void 臂和产值臂被实际执行的路径。
#[test]
fn mixed_arms_in_statement_position_allowed() {
    let source = r#"
Result :: .Ok(i32) | .Err(i32)

double :: (n i32) i32 => {
    ret n * 2
}

main :: () i32 => {
    ok := .Ok(1)
    ? ok is {
        .Ok(v) => println("void-arm-taken"),
        .Err(e) => double(e)
    }
    bad := .Err(21)
    ? bad is {
        .Ok(w) => println("not-taken"),
        .Err(f) => double(f)
    }
    println("done")
    ret 0
}
"#;

    let (out, code) = compile_and_run(source, "stmt_pos_mixed");
    assert_eq!(code, 0, "程序应正常退出，实际退出码 {}，输出:\n{}", code, out);
    assert_eq!(
        out, "void-arm-taken\ndone\n",
        "语句位置的混用臂应正常执行，实际输出:\n{}",
        out
    );
}

/// 语句位置混用臂时，**产值臂在前**同样要能降级。
///
/// 降级层按首个臂的类型分配 phi temp（control.rs 的 result_local）。
/// 产值臂在前会分配非 void 的 temp，此时后续 void 臂的结果若被无条件
/// 写入该 temp，codegen 会报 "void has no value"。void 臂在前之所以
/// 不触发，只是因为 result_local 保持 None 而绕过了赋值路径 —— 故此
/// 用例专门固定产值臂在前的臂序，与上一个用例互补。
#[test]
fn mixed_arms_in_statement_position_value_arm_first() {
    let source = r#"
Result :: .Ok(i32) | .Err(i32)

double :: (n i32) i32 => {
    ret n * 2
}

main :: () i32 => {
    ok := .Ok(1)
    ? ok is {
        .Ok(v) => double(v),
        .Err(e) => println("err-arm-not-taken")
    }
    bad := .Err(21)
    ? bad is {
        .Ok(w) => double(w),
        .Err(f) => println("err-arm-taken")
    }
    println("done")
    ret 0
}
"#;

    let (out, code) = compile_and_run(source, "stmt_pos_mixed_value_first");
    assert_eq!(code, 0, "程序应正常退出，实际退出码 {}，输出:\n{}", code, out);
    assert_eq!(
        out, "err-arm-taken\ndone\n",
        "产值臂在前的语句位置混用臂应正常执行，实际输出:\n{}",
        out
    );
}

/// 嵌套在调用实参里的分支属于值位置，语句位置标记不得穿透传递。
///
/// 若 `check_expr` 忘记在递归前取走标记，外层 `Stmt::Expr` 的标记会被
/// 实参中的分支误继承，导致值丢失重新变成静默错误。
#[test]
fn mixed_arms_nested_in_call_arg_rejected() {
    let source = r#"
Result :: .Ok(i32) | .Err(i32)

double :: (n i32) i32 => {
    ret n * 2
}

main :: () i32 => {
    r := .Ok(1)
    double(? r is {
        .Ok(v) => println("void arm"),
        .Err(e) => double(e)
    })
    ret 0
}
"#;

    let (ok, out) = compile_only(source, "nested_call_arg");
    assert!(
        !ok,
        "值位置（调用实参）的混用臂应当编译失败，实际却通过了:\n{}",
        out
    );
    assert!(
        out.contains("E4004"),
        "应当报类型不一致错误 E4004，实际输出:\n{}",
        out
    );
}
