//! 编译器内部调试跟踪的门控。
//!
//! 这些跟踪是给编译器开发者看的，不是给 Kore 用户看的：用户可见的问题走
//! `diag`（ADR 009 的双通道）。默认关闭是自举的硬要求——闭合判据是 stage2
//! 与 stage3 逐字节相同，编译器无条件往 stderr 写东西会污染这个比较。
//!
//! 用全局开关而不是给每个 pass 传 `Options`：跟踪点散落在 lowering 与
//! codegen 的十几层调用里，为纯调试功能改这些签名会让 stage1 的机械重写
//! 多背一堆无关参数。

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// 跟踪是否开启。`trace!` 的判据，热路径上只是一次原子读。
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// 从 `--debug-trace` 与 `KORE_TRACE` 初始化。任一开启即开启。
///
/// 保留环境变量入口是为了在库测试里打开跟踪——那条路径没有 CLI。
pub fn init_from(flag: bool) {
    set_enabled(flag || std::env::var_os("KORE_TRACE").is_some());
}

/// 写一行调试跟踪到 stderr，仅在跟踪开启时。
///
/// 参数在关闭时不求值，所以跟踪点里做 `{:?}` 格式化不会有代价。
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        if $crate::trace::enabled() {
            eprintln!($($arg)*);
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_off_and_toggles() {
        // 默认关闭是自举闭合的前提。
        set_enabled(false);
        assert!(!enabled());
        set_enabled(true);
        assert!(enabled());
        set_enabled(false);
    }

    #[test]
    fn macro_does_not_evaluate_args_when_off() {
        set_enabled(false);
        let mut touched = false;
        crate::trace!("{}", {
            touched = true;
            1
        });
        assert!(!touched);
    }
}
