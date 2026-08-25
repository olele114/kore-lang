//! 词法分析层。源文本 → 记号流。

mod keywords;
mod lexer_impl;
pub mod token;

pub use keywords::{KEYWORDS, is_keyword};
pub use lexer_impl::{classify_comment, tokenize};
pub use token::{CommentKind, Token, TokenKind};
