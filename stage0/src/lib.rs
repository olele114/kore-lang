//! stage0 —— 自举用的最小 Kore 编译器，宿主语言是 Rust。
//!
//! 自举闭合（stage2 与 stage3 逐字节相同）后本 crate 归档，因此刻意不
//! 追求工程完备度。此处的结构以「能被 stage1 用 Kore0 机械重写」为准，
//! 而不是以 Rust 的惯用法为准。

pub mod backend;
pub mod diag;
pub mod driver;
pub mod frontend;
pub mod ice;
pub mod middleend;
pub mod trace;

/// ADR 009 Q21 的四档退出码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// 成功。
    Ok = 0,
    /// 编译错误。
    CompileError = 1,
    /// 用法错误。
    UsageError = 2,
    /// ICE。`101` 而非 `3` 是为了与 Rust 对齐，stage0 就是 Rust 写的。
    Ice = 101,
}

impl ExitCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_match_adr_009() {
        assert_eq!(ExitCode::Ok.as_i32(), 0);
        assert_eq!(ExitCode::CompileError.as_i32(), 1);
        assert_eq!(ExitCode::UsageError.as_i32(), 2);
        assert_eq!(ExitCode::Ice.as_i32(), 101);
    }
}
