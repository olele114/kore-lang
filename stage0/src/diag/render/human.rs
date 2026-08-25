//! 人读渲染器。默认格式。

use super::TOO_MANY_ERRORS;
use crate::diag::diagnostic::{DiagLoc, Diagnostic};

pub fn render(diags: &[Diagnostic], suppressed: u32) -> String {
    let mut out = String::new();
    for d in diags {
        out.push_str(d.severity.as_str());
        out.push('[');
        out.push_str(&d.code_str());
        out.push_str("]: ");
        out.push_str(&d.msg);
        out.push('\n');

        match d.loc {
            DiagLoc::None => {}
            DiagLoc::File(f) => {
                out.push_str(&format!("  --> file#{}\n", f.0));
            }
            DiagLoc::At(s) => {
                out.push_str(&format!("  --> file#{}:{}..{}\n", s.file.0, s.lo, s.hi));
            }
        }

        for c in &d.children {
            out.push_str("  = ");
            out.push_str(c.severity.as_str());
            out.push_str(": ");
            out.push_str(&c.msg);
            if let Some(s) = c.span {
                out.push_str(&format!(" (file#{}:{}..{})", s.file.0, s.lo, s.hi));
            }
            out.push('\n');
        }

        if d.occurrences > 1 {
            out.push_str(&format!("  = note: 同一诊断出现 {} 次\n", d.occurrences));
        }
    }
    if suppressed > 0 {
        out.push_str(TOO_MANY_ERRORS);
        out.push_str(&format!("（另有 {suppressed} 条未显示）\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::diagnostic::{FileId, Severity, Span, SubDiag};

    #[test]
    fn renders_code_severity_and_span() {
        let d = Diagnostic::error(
            4001,
            "类型不匹配",
            DiagLoc::At(Span::new(FileId(0), 10, 14)),
        )
        .child(SubDiag::new(Severity::Note, "期望 u32"));
        let s = render(&[d], 0);
        assert!(s.contains("error[E4001]: 类型不匹配"));
        assert!(s.contains("file#0:10..14"));
        assert!(s.contains("note: 期望 u32"));
    }

    #[test]
    fn reports_occurrence_count() {
        let mut d = Diagnostic::error(2001, "语法错误", DiagLoc::None);
        d.occurrences = 7;
        assert!(render(&[d], 0).contains("出现 7 次"));
    }

    #[test]
    fn throttling_notice_is_last_line() {
        let d = Diagnostic::error(2001, "语法错误", DiagLoc::None);
        let s = render(&[d], 3);
        let last = s.lines().next_back().unwrap();
        assert!(last.starts_with(TOO_MANY_ERRORS));
        assert!(last.contains('3'));
    }
}
