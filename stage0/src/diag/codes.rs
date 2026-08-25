//! 错误码和警告码定义。
//!
//! 编码方案：
//! - 错误码：E{:04}，u16 存储。号段见 `errors/registry.sexp`（ADR 009 定为唯一真源）：
//!   E1xxx 词法 / E2xxx 语法 / E3xxx 名字解析 / E4xxx 类型 /
//!   E5xxx 内存与所有权 / E6xxx 编译期求值 / E7xxx 代码生成 / E9xxx driver
//! - 警告码：W{:04}，u16 存储，范围 W3xxx（未使用）/ W5xxx（风格）
//!
//! 注意：本枚举里若干既有条目（`StringLiteralCrossesLine` = 2001、
//! `UnterminatedString` = 4001、`UndefinedName` = 4002、`Redefinition` = 4003）
//! 与登记表号段不符，且 4001 与登记表中的"类型不匹配"撞号。这是本模块引入前
//! 就存在的偏差，修正需同时改词法/语法/resolve 与其测试，故单列处理。

use std::fmt;
use std::str::FromStr;

/// 错误码。存储为 u16，渲染为 E{:04}。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
#[non_exhaustive]
pub enum ErrorCode {
    /// E2001: 字符串字面量不能跨行
    StringLiteralCrossesLine = 2001,
    /// E4001: 未闭合的字符串
    UnterminatedString = 4001,
    /// E4002: 未定义的名字
    UndefinedName = 4002,
    /// E4003: 重复定义
    Redefinition = 4003,
    /// E4004: 类型不匹配
    TypeMismatch = 4004,
    /// E4005: 无效的函数调用
    InvalidFunctionCall = 4005,
    /// E4006: 未定义的模块
    UndefinedModule = 4006,
    /// E4007: 未定义的符号（跨模块访问）
    UndefinedSymbol = 4007,
    /// E4008: 私有符号
    PrivateSymbol = 4008,
    /// E4009: 循环依赖
    CircularDependency = 4009,
    /// E4021: 错误传播用于非错误联合类型
    PropagateNonErrUnion = 4021,
    /// E4022: 错误类型不兼容
    IncompatibleErrorType = 4022,
    /// E4023: 缺少错误处理
    MissingErrorHandling = 4023,
    /// E5001: 移动后使用
    UseAfterMove = 5001,
    /// E5002: 借用指针逃逸到堆
    BorrowEscapesToHeap = 5002,
    /// E5003: 借用指针逃逸到返回值
    BorrowEscapesToReturn = 5003,
    /// E6001: 编译期求值错误
    ComptimeEvalError = 6001,
    /// E6002: 编译期求值步数超限
    ComptimeEvalStepLimitExceeded = 6002,
    /// E7001: 未实现的功能
    Unimplemented = 7001,
    /// E7002: 代码生成失败
    CodegenFailed = 7002,
    /// E9001: 内部编译器错误
    InternalCompilerError = 9001,
}

impl ErrorCode {
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{:04}", self.as_u16())
    }
}

impl FromStr for ErrorCode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let digits = s.strip_prefix('E')
            .or_else(|| s.strip_prefix('e'))
            .unwrap_or(s);

        let code = digits.parse::<u16>().map_err(|_| ())?;

        match code {
            2001 => Ok(ErrorCode::StringLiteralCrossesLine),
            4001 => Ok(ErrorCode::UnterminatedString),
            4002 => Ok(ErrorCode::UndefinedName),
            4003 => Ok(ErrorCode::Redefinition),
            4004 => Ok(ErrorCode::TypeMismatch),
            4005 => Ok(ErrorCode::InvalidFunctionCall),
            4006 => Ok(ErrorCode::UndefinedModule),
            4007 => Ok(ErrorCode::UndefinedSymbol),
            4008 => Ok(ErrorCode::PrivateSymbol),
            4009 => Ok(ErrorCode::CircularDependency),
            4021 => Ok(ErrorCode::PropagateNonErrUnion),
            4022 => Ok(ErrorCode::IncompatibleErrorType),
            4023 => Ok(ErrorCode::MissingErrorHandling),
            5001 => Ok(ErrorCode::UseAfterMove),
            5002 => Ok(ErrorCode::BorrowEscapesToHeap),
            5003 => Ok(ErrorCode::BorrowEscapesToReturn),
            6001 => Ok(ErrorCode::ComptimeEvalError),
            6002 => Ok(ErrorCode::ComptimeEvalStepLimitExceeded),
            7001 => Ok(ErrorCode::Unimplemented),
            7002 => Ok(ErrorCode::CodegenFailed),
            9001 => Ok(ErrorCode::InternalCompilerError),
            _ => Err(()),
        }
    }
}

/// 警告码。存储为 u16，渲染为 W{:04}。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
#[non_exhaustive]
pub enum WarningCode {
    /// W3001: 未使用的变量
    UnusedVariable = 3001,
    /// W5001: 不建议的命名风格
    UnconventionalNaming = 5001,
}

impl WarningCode {
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

impl fmt::Display for WarningCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "W{:04}", self.as_u16())
    }
}

impl FromStr for WarningCode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let digits = s.strip_prefix('W')
            .or_else(|| s.strip_prefix('w'))
            .unwrap_or(s);

        let code = digits.parse::<u16>().map_err(|_| ())?;

        match code {
            3001 => Ok(WarningCode::UnusedVariable),
            5001 => Ok(WarningCode::UnconventionalNaming),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_display() {
        assert_eq!(ErrorCode::UnterminatedString.to_string(), "E4001");
        assert_eq!(ErrorCode::StringLiteralCrossesLine.to_string(), "E2001");
    }

    #[test]
    fn error_code_parse() {
        assert_eq!("E4001".parse(), Ok(ErrorCode::UnterminatedString));
        assert_eq!("4001".parse(), Ok(ErrorCode::UnterminatedString));
        assert_eq!("e4001".parse(), Ok(ErrorCode::UnterminatedString));
        assert!("W3001".parse::<ErrorCode>().is_err());
        assert!("E9999".parse::<ErrorCode>().is_err());
    }

    #[test]
    fn warning_code_display() {
        assert_eq!(WarningCode::UnusedVariable.to_string(), "W3001");
        assert_eq!(WarningCode::UnconventionalNaming.to_string(), "W5001");
    }

    #[test]
    fn warning_code_parse() {
        assert_eq!("W3001".parse(), Ok(WarningCode::UnusedVariable));
        assert_eq!("3001".parse(), Ok(WarningCode::UnusedVariable));
        assert_eq!("w3001".parse(), Ok(WarningCode::UnusedVariable));
        assert!("E4001".parse::<WarningCode>().is_err());
        assert!("W9999".parse::<WarningCode>().is_err());
    }
}
