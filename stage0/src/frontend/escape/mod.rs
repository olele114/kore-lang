//! 不逃逸检查（escape check）。
//!
//! 实现 docs/spec/05-memory.md §2 的两条编译期规则：
//! - 移动后不可用（E5001）
//! - 借用指针不逃逸（E5002 / E5003）
//!
//! Kore 没有生命周期变量，检查是流敏感的、保守的：不确定时报错。

pub mod checker;
pub mod context;

pub use checker::EscapeChecker;
pub use context::{BindingInfo, BindingKind, EscapeContext, OwnershipState};
