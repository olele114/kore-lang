//! 字符串类型 HIR 降级测试

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::driver::run_frontend;
use kore_stage0::middleend::lower::lower_module;
use kore_stage0::middleend::hir::ty::HirType;

#[test]
fn lower_string_literal() {
    let source = r#"
main :: () str => "hello"
"#;
    let mut sink = DiagSink::new();
    let frontend = run_frontend(FileId(0), source, &mut sink);

    assert!(!sink.has_errors(), "前端不应有错误");
    let module = frontend.module.expect("应生成模块");
    let symbols = frontend.symbols.expect("应生成符号表");
    let type_ctx = frontend.type_ctx.expect("应生成类型上下文");

    let hir = lower_module(&module, &symbols, &type_ctx, &mut sink);

    if sink.has_errors() {
        eprintln!("HIR 降级错误:");
        for d in sink.peek() {
            eprintln!("  {:?}", d);
        }
    }
    assert!(!sink.has_errors(), "HIR 降级不应有错误");

    // 验证生成了 main 函数（跳过内置函数）
    let main_fn = hir.functions.iter()
        .find(|f| f.name == "main")
        .expect("应有 main 函数");

    // 验证返回类型为 str
    assert_eq!(main_fn.ret_type, HirType::Str);
}

#[test]
fn lower_string_with_escapes() {
    let source = r#"
msg :: () str => "hello\nworld\t!"
"#;
    let mut sink = DiagSink::new();
    let frontend = run_frontend(FileId(0), source, &mut sink);

    assert!(!sink.has_errors());
    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut sink);
    assert!(!sink.has_errors(), "应正确降级带转义的字符串");

    let func = hir.functions.iter()
        .find(|f| f.name == "msg")
        .expect("应有 msg 函数");
    assert_eq!(func.ret_type, HirType::Str);
}

#[test]
fn lower_string_parameter() {
    let source = r#"
greet :: (name str) str => name
"#;
    let mut sink = DiagSink::new();
    let frontend = run_frontend(FileId(0), source, &mut sink);

    assert!(!sink.has_errors());
    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut sink);
    assert!(!sink.has_errors());

    let func = hir.functions.iter()
        .find(|f| f.name == "greet")
        .expect("应有 greet 函数");
    assert_eq!(func.params.len(), 1);
    assert_eq!(func.params[0].ty, HirType::Str);
    assert_eq!(func.ret_type, HirType::Str);
}

#[test]
fn lower_string_local_var() {
    let source = r#"
main :: () void => {
    ~msg := "test"
}
"#;
    let mut sink = DiagSink::new();
    let frontend = run_frontend(FileId(0), source, &mut sink);

    assert!(!sink.has_errors());
    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let _hir = lower_module(&module, &symbols, &type_ctx, &mut sink);

    if sink.has_errors() {
        eprintln!("HIR 降级错误:");
        for d in sink.peek() {
            eprintln!("  {:?}", d);
        }
    }
    assert!(!sink.has_errors(), "应正确降级字符串局部变量");
}

#[test]
fn lower_raw_string_literal() {
    let source = r#"
path :: () str => `C:\Users\test\file.txt`
"#;
    let mut sink = DiagSink::new();
    let frontend = run_frontend(FileId(0), source, &mut sink);

    assert!(!sink.has_errors());
    let module = frontend.module.unwrap();
    let symbols = frontend.symbols.unwrap();
    let type_ctx = frontend.type_ctx.unwrap();

    let hir = lower_module(&module, &symbols, &type_ctx, &mut sink);
    assert!(!sink.has_errors(), "应正确降级原始字符串");

    let func = hir.functions.iter()
        .find(|f| f.name == "path")
        .expect("应有 path 函数");
    assert_eq!(func.ret_type, HirType::Str);
}
