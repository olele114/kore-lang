//! JSON 渲染器。ADR 009 要求输出携带 `"version": 1`。
//!
//! 手写序列化：不引入 serde，stage1 要把这段机械翻译成 Kore0。
//!
//! 节流提示以 `"suppressed": N` 字段表达，而不是像 human/short 那样追加一行
//! 文本——输出必须是单个可解析对象。字段常在（未节流时为 `0`），消费方不必
//! 为它准备缺失分支。

use crate::diag::diagnostic::{DiagLoc, Diagnostic};

/// 输出格式版本。ADR 009 明确要求。
pub const FORMAT_VERSION: u32 = 1;

pub fn render(diags: &[Diagnostic], suppressed: u32) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{{\"version\": {FORMAT_VERSION}, \"suppressed\": {suppressed}, \"diagnostics\": ["
    ));
    for (i, d) in diags.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&one(d));
    }
    out.push_str("]}\n");
    out
}

fn one(d: &Diagnostic) -> String {
    let mut s = String::new();
    s.push('{');
    s.push_str(&format!("\"severity\": \"{}\", ", d.severity.as_str()));
    s.push_str(&format!("\"code\": {}, ", d.code));
    s.push_str(&format!("\"msg\": {}, ", quote(&d.msg)));
    s.push_str(&format!("\"loc\": {}, ", loc(&d.loc)));
    s.push_str(&format!("\"occurrences\": {}, ", d.occurrences));
    s.push_str("\"children\": [");
    for (i, c) in d.children.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        let span = match c.span {
            Some(sp) => format!(
                "{{\"file\": {}, \"lo\": {}, \"hi\": {}}}",
                sp.file.0, sp.lo, sp.hi
            ),
            None => "null".to_string(),
        };
        s.push_str(&format!(
            "{{\"severity\": \"{}\", \"msg\": {}, \"span\": {}}}",
            c.severity.as_str(),
            quote(&c.msg),
            span
        ));
    }
    s.push_str("]}");
    s
}

fn loc(l: &DiagLoc) -> String {
    match l {
        DiagLoc::None => "null".to_string(),
        DiagLoc::File(f) => format!("{{\"file\": {}}}", f.0),
        DiagLoc::At(s) => format!(
            "{{\"file\": {}, \"lo\": {}, \"hi\": {}}}",
            s.file.0, s.lo, s.hi
        ),
    }
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::diagnostic::{FileId, Severity, Span, SubDiag};

    #[test]
    fn carries_version_one() {
        let s = render(&[], 0);
        assert!(s.contains("\"version\": 1"));
    }

    #[test]
    fn suppressed_field_always_present() {
        assert!(render(&[], 0).contains("\"suppressed\": 0"));
        assert!(render(&[], 42).contains("\"suppressed\": 42"));
    }

    #[test]
    fn throttling_keeps_output_a_single_object() {
        let d = Diagnostic::error(2001, "e", DiagLoc::None);
        let s = render(&[d], 9);
        // 唯一一行，且首尾就是这个对象的括号——没有尾随明文。
        assert_eq!(s.lines().count(), 1);
        let line = s.trim_end();
        assert!(line.starts_with('{'));
        assert!(line.ends_with('}'));
        assert!(!line.contains("too many errors"));
    }

    #[test]
    fn escapes_quotes_and_newlines() {
        let d = Diagnostic::error(1001, "含 \" 与\n换行", DiagLoc::None);
        let s = render(&[d], 0);
        assert!(s.contains("\\\""));
        assert!(s.contains("\\n"));
    }

    #[test]
    fn null_loc_and_null_child_span() {
        let d = Diagnostic::error(1002, "m", DiagLoc::None)
            .child(SubDiag::new(Severity::Help, "试试 as"));
        let s = render(&[d], 0);
        assert!(s.contains("\"loc\": null"));
        assert!(s.contains("\"span\": null"));
    }

    #[test]
    fn emits_span_fields() {
        let d = Diagnostic::warning(5001, "m", DiagLoc::At(Span::new(FileId(3), 7, 9)));
        let s = render(&[d], 0);
        assert!(s.contains("\"file\": 3"));
        assert!(s.contains("\"lo\": 7"));
        assert!(s.contains("\"hi\": 9"));
    }
}
