//! 语法分析器的游标与基础设施。
//!
//! 记号流里的注释不删：`---`/`--!` 要附着到 AST 节点，`--~`/`--=` 由树外
//! runner 消费。所以游标在推进时跳过注释，而不是词法阶段丢掉它们。

use crate::diag::{DiagLoc, DiagSink, Diagnostic, FileId, Span};
use crate::frontend::lexer::{Token, TokenKind};

/// 期望某个记号但没等到。ADR 009 的语法族错误码段。
const E_UNEXPECTED: u16 = 2001;

pub struct Parser<'a> {
    toks: Vec<Token>,
    pos: usize,
    file: FileId,
    sink: &'a mut DiagSink,
}

impl<'a> Parser<'a> {
    pub fn new(file: FileId, toks: Vec<Token>, sink: &'a mut DiagSink) -> Self {
        let mut p = Parser { toks, pos: 0, file, sink };
        p.skip_comments();
        p
    }

    pub fn file(&self) -> FileId {
        self.file
    }

    /// 获取当前 parser 位置（用于检测是否推进）。
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// 当前记号。记号流末尾必为 `Eof`，所以这里不会越界。
    pub fn peek(&self) -> &TokenKind {
        &self.toks[self.pos.min(self.toks.len() - 1)].kind
    }

    pub fn peek_span(&self) -> Span {
        self.toks[self.pos.min(self.toks.len() - 1)].span
    }

    /// 向前看 n 个记号（跳过注释）。
    pub fn peek_ahead(&self, n: usize) -> &TokenKind {
        let mut idx = self.pos;
        let mut count = 0;
        while idx < self.toks.len() && count < n {
            idx += 1;
            // 跳过注释
            while idx < self.toks.len() && self.toks[idx].is_comment() {
                idx += 1;
            }
            count += 1;
        }
        &self.toks[idx.min(self.toks.len() - 1)].kind
    }

    pub fn at_eof(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    /// 吃掉当前记号并返回它的位置。
    pub fn bump(&mut self) -> Span {
        let s = self.peek_span();
        if !self.at_eof() {
            self.pos += 1;
            self.skip_comments();
        }
        s
    }

    /// 当前是这个记号就吃掉，返回是否吃到。
    pub fn eat_punct(&mut self, p: &str) -> bool {
        if matches!(self.peek(), TokenKind::Punct(x) if *x == p) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub fn eat_keyword(&mut self, k: &str) -> bool {
        if matches!(self.peek(), TokenKind::Keyword(x) if *x == k) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// 必须是这个记号，否则报错并原地不动。返回是否满足。
    pub fn expect_punct(&mut self, p: &str) -> bool {
        if self.eat_punct(p) {
            return true;
        }
        self.unexpected(p);
        false
    }

    /// 吃掉一个标识符，返回它的名字。
    pub fn eat_ident(&mut self) -> Option<(String, Span)> {
        if let TokenKind::Ident(name) = self.peek() {
            let name = name.clone();
            let s = self.bump();
            Some((name, s))
        } else {
            None
        }
    }

    pub fn unexpected(&mut self, expected: &str) {
        let span = self.peek_span();
        let got = describe(self.peek());
        self.sink.emit(Diagnostic::error(
            E_UNEXPECTED,
            format!("期望 `{expected}`，遇到 {got}"),
            DiagLoc::At(span),
        ));
    }

    /// 报错后跳到下一个可能的项起点，避免一处语法错误引出一串级联诊断。
    pub fn recover_to_item(&mut self) {
        while !self.at_eof() {
            if matches!(self.peek(), TokenKind::Punct("::")) {
                return;
            }
            self.bump();
        }
    }

    fn skip_comments(&mut self) {
        while self.pos < self.toks.len() && self.toks[self.pos].is_comment() {
            self.pos += 1;
        }
    }
}

fn describe(k: &TokenKind) -> String {
    match k {
        TokenKind::Ident(n) => format!("标识符 `{n}`"),
        TokenKind::Keyword(k) => format!("关键字 `{k}`"),
        TokenKind::Int(v) | TokenKind::Float(v) => format!("字面量 `{v}`"),
        TokenKind::Str(_) => "字符串字面量".into(),
        TokenKind::Punct(p) => format!("`{p}`"),
        TokenKind::Comment(_, _) => "注释".into(),
        TokenKind::Eof => "文件结束".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::lexer::CommentKind;

    fn tok(kind: TokenKind) -> Token {
        Token::new(kind, Span::new(FileId(0), 0, 1))
    }

    fn eof() -> Token {
        Token::new(TokenKind::Eof, Span::new(FileId(0), 9, 9))
    }

    #[test]
    fn comments_are_skipped_by_the_cursor() {
        let mut sink = DiagSink::new();
        let toks = vec![
            tok(TokenKind::Comment(CommentKind::ItemDoc, "文档".into())),
            tok(TokenKind::Ident("main".into())),
            eof(),
        ];
        let mut p = Parser::new(FileId(0), toks, &mut sink);
        assert_eq!(p.eat_ident().map(|(n, _)| n), Some("main".to_string()));
        assert!(p.at_eof());
    }

    #[test]
    fn bump_stops_at_eof() {
        let mut sink = DiagSink::new();
        let mut p = Parser::new(FileId(0), vec![eof()], &mut sink);
        for _ in 0..3 {
            p.bump();
        }
        assert!(p.at_eof());
    }

    #[test]
    fn expect_reports_and_does_not_consume() {
        let mut sink = DiagSink::new();
        let toks = vec![tok(TokenKind::Punct("?")), eof()];
        let mut p = Parser::new(FileId(0), toks, &mut sink);
        assert!(!p.expect_punct("::"));
        // 没吃掉，恢复逻辑才有东西可看。
        assert!(matches!(p.peek(), TokenKind::Punct("?")));
        drop(p);
        assert!(sink.has_errors());
    }

    #[test]
    fn recovery_lands_on_the_next_binding() {
        let mut sink = DiagSink::new();
        let toks = vec![
            tok(TokenKind::Punct("?")),
            tok(TokenKind::Ident("f".into())),
            tok(TokenKind::Punct("::")),
            eof(),
        ];
        let mut p = Parser::new(FileId(0), toks, &mut sink);
        p.recover_to_item();
        assert!(matches!(p.peek(), TokenKind::Punct("::")));
    }
}
