//! codegen 错误路径的诊断回归测试（ADR 009）。
//!
//! 生产路径是 `compile_to_object`，它必须在 codegen 失败时把 E7002 写进
//! DiagSink，而不是只返回 `LinkerError` 让 main.rs 自己 eprintln!。后者会
//! 绕过 render，破坏「默认零 stderr」与 stage2/stage3 逐字节比较。
//!
//! 无法用 Kore 源码触发 codegen 失败（前端会先拦下所有畸形输入），因此
//! 这里直接构造畸形 HIR 打到 pass 1 的类型转换。

use kore_stage0::backend::{compile_to_object, EmitType, LinkerError};
use kore_stage0::diag::{DiagSink, FileId, Span};
use kore_stage0::middleend::hir::{HirFunction, HirModule, HirParam};
use kore_stage0::middleend::hir::ty::HirType;

/// 构造一个签名里带畸形类型的模块。
///
/// `register_functions`（codegen pass 1）会为每个参数调用 `convert_type`，
/// 宽度 7 不在 {8,16,32,64} 内，于是返回 `CodegenError::TypeConversion`。
/// 注意不能用「函数无 body」当触发器：`codegen_module` 用
/// `if func.body.is_some()` 守卫了那条分支，从生产入口不可达。
fn module_with_bad_param_type() -> HirModule {
    let span = Span::new(FileId(0), 0, 1);
    HirModule {
        functions: vec![HirFunction {
            name: "bad".to_string(),
            params: vec![HirParam {
                name: "x".to_string(),
                ty: HirType::Int { width: 7, signed: true },
                span,
            }],
            ret_type: HirType::Void,
            body: None,
            span,
        }],
        structs: vec![],
        unions: vec![],
        globals: vec![],
    }
}

#[test]
fn codegen_failure_emits_e7002() {
    let hir = module_with_bad_param_type();
    let out = std::env::temp_dir().join("kore_codegen_diag_e7002.ll");
    let mut sink = DiagSink::new();

    let result = compile_to_object(&hir, &out, EmitType::LlvmIr, &mut sink);

    assert!(
        matches!(result, Err(LinkerError::CodegenFailed(_))),
        "畸形签名应让 codegen 失败，实际: {:?}",
        result
    );

    assert!(sink.has_errors(), "codegen 失败必须进 DiagSink");
    assert!(
        sink.peek().iter().any(|d| d.code == 7002),
        "应上报 E7002，实际码号: {:?}",
        sink.peek().iter().map(|d| d.code).collect::<Vec<_>>()
    );

    let _ = std::fs::remove_file(&out);
}

#[test]
fn codegen_success_emits_nothing() {
    // 反向对照：合法模块不得往 DiagSink 里写任何东西，否则「零 stderr」
    // 的判据会被 false positive 污染。
    let span = Span::new(FileId(0), 0, 1);
    let hir = HirModule {
        functions: vec![HirFunction {
            name: "noop".to_string(),
            params: vec![HirParam {
                name: "x".to_string(),
                ty: HirType::i32(),
                span,
            }],
            ret_type: HirType::Void,
            body: None,
            span,
        }],
        structs: vec![],
        unions: vec![],
        globals: vec![],
    };
    let out = std::env::temp_dir().join("kore_codegen_diag_ok.ll");
    let mut sink = DiagSink::new();

    let result = compile_to_object(&hir, &out, EmitType::LlvmIr, &mut sink);

    assert!(result.is_ok(), "合法签名应通过 codegen，实际: {:?}", result);
    assert_eq!(sink.err_count(), 0, "成功路径不应有诊断");

    let _ = std::fs::remove_file(&out);
}
