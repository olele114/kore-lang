//! 测试注解验证器。ADR 010 Q3-Q5：验证 `--~` 注解标记的预期诊断。
//!
//! 支持错误注解（`--~ E4001`）和警告注解（`--~ W3001`）。
//!
//! 验证流程：
//! 1. 扫描源码，提取所有 `--~` 注解（行号 + 错误/警告码 + 可选消息片段）
//! 2. 编译源码，收集 DiagSink 中的所有诊断（错误和警告）
//! 3. 匹配：注解所在行是否触发了声明的诊断码
//! 4. 报告：通过 / 注解未触发 / 出现未预期诊断

use crate::diag::{Diagnostic, Severity};
use crate::frontend::lexer::{CommentKind, Token, TokenKind};
#[cfg(test)]
use crate::frontend::lexer::tokenize;
use std::collections::HashSet;

/// 测试注解：`--~ E2001 可选的消息片段`
#[derive(Debug, Clone, PartialEq)]
pub struct TestAnnotation {
    /// 注解所在行号（1-based）
    pub line: u32,
    /// 期望的诊断级别（从错误码前缀推断：E=Error, W=Warning, I=Info）
    pub expected_severity: Severity,
    /// 期望的错误码
    pub expected_code: u16,
    /// 可选的消息片段（如果存在，诊断消息必须包含它）
    pub msg_fragment: String,
    /// 注解在源文件中的原始文本（用于错误报告）
    pub raw: String,
}

/// 验证结果
#[derive(Debug, Clone, PartialEq)]
pub enum TestResult {
    /// 所有注解都匹配，没有未预期的诊断
    Pass,
    /// 某个注解声明的错误没有触发
    AnnotationNotTriggered(TestAnnotation),
    /// 出现了未被注解覆盖的诊断
    UnexpectedDiags(Vec<String>),
    /// 注解格式错误
    MalformedAnnotation { line: u32, reason: String },
}

impl TestResult {
    pub fn is_pass(&self) -> bool {
        matches!(self, TestResult::Pass)
    }
}

/// 从源码中提取所有 `--~` 测试注解。
///
/// 注解格式：`--~ ENNNN [消息片段]` 或 `--~ WNNNN [消息片段]`
/// - `ENNNN` 是 4 位错误码（可选 'E' 前缀）
/// - `WNNNN` 是 4 位警告码（可选 'W' 前缀）
/// - 消息片段是可选的，如果存在，诊断消息必须包含它
pub fn extract_test_annotations(source: &str, tokens: &[Token]) -> Result<Vec<TestAnnotation>, TestResult> {
    let mut annotations = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    for token in tokens {
        if let TokenKind::Comment(CommentKind::TestAnnot, content) = &token.kind {
            // 计算行号（1-based）
            let line = count_lines_before(&source[..token.span.lo as usize]) + 1;

            // 解析注解内容：`E2001 字符串字面量不能跨行`
            let content = content.trim();

            // 第一个 token 必须是错误码
            let parts: Vec<&str> = content.splitn(2, char::is_whitespace).collect();
            if parts.is_empty() || parts[0].is_empty() {
                return Err(TestResult::MalformedAnnotation {
                    line,
                    reason: format!("测试注解缺少错误码：`--~ {}`", content),
                });
            }

            let code_str = parts[0];

            // 从错误码前缀推断严重级别
            let expected_severity = if code_str.starts_with('E') {
                Severity::Error
            } else if code_str.starts_with('W') {
                Severity::Warning
            } else if code_str.starts_with('I') {
                Severity::Note
            } else {
                return Err(TestResult::MalformedAnnotation {
                    line,
                    reason: format!("错误码 `{}` 必须以 E/W/I 开头", code_str),
                });
            };

            let expected_code = parse_error_code(code_str).ok_or_else(|| {
                TestResult::MalformedAnnotation {
                    line,
                    reason: format!("无法解析错误码 `{}`", code_str),
                }
            })?;

            let msg_fragment = if parts.len() > 1 {
                parts[1].trim().to_string()
            } else {
                String::new()
            };

            let raw = if line <= lines.len() as u32 {
                lines[line as usize - 1].to_string()
            } else {
                String::new()
            };

            annotations.push(TestAnnotation {
                line,
                expected_severity,
                expected_code,
                msg_fragment,
                raw,
            });
        }
    }

    Ok(annotations)
}

/// 验证诊断是否匹配测试注解。
///
/// 匹配规则：
/// - 注解行号 == 诊断 primary location 的行号
/// - 注解错误码 == 诊断错误码
/// - 如果注解包含消息片段，诊断消息必须包含该片段
pub fn verify_test_annotations(
    source: &str,
    tokens: &[Token],
    diags: &[Diagnostic],
) -> TestResult {
    let annotations = match extract_test_annotations(source, tokens) {
        Ok(a) => a,
        Err(e) => return e,
    };

    if annotations.is_empty() && diags.is_empty() {
        return TestResult::Pass;
    }

    let mut matched_diag_indices = HashSet::new();

    // 检查每个注解是否有匹配的诊断
    for annot in &annotations {
        let mut found = false;

        for (idx, diag) in diags.iter().enumerate() {
            // 匹配严重级别
            if diag.severity != annot.expected_severity {
                continue;
            }

            // 获取诊断的行号
            let diag_line = if let Some(span) = match diag.loc {
                crate::diag::DiagLoc::At(s) => Some(s),
                _ => None,
            } {
                count_lines_before(&source[..span.lo as usize]) + 1
            } else {
                continue;
            };

            // 匹配条件：行号 + 错误码 + 可选消息片段
            let line_matches = diag_line == annot.line;
            let code_matches = diag.code == annot.expected_code;
            let msg_matches = annot.msg_fragment.is_empty()
                || diag.msg.contains(&annot.msg_fragment);

            if line_matches && code_matches && msg_matches {
                found = true;
                matched_diag_indices.insert(idx);
                break;
            }
        }

        if !found {
            return TestResult::AnnotationNotTriggered(annot.clone());
        }
    }

    // 检查是否有未被注解覆盖的诊断
    let uncovered: Vec<String> = diags
        .iter()
        .enumerate()
        .filter(|(idx, _diag)| !matched_diag_indices.contains(idx))
        .map(|(_, diag)| {
            let prefix = match diag.severity {
                Severity::Error => "E",
                Severity::Warning => "W",
                Severity::Note => "I",
                Severity::Help => "H",
            };
            format!("{}{:04}: {}", prefix, diag.code, diag.msg)
        })
        .collect();

    if uncovered.is_empty() {
        TestResult::Pass
    } else {
        TestResult::UnexpectedDiags(uncovered)
    }
}

/// 解析诊断码：接受 `E2001`、`W3001`、`I8888` 和纯数字 `2001` 形式
fn parse_error_code(s: &str) -> Option<u16> {
    let digits = s.strip_prefix('E')
        .or_else(|| s.strip_prefix('e'))
        .or_else(|| s.strip_prefix('W'))
        .or_else(|| s.strip_prefix('w'))
        .or_else(|| s.strip_prefix('I'))
        .or_else(|| s.strip_prefix('i'))
        .unwrap_or(s);

    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    digits.parse::<u16>().ok()
}

/// 计算给定位置之前的换行符数量（即行号，0-based）
fn count_lines_before(text: &str) -> u32 {
    text.bytes().filter(|&b| b == b'\n').count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{DiagLoc, DiagSink, FileId, Span};

    #[test]
    fn parse_error_code_accepts_both_forms() {
        assert_eq!(parse_error_code("E2001"), Some(2001));
        assert_eq!(parse_error_code("2001"), Some(2001));
        assert_eq!(parse_error_code("e2001"), Some(2001));
        assert_eq!(parse_error_code("W3001"), Some(3001));
        assert_eq!(parse_error_code("w3001"), Some(3001));
        assert_eq!(parse_error_code("I8888"), Some(8888));
        assert_eq!(parse_error_code("i8888"), Some(8888));
        assert_eq!(parse_error_code("E"), None);
        assert_eq!(parse_error_code("abc"), None);
        assert_eq!(parse_error_code(""), None);
    }

    #[test]
    fn count_lines_works() {
        assert_eq!(count_lines_before(""), 0);
        assert_eq!(count_lines_before("abc"), 0);
        assert_eq!(count_lines_before("abc\n"), 1);
        assert_eq!(count_lines_before("abc\ndef\n"), 2);
    }

    #[test]
    fn extract_simple_annotation() {
        let source = "x := \"unclosed\n--~ E2001";
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut sink);

        let annots = extract_test_annotations(source, &tokens).unwrap();
        assert_eq!(annots.len(), 1);
        assert_eq!(annots[0].line, 2);
        assert_eq!(annots[0].expected_code, 2001);
        assert_eq!(annots[0].msg_fragment, "");
    }

    #[test]
    fn extract_annotation_with_message() {
        let source = "x := \"unclosed\n--~ E2001 字符串字面量";
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut sink);

        let annots = extract_test_annotations(source, &tokens).unwrap();
        assert_eq!(annots.len(), 1);
        assert_eq!(annots[0].expected_code, 2001);
        assert_eq!(annots[0].msg_fragment, "字符串字面量");
    }

    #[test]
    fn malformed_annotation_without_code() {
        let source = r#"x := 1  --~"#;
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut sink);

        let result = extract_test_annotations(source, &tokens);
        assert!(matches!(result, Err(TestResult::MalformedAnnotation { .. })));
    }

    #[test]
    fn verification_passes_when_annotation_matches() {
        let source = "x := \"unclosed\n--~ E2001";
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut sink);

        // 模拟诊断：在第 1 行（字符串起始位置）产生 E2001
        // 注解在第 2 行，但诊断指向第 1 行的错误位置
        let diag = Diagnostic::error(
            2001,
            "字符串字面量不能跨行",
            DiagLoc::At(Span::new(FileId(0), 15, 16)),  // 第 2 行的第一个字符
        );
        let diags = vec![diag];

        let result = verify_test_annotations(source, &tokens, &diags);
        assert!(result.is_pass());
    }

    #[test]
    fn verification_fails_when_annotation_not_triggered() {
        let source = r#"x := 1  --~ E2001"#;
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut sink);

        // 没有诊断
        let diags = vec![];

        let result = verify_test_annotations(source, &tokens, &diags);
        assert!(matches!(result, TestResult::AnnotationNotTriggered(_)));
    }

    #[test]
    fn verification_fails_on_unexpected_diag() {
        let source = r#"x := 1"#;
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut sink);

        // 有诊断但没有注解
        let diag = Diagnostic::error(
            2001,
            "意外错误",
            DiagLoc::At(Span::new(FileId(0), 0, 1)),
        );
        let diags = vec![diag];

        let result = verify_test_annotations(source, &tokens, &diags);
        assert!(matches!(result, TestResult::UnexpectedDiags(_)));
    }

    #[test]
    fn verification_matches_with_message_fragment() {
        let source = "x := \"unclosed\n--~ E2001 字符串";
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut sink);

        // 诊断消息包含"字符串"，定位在第 2 行
        let diag = Diagnostic::error(
            2001,
            "字符串字面量不能跨行",
            DiagLoc::At(Span::new(FileId(0), 15, 16)),
        );
        let diags = vec![diag];

        let result = verify_test_annotations(source, &tokens, &diags);
        assert!(result.is_pass());
    }

    #[test]
    fn verification_fails_when_message_fragment_not_found() {
        let source = "x := \"unclosed\n--~ E2001 数字";
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut sink);

        // 诊断消息不包含"数字"
        let diag = Diagnostic::error(
            2001,
            "字符串字面量不能跨行",
            DiagLoc::At(Span::new(FileId(0), 15, 16)),
        );
        let diags = vec![diag];

        let result = verify_test_annotations(source, &tokens, &diags);
        assert!(matches!(result, TestResult::AnnotationNotTriggered(_)));
    }
}
