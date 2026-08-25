//! 单行渲染器。每条诊断一行，子诊断不展开。

use super::TOO_MANY_ERRORS;
use crate::diag::diagnostic::{DiagLoc, Diagnostic};

pub fn render(diags: &[Diagnostic], suppressed: u32) -> String {
    let mut out = String::new();
    for d in diags {
        let loc = match d.loc {
            DiagLoc::None => "<no-loc>".to_string(),
            DiagLoc::File(f) => format!("file#{}", f.0),
            DiagLoc::At(s) => format!("file#{}:{}", s.file.0, s.lo),
        };
        out.push_str(&format!(
            "{}: {}: {}: {}\n",
            loc,
            d.severity.as_str(),
            d.code_str(),
            d.msg
        ));
    }
    if suppressed > 0 {
        // short 保持一行一条的形状，提示语也不换行拼接额外内容。
        out.push_str(&format!("{TOO_MANY_ERRORS} ({suppressed})\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::diagnostic::{FileId, Severity, Span, SubDiag};

    #[test]
    fn one_line_per_diagnostic() {
        let d1 = Diagnostic::error(4001, "a", DiagLoc::At(Span::new(FileId(0), 5, 6)))
            .child(SubDiag::new(Severity::Note, "子诊断不展开"));
        let d2 = Diagnostic::warning(4002, "b", DiagLoc::None);
        let s = render(&[d1, d2], 0);
        assert_eq!(s.lines().count(), 2);
        assert!(s.contains("file#0:5: error: E4001: a"));
        assert!(s.contains("<no-loc>: warning: W4002: b"));
        assert!(!s.contains("子诊断不展开"));
    }

    #[test]
    fn throttling_adds_exactly_one_line() {
        let d = Diagnostic::error(4001, "a", DiagLoc::None);
        let s = render(&[d], 5);
        assert_eq!(s.lines().count(), 2);
        assert!(s.lines().next_back().unwrap().contains("(5)"));
    }
}
