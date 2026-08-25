//! 后端：LLVM IR 代码生成。
//!
//! ADR 011：HIR → LLVM IR 单向依赖。
//! 使用 inkwell 绑定生成 LLVM IR，支持多目标架构。

pub mod llvm;
pub mod linker;

pub use llvm::compile_to_llvm;
pub use linker::{compile_to_object, link_executable, compile_and_link, EmitType, LinkerError};
