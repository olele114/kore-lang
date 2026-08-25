//! 诊断子系统。ADR 009 的双通道中的「用户代码有问题」这一条。
//!
//! 分层：`diagnostic` 是数据，`sink` 累积与计数，`render` 渲染，
//! `registry` 提供 `--explain` 的长文。渲染不回写 sink，sink 不认识渲染。
//!
//! `diag` 位于 crate 顶层而非 `frontend` 或 `driver` 之下：所有 pass 都要
//! 报错，放进任一层都会造成依赖倒置（ADR 007 Q13）。

pub mod codes;
pub mod diagnostic;
pub mod registry;
pub mod render;
pub mod sink;

pub use codes::{ErrorCode, WarningCode};
pub use diagnostic::{DiagLoc, Diagnostic, FileId, Severity, Span, SubDiag};
pub use registry::{CodeStatus, Registry, RegistryError};
pub use render::{ErrorFormat, render};
pub use sink::DiagSink;

/// `--error-limit` 的默认值。ADR 009：默认 100，`0` 表示不限。
pub const DEFAULT_ERROR_LIMIT: u32 = 100;

/// 把 `--error-limit=N` 的原始值转成 sink 需要的 `Option<u32>`。
/// `0` 是「不限」而非「一条都不存」。
pub fn error_limit_from_arg(n: u32) -> Option<u32> {
    if n == 0 { None } else { Some(n) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_means_unlimited() {
        assert_eq!(error_limit_from_arg(0), None);
        assert_eq!(error_limit_from_arg(100), Some(100));
    }

    #[test]
    fn unlimited_stores_everything() {
        let mut sink = DiagSink::with_error_limit(error_limit_from_arg(0));
        for i in 0..500 {
            sink.emit(Diagnostic::error(
                2001,
                "e",
                DiagLoc::At(Span::new(FileId(0), i, i + 1)),
            ));
        }
        assert_eq!(sink.peek().len(), 500);
        assert_eq!(sink.suppressed(), 0);
    }
}
