//! 契约断言检查点。
//!
//! 契约断言（`--=`）声明机器级语义保证，由编译器在对应 pass 验证。
//! 例如：
//! - `--= tailcall f` 验证尾调用降级为跳转而非调用
//! - `--= volatile-load u32` 验证 volatile 访问未被优化消除
//!
//! 与测试注解（`--~`）的区别：
//! - `--~` 由外部 runner 验证，编译器视为注释
//! - `--=` 由编译器内部验证，在各 pass 检查点检查
//!
//! ## 设计原则（ADR 010 Q2）
//!
//! 1. **无法识别的断言种类必须报错**，不能静默通过
//! 2. **检查点在编译器内部**，不依赖外部工具
//! 3. **不比对汇编文本**，只检查语义保证
//!
//! ## 当前实现状态
//!
//! Stage0 还没有后端，所以只实现基础框架：
//! - 从 token 流提取契约断言
//! - 报告无法识别的断言种类
//! - 为未来的检查点预留接口

use crate::diag::{DiagSink, Diagnostic, DiagLoc};
use crate::frontend::lexer::{CommentKind, Token, TokenKind};
use std::collections::HashMap;

/// 契约断言。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractAssertion {
    /// 断言所在行号（1-based）
    pub line: u32,
    /// 断言种类（如 "tailcall"、"volatile-load"）
    pub kind: String,
    /// 断言参数（如 "f"、"u32"）
    pub args: Vec<String>,
    /// 原始文本（用于诊断）
    pub raw: String,
}

/// 从 token 流中提取契约断言。
///
/// 只提取 `CommentKind::Contract` 的 token，解析其内容为断言结构。
pub fn extract_contract_assertions(
    source: &str,
    tokens: &[Token],
) -> Result<Vec<ContractAssertion>, Vec<Diagnostic>> {
    let mut assertions = Vec::new();
    let mut errors = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    for token in tokens {
        if let TokenKind::Comment(CommentKind::Contract, body) = &token.kind {
            // 计算行号
            let line = count_lines_before(&source[..token.span.lo as usize]) + 1;

            let raw = if line <= lines.len() as u32 {
                lines[line as usize - 1].to_string()
            } else {
                String::new()
            };

            // 解析断言内容："kind arg1 arg2 ..."
            let parts: Vec<&str> = body.split_whitespace().collect();

            if parts.is_empty() {
                errors.push(Diagnostic::error(
                    9001,
                    "契约断言不能为空",
                    DiagLoc::At(token.span),
                ));
                continue;
            }

            let kind = parts[0].to_string();
            let args = parts[1..].iter().map(|s| s.to_string()).collect();

            assertions.push(ContractAssertion {
                line,
                kind,
                args,
                raw,
            });
        }
    }

    if errors.is_empty() {
        Ok(assertions)
    } else {
        Err(errors)
    }
}

/// 计算文本中换行符的数量。
fn count_lines_before(text: &str) -> u32 {
    text.bytes().filter(|&b| b == b'\n').count() as u32
}

/// 验证契约断言。
///
/// 当前 stage0 还没有后端，所以只做基础验证：
/// - 检查断言种类是否被识别
/// - 报告无法识别的断言种类
///
/// ## 参数
///
/// - `assertions`: 待验证的断言列表
/// - `sink`: 诊断接收器
///
/// ## 返回
///
/// 未识别的断言数量。如果返回 0，说明所有断言都通过验证。
pub fn verify_contract_assertions(
    assertions: &[ContractAssertion],
    sink: &mut DiagSink,
) -> usize {
    let mut unrecognized = 0;

    for assertion in assertions {
        if !is_recognized_assertion_kind(&assertion.kind) {
            sink.emit(Diagnostic::error(
                9002,
                format!("无法识别的契约断言种类: {}", assertion.kind),
                DiagLoc::None,
            ));
            unrecognized += 1;
        }
    }

    unrecognized
}

/// 检查断言种类是否被识别。
///
/// 当前实现只列出 ADR 010 中定义的两种断言：
/// - `tailcall`: 尾调用优化
/// - `volatile-load`: volatile 加载
/// - `volatile-store`: volatile 存储
///
/// 未来扩展：其他断言种类在此添加。
fn is_recognized_assertion_kind(kind: &str) -> bool {
    matches!(kind, "tailcall" | "volatile-load" | "volatile-store")
}

/// 验证函数类型。
type VerifierFn = Box<dyn Fn(&ContractAssertion) -> bool>;

/// 检查点接口。
///
/// 各 pass 通过此接口注册检查点，在对应位置验证断言。
/// 当前为预留接口，等后端实现后再填充。
pub struct CheckpointRegistry {
    checkpoints: HashMap<String, VerifierFn>,
}

impl CheckpointRegistry {
    pub fn new() -> Self {
        Self {
            checkpoints: HashMap::new(),
        }
    }

    /// 注册检查点。
    ///
    /// 当后端实现后，各 pass 调用此方法注册验证函数。
    #[allow(dead_code)]
    pub fn register<F>(&mut self, kind: &str, checker: F)
    where
        F: Fn(&ContractAssertion) -> bool + 'static,
    {
        self.checkpoints.insert(kind.to_string(), Box::new(checker));
    }

    /// 执行检查点验证。
    #[allow(dead_code)]
    pub fn verify(&self, assertion: &ContractAssertion) -> bool {
        if let Some(checker) = self.checkpoints.get(&assertion.kind) {
            checker(assertion)
        } else {
            false
        }
    }
}

impl Default for CheckpointRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::FileId;
    use crate::frontend::lexer::tokenize;

    #[test]
    fn extract_tailcall_assertion() {
        let source = "f :: () void => g()  --= tailcall g";
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut sink);

        let result = extract_contract_assertions(source, &tokens);
        assert!(result.is_ok());

        let assertions = result.unwrap();
        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].kind, "tailcall");
        assert_eq!(assertions[0].args, vec!["g"]);
    }

    #[test]
    fn extract_volatile_load_assertion() {
        let source = "x := load(ptr)  --= volatile-load u32";
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut sink);

        let result = extract_contract_assertions(source, &tokens);
        assert!(result.is_ok());

        let assertions = result.unwrap();
        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].kind, "volatile-load");
        assert_eq!(assertions[0].args, vec!["u32"]);
    }

    #[test]
    fn empty_assertion_produces_error() {
        let source = "x := 42  --=";
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut sink);

        let result = extract_contract_assertions(source, &tokens);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, 9001);
    }

    #[test]
    fn unrecognized_assertion_kind_reports_error() {
        let source = "x := 42  --= unknown-kind";
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), source, &mut sink);

        let assertions = extract_contract_assertions(source, &tokens).unwrap();
        let unrecognized = verify_contract_assertions(&assertions, &mut sink);

        assert_eq!(unrecognized, 1);
        assert_eq!(sink.err_count(), 1);
    }

    #[test]
    fn recognized_assertion_kinds_pass() {
        let mut sink = DiagSink::new();

        let assertions = vec![
            ContractAssertion {
                line: 1,
                kind: "tailcall".to_string(),
                args: vec!["f".to_string()],
                raw: "--= tailcall f".to_string(),
            },
            ContractAssertion {
                line: 2,
                kind: "volatile-load".to_string(),
                args: vec!["u32".to_string()],
                raw: "--= volatile-load u32".to_string(),
            },
            ContractAssertion {
                line: 3,
                kind: "volatile-store".to_string(),
                args: vec!["u64".to_string()],
                raw: "--= volatile-store u64".to_string(),
            },
        ];

        let unrecognized = verify_contract_assertions(&assertions, &mut sink);
        assert_eq!(unrecognized, 0);
        assert_eq!(sink.err_count(), 0);
    }
}
