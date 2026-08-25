//! 语法分析层。记号流 → AST。
//!
//! 分四个文件：游标与恢复在 `parser`，其余三个按项/语句/表达式切。切分依据是
//! ADR 007 的模块划分，而不是文件长度。

pub mod decl;
pub mod expr;
mod parser_impl;
pub mod stmt;

pub use parser_impl::Parser;

use crate::diag::{DiagSink, FileId};
use crate::frontend::ast::Module;
use crate::frontend::lexer::Token;

/// 语法分析入口。
///
/// 尚未实现：吃空记号流，返回没有项的模块。不报诊断——「没认出任何项」在
/// 分析器写完之前不是用户的错。
pub fn parse(file: FileId, toks: Vec<Token>, sink: &mut DiagSink) -> Module {
    let mut p = Parser::new(file, toks, sink);
    let span = p.peek_span();
    let mut items = Vec::new();
    while !p.at_eof() {
        match decl::parse_item(&mut p) {
            Some(it) => items.push(it),
            // 认不出就往前走一格，否则空实现会在这里死循环。
            None => {
                p.bump();
            }
        }
    }
    Module { items, span }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::Span;
    use crate::frontend::lexer::TokenKind;

    #[test]
    fn empty_stream_parses_to_an_empty_module() {
        let mut sink = DiagSink::new();
        let toks = vec![Token::new(TokenKind::Eof, Span::new(FileId(0), 0, 0))];
        let m = parse(FileId(0), toks, &mut sink);
        assert!(m.items.is_empty());
        assert!(!sink.has_errors());
    }

    #[test]
    fn unrecognized_tokens_do_not_hang() {
        let mut sink = DiagSink::new();
        let s = Span::new(FileId(0), 0, 1);
        let toks = vec![
            Token::new(TokenKind::Ident("main".into()), s),
            Token::new(TokenKind::Punct("::"), s),
            Token::new(TokenKind::Eof, Span::new(FileId(0), 6, 6)),
        ];
        let m = parse(FileId(0), toks, &mut sink);
        assert!(m.items.is_empty());
    }
}
