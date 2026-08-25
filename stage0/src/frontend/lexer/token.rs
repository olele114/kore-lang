//! 记号种类。只覆盖 Kore0 子集需要的形状（ADR 007 Q22）。

use crate::diag::Span;

/// 注释的五种形式，全靠 `--` 之后的第三个字符区分（ADR 008 第 5 节 +
/// ADR 010 Q3）。
///
/// 词法层保留全部五种而不是丢掉：`---` 与 `--!` 要附着到 AST 节点，
/// `--~` 与 `--=` 由树外 runner 与后端检查点消费。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentKind {
    /// `--` 普通行注释。
    Line,
    /// `---` 项文档注释，描述紧随其后的那一项。
    ItemDoc,
    /// `--!` 模块级文档注释，只允许出现在文件顶部。
    ModuleDoc,
    /// `--~` 测试注解，声明该行期望的诊断。
    TestAnnot,
    /// `--=` 契约断言，声明一条机器级语义保证。
    Contract,
}

/// 记号种类。
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// 标识符。
    Ident(String),
    /// 关键字，取值必为 `keywords::KEYWORDS` 中的一项。
    Keyword(&'static str),
    /// 整数字面量，保留原文而非提前求值：进制与下划线的诊断要指回原文。
    Int(String),
    /// 浮点字面量，同样保留原文。
    Float(String),
    /// 字符串字面量，已解转义。
    Str(String),
    /// 记号（`::`、`:=`、`?`、`@`、`^`、`~`、`!` 等）。
    Punct(&'static str),
    /// 注释。
    Comment(CommentKind, String),
    /// 文件结束。
    Eof,
}

/// 一个记号 = 种类 + 位置。位置用 `Span` 而非行列：ADR 009 定了 `Span` 是
/// 12 字节的 `(file, lo, hi)`，行列在渲染时才算。
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Token { kind, span }
    }

    /// 注释在语法分析里要跳过，但不能在词法阶段丢掉。
    pub fn is_comment(&self) -> bool {
        matches!(self.kind, TokenKind::Comment(_, _))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::FileId;

    #[test]
    fn comment_tokens_are_flagged() {
        let span = Span::new(FileId(0), 0, 2);
        let c = Token::new(
            TokenKind::Comment(CommentKind::Line, String::new()),
            span,
        );
        assert!(c.is_comment());
        assert!(!Token::new(TokenKind::Eof, span).is_comment());
    }
}
