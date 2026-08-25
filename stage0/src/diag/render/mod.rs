//! 三个渲染器。ADR 009：它们各自独立消费同一份 `[Diagnostic]`，
//! 互不复用中间产物，也不回写 sink。

pub mod human;
pub mod json;
pub mod short;

use super::diagnostic::Diagnostic;

/// --error-format 的取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorFormat {
    Human,
    Json,
    Short,
}

impl ErrorFormat {
    pub fn parse(s: &str) -> Option<ErrorFormat> {
        match s {
            "human" => Some(ErrorFormat::Human),
            "json" => Some(ErrorFormat::Json),
            "short" => Some(ErrorFormat::Short),
            _ => None,
        }
    }
}

/// 按格式渲染。返回值写 stderr。
///
/// `suppressed` 是被 `--error-limit` 丢弃的错误条数。它必须走渲染器而不是
/// 由调用方追加明文：JSON 的输出是单个对象，任何尾随文本都会让 IDE 解析
/// 失败。ADR 009 要求节流「对所有 renderer 一致」，一致指的是都要如实报告，
/// 而不是都用同一句人话。
pub fn render(format: ErrorFormat, diags: &[Diagnostic], suppressed: u32) -> String {
    match format {
        ErrorFormat::Human => human::render(diags, suppressed),
        ErrorFormat::Json => json::render(diags, suppressed),
        ErrorFormat::Short => short::render(diags, suppressed),
    }
}

/// 超限提示语。ADR 009 规定的字面量，human 与 short 共用。
pub const TOO_MANY_ERRORS: &str = "error: too many errors, stopping";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_three_formats() {
        assert_eq!(ErrorFormat::parse("human"), Some(ErrorFormat::Human));
        assert_eq!(ErrorFormat::parse("json"), Some(ErrorFormat::Json));
        assert_eq!(ErrorFormat::parse("short"), Some(ErrorFormat::Short));
        assert_eq!(ErrorFormat::parse("pretty"), None);
    }

    #[test]
    fn every_format_reports_throttling() {
        use crate::diag::diagnostic::DiagLoc;
        let d = [Diagnostic::error(2001, "e", DiagLoc::None)];
        // 三个渲染器都要如实报告节流，方言可以不同。
        assert!(render(ErrorFormat::Human, &d, 8).contains(TOO_MANY_ERRORS));
        assert!(render(ErrorFormat::Short, &d, 8).contains(TOO_MANY_ERRORS));
        assert!(render(ErrorFormat::Json, &d, 8).contains("\"suppressed\": 8"));
    }

    #[test]
    fn no_throttling_notice_when_nothing_suppressed() {
        use crate::diag::diagnostic::DiagLoc;
        let d = [Diagnostic::error(2001, "e", DiagLoc::None)];
        assert!(!render(ErrorFormat::Human, &d, 0).contains(TOO_MANY_ERRORS));
        assert!(!render(ErrorFormat::Short, &d, 0).contains(TOO_MANY_ERRORS));
    }
}
