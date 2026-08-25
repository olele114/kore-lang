//! 链接器集成模块。
//!
//! 提供将 LLVM IR 编译为目标文件并链接为可执行文件的功能。

use std::path::{Path, PathBuf};
use std::process::Command;
use inkwell::context::Context;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::OptimizationLevel;

use crate::middleend::hir::HirModule;
use crate::diag::DiagSink;
use crate::backend::llvm::CodegenContext;
use crate::backend::llvm::module::codegen_module;

/// 链接器错误
#[derive(Debug)]
pub enum LinkerError {
    /// LLVM 目标初始化失败
    TargetInitFailed(String),
    /// 代码生成失败
    CodegenFailed(String),
    /// 目标文件写入失败
    ObjectWriteFailed(String),
    /// 链接器调用失败
    LinkerFailed(String),
}

impl std::fmt::Display for LinkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LinkerError::TargetInitFailed(msg) => write!(f, "Target init failed: {}", msg),
            LinkerError::CodegenFailed(msg) => write!(f, "Codegen failed: {}", msg),
            LinkerError::ObjectWriteFailed(msg) => write!(f, "Object write failed: {}", msg),
            LinkerError::LinkerFailed(msg) => write!(f, "Linker failed: {}", msg),
        }
    }
}

impl std::error::Error for LinkerError {}

/// codegen 失败的双通道上报：进 DiagSink（E7002，用户可见），同时返回
/// LinkerError 让调用方决定退出码。codegen 错误没有源码位置，用
/// DiagLoc::None（ADR 009）。
fn emit_codegen_failure(diag: &mut DiagSink, msg: String) -> LinkerError {
    diag.emit(crate::diag::Diagnostic::error(
        crate::diag::ErrorCode::CodegenFailed.as_u16(),
        format!("代码生成失败: {}", msg),
        crate::diag::DiagLoc::None,
    ));
    LinkerError::CodegenFailed(msg)
}

/// 编译产物类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitType {
    /// LLVM IR (.ll)
    LlvmIr,
    /// 汇编 (.s)
    Assembly,
    /// 目标文件 (.o)
    Object,
}

/// 将 HIR 编译为目标文件
///
/// # 参数
/// - `hir`: HIR 模块
/// - `output_path`: 输出文件路径
/// - `emit_type`: 编译产物类型
/// - `diag`: 诊断信息接收器
pub fn compile_to_object(
    hir: &HirModule,
    output_path: &Path,
    emit_type: EmitType,
    diag: &mut DiagSink,
) -> Result<(), LinkerError> {
    // 初始化 LLVM 目标
    Target::initialize_aarch64(&InitializationConfig::default());
    Target::initialize_x86(&InitializationConfig::default());

    let context = Context::create();
    let mut codegen_ctx = CodegenContext::new(&context, "kore_module");
    codegen_ctx.set_hir_module(hir);

    // 生成 LLVM IR
    if let Err(e) = codegen_module(&mut codegen_ctx, hir) {
        return Err(emit_codegen_failure(diag, e.to_string()));
    }

    crate::trace!("=== Generated LLVM IR ===\n{}\n=== END IR ===",
        codegen_ctx.module.print_to_string().to_string());

    // 处理 LLVM IR 输出
    if emit_type == EmitType::LlvmIr {
        let ir = codegen_ctx.module.print_to_string().to_string();
        std::fs::write(output_path, ir)
            .map_err(|e| LinkerError::ObjectWriteFailed(e.to_string()))?;
        return Ok(());
    }

    // 获取目标三元组
    let target_triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&target_triple)
        .map_err(|e| LinkerError::TargetInitFailed(e.to_string()))?;

    // 创建目标机器
    let target_machine = target
        .create_target_machine(
            &target_triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| LinkerError::TargetInitFailed("Failed to create target machine".into()))?;

    // 设置目标数据布局
    codegen_ctx.module.set_data_layout(&target_machine.get_target_data().get_data_layout());
    codegen_ctx.module.set_triple(&target_triple);

    // 生成目标文件或汇编
    let file_type = match emit_type {
        EmitType::Assembly => FileType::Assembly,
        EmitType::Object => FileType::Object,
        EmitType::LlvmIr => unreachable!(),
    };

    crate::trace!("=== About to write to file: {:?}, type: {:?}", output_path, file_type);
    crate::trace!("=== Module verification:");
    if let Err(e) = codegen_ctx.module.verify() {
        crate::trace!("  LLVM module verification FAILED: {}", e.to_string());
        return Err(emit_codegen_failure(
            diag,
            format!("Module verification failed: {}", e.to_string()),
        ));
    }
    crate::trace!("  Module verification PASSED");

    crate::trace!("=== Writing to file...");
    target_machine
        .write_to_file(&codegen_ctx.module, file_type, output_path)
        .map_err(|e| LinkerError::ObjectWriteFailed(e.to_string()))?;
    crate::trace!("=== Write completed successfully");

    Ok(())
}

/// 链接目标文件为可执行文件
///
/// # 参数
/// - `object_path`: 目标文件路径
/// - `output_path`: 可执行文件输出路径
pub fn link_executable(object_path: &Path, output_path: &Path) -> Result<(), LinkerError> {
    // 编译运行时库
    let runtime_src = Path::new("runtime/kore_runtime.c");
    let runtime_obj = PathBuf::from(format!(
        "/data/data/com.termux/files/tmp/kore_runtime_{}.o",
        std::process::id()
    ));

    let compile_status = Command::new("clang")
        .arg("-c")
        .arg(runtime_src)
        .arg("-o")
        .arg(&runtime_obj)
        .status()
        .map_err(|e| LinkerError::LinkerFailed(format!("Failed to compile runtime: {}", e)))?;

    if !compile_status.success() {
        return Err(LinkerError::LinkerFailed(format!(
            "Runtime compilation failed with code: {:?}",
            compile_status.code()
        )));
    }

    // 使用 clang 作为链接器，链接主程序和运行时库
    let status = Command::new("clang")
        .arg(object_path)
        .arg(&runtime_obj)
        .arg("-o")
        .arg(output_path)
        .status()
        .map_err(|e| LinkerError::LinkerFailed(format!("Failed to spawn linker: {}", e)))?;

    // 清理运行时临时文件
    let _ = std::fs::remove_file(&runtime_obj);

    if !status.success() {
        return Err(LinkerError::LinkerFailed(format!(
            "Linker exited with code: {:?}",
            status.code()
        )));
    }

    Ok(())
}

/// 完整编译流程：HIR → 目标文件 → 可执行文件
///
/// # 参数
/// - `hir`: HIR 模块
/// - `output_path`: 可执行文件输出路径
/// - `diag`: 诊断信息接收器
pub fn compile_and_link(
    hir: &HirModule,
    output_path: &Path,
    diag: &mut DiagSink,
) -> Result<(), LinkerError> {
    // 生成唯一临时目标文件（避免并发测试冲突）
    let obj_path = PathBuf::from(format!(
        "/data/data/com.termux/files/tmp/kore_temp_{}.o",
        std::process::id()
    ));

    // 编译为目标文件
    compile_to_object(hir, &obj_path, EmitType::Object, diag)?;

    // 链接为可执行文件
    link_executable(&obj_path, output_path)?;

    // 清理临时文件
    let _ = std::fs::remove_file(&obj_path);

    Ok(())
}
