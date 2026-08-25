//! 中端：HIR（高级中间表示）与降级 Pass。
//!
//! ADR 011：frontend AST → middleend HIR → backend LLVM IR 单向依赖链。
//! HIR 使用显式 CFG 基本块表示，控制流通过 Terminator 显式化。

pub mod hir;
pub mod lower;
pub mod pass;
pub mod validate;
