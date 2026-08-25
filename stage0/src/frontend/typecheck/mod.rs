//! 类型检查。ADR 007 §185–206：类型推断、类型检查、借用检查。
//!
//! 本模块实现 Kore0 子集的类型系统，包括：
//! - 基础类型（i32, u64, f64, bool, str, void）
//! - 指针类型（^T, own ^T）
//! - 复合类型（数组、结构体、联合）
//! - 错误联合（T ! E）
//! - 函数类型

pub mod checker;
pub mod context;
pub mod types;

pub use checker::TypeChecker;
pub use context::TypeContext;
pub use types::Type;
