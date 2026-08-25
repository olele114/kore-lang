//! LLVM 代码生成核心。
//!
//! 提供 CodegenContext 管理 LLVM 上下文、模块和构建器。

use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::Builder;
use inkwell::types::BasicTypeEnum;
use inkwell::values::FunctionValue;
use std::collections::HashMap;

use crate::middleend::hir::{HirModule, ty::HirType};
use crate::diag::DiagSink;

pub mod module;
pub mod function;
pub mod ty;
pub mod stmt;
pub mod rvalue;
pub mod place;
pub mod const_;
pub mod runtime;

use module::codegen_module;

/// 代码生成错误
#[derive(Debug)]
pub enum CodegenError {
    /// 类型转换失败
    TypeConversion(String),
    /// 未找到符号
    SymbolNotFound(String),
    /// LLVM 构建失败
    BuildError(String),
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            CodegenError::TypeConversion(msg) => write!(f, "Type conversion error: {}", msg),
            CodegenError::SymbolNotFound(msg) => write!(f, "Symbol not found: {}", msg),
            CodegenError::BuildError(msg) => write!(f, "LLVM build error: {}", msg),
        }
    }
}

impl std::error::Error for CodegenError {}

/// LLVM 代码生成上下文
pub struct CodegenContext<'ctx> {
    /// LLVM 上下文
    pub context: &'ctx Context,
    /// LLVM 模块
    pub module: Module<'ctx>,
    /// IR 构建器
    pub builder: Builder<'ctx>,

    /// HIR 类型 → LLVM 类型缓存
    pub type_cache: HashMap<HirType, BasicTypeEnum<'ctx>>,
    /// 函数名 → LLVM 函数值
    pub functions: HashMap<String, FunctionValue<'ctx>>,

    /// HIR 模块引用（用于查询类型定义）
    pub hir_module: Option<&'ctx HirModule>,
}

impl<'ctx> CodegenContext<'ctx> {
    /// 创建新的代码生成上下文
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();

        Self {
            context,
            module,
            builder,
            type_cache: HashMap::new(),
            functions: HashMap::new(),
            hir_module: None,
        }
    }

    /// 设置 HIR 模块引用
    pub fn set_hir_module(&mut self, hir: &'ctx HirModule) {
        self.hir_module = Some(hir);
    }
}

/// 将 HIR 模块编译为 LLVM IR
///
/// # 参数
/// - `hir`: HIR 模块
/// - `diag`: 诊断信息接收器
///
/// # 返回
/// LLVM IR 字符串,失败时返回 None
pub fn compile_to_llvm(hir: &HirModule, diag: &mut DiagSink) -> Option<String> {
    let context = Context::create();
    let mut codegen_ctx = CodegenContext::new(&context, "kore_module");

    // 设置 HIR 模块引用，用于类型定义查询
    codegen_ctx.set_hir_module(hir);

    match codegen_module(&mut codegen_ctx, hir) {
        Ok(()) => {
            let ir = codegen_ctx.module.print_to_string().to_string();
            crate::trace!("=== Generated LLVM IR ===\n{}\n=== END IR ===", ir);
            Some(ir)
        }
        Err(e) => {
            // 后端失败走 diag 通道（ADR 009）：无门控 eprintln! 会污染
            // stage2/stage3 的逐字节比较。codegen 错误没有源码位置，用
            // DiagLoc::None。
            diag.emit(crate::diag::Diagnostic::error(
                crate::diag::ErrorCode::CodegenFailed.as_u16(),
                format!("代码生成失败: {}", e),
                crate::diag::DiagLoc::None,
            ));
            None
        }
    }
}
