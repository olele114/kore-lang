//! 前端：词法、语法、语义分析。
//!
//! ADR 007 Q13：frontend → middleend → backend 单向依赖链，driver 协调。
//! 前端模块按 pass 顺序排列，每个子模块自给，互不依赖（除了都读 `diag`）。

pub mod ast;
pub mod counters;
pub mod escape;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod resolve;
pub mod typecheck;
