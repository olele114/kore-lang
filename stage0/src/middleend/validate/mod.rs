//! HIR 验证器。
//!
//! 验证 HIR 的结构正确性，包括：
//! - CFG 完整性（所有块可达、无悬空引用）
//! - 终结符完整性（每个块都有终结符）
//! - BlockId 引用有效性
//! - 局部变量使用前已定义

pub mod cfg;
pub mod local;
pub mod typecheck;

use crate::middleend::hir::{HirBody, HirUnion};
use crate::diag::DiagSink;

/// HIR 验证器
pub struct Validator<'a> {
    unions: &'a [HirUnion],
    sink: &'a mut DiagSink,
}

impl<'a> Validator<'a> {
    pub fn new(unions: &'a [HirUnion], sink: &'a mut DiagSink) -> Self {
        Self { unions, sink }
    }

    /// 验证 HIR body
    pub fn validate_body(&mut self, body: &HirBody) -> bool {
        let cfg_ok = cfg::validate_cfg(body, self.sink);
        let locals_ok = local::validate_locals(body, self.sink);
        let types_ok = typecheck::validate_types(body, self.unions, self.sink);
        cfg_ok && locals_ok && types_ok
    }
}
