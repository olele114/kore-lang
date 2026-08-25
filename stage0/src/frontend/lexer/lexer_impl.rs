//! 词法分析。stage0 只需要认出 Kore0 子集用到的记号。
//!
//! 目前只有注释分类是完整实现——它是唯一一处「记号阶梯」逻辑，五种形式
//! 的判定错了会让测试注解与契约断言静默失效，所以先把它钉死。

use super::keywords;
use super::token::{CommentKind, Token, TokenKind};
use crate::diag::{DiagSink, FileId, Span};

/// 注释起始记号。五种形式共用这个前缀。
const COMMENT_PREFIX_DASH: &str = "--";
const COMMENT_PREFIX_SLASH: &str = "//";

/// 把一段以 `--` 或 `//` 开头的文本分类，返回形式与去掉记号后的正文。
///
/// 判定只看前缀之后的第三个字符（ADR 008:432、ADR 010 Q3）。`text` 应为
/// 从前缀开始到行尾的切片，不含换行。
pub fn classify_comment(text: &str) -> (CommentKind, &str) {
    let prefix_len = if text.starts_with(COMMENT_PREFIX_DASH) {
        COMMENT_PREFIX_DASH.len()
    } else if text.starts_with(COMMENT_PREFIX_SLASH) {
        COMMENT_PREFIX_SLASH.len()
    } else {
        debug_assert!(false, "注释必须以 -- 或 // 开头");
        return (CommentKind::Line, text);
    };

    let rest = &text[prefix_len..];
    let bytes = rest.as_bytes();

    match bytes.first() {
        // 前缀后直接到行尾：普通行注释。
        None => (CommentKind::Line, ""),
        Some(b'-') | Some(b'/') => {
            // `----` 或 `////` 及更长的横线是视觉分隔线，不是项文档。ADR 008 说 `--!`
            // 借用 Rust `//!` 的语义，那就一并沿用 Rust 对 `////` 的处理：
            // 多一根横线即退回普通注释，否则一条分隔线会把文档挂到下一项上。
            if bytes.get(1) == Some(&b'-') || bytes.get(1) == Some(&b'/') {
                (CommentKind::Line, trim_body(rest))
            } else {
                (CommentKind::ItemDoc, trim_body(&rest[1..]))
            }
        }
        Some(b'!') => (CommentKind::ModuleDoc, trim_body(&rest[1..])),
        Some(b'~') => (CommentKind::TestAnnot, trim_body(&rest[1..])),
        // `--=` 或 `//=` 多一层判定：只有其后紧跟空白（或直接到行尾）才是契约断言
        // （ADR 010:325）。`--===` 或 `//===` 之类的分隔线必须退回普通行注释，否则
        // 检查点会对着 `== ...` 报「未识别断言种类」——那一行明令禁止。
        //
        // 这层判定只加在 `=` 上，不加在 `!` 与 `~` 上：ADR 010 只对 `--=`
        // 提了这条，而 `=` 是唯一会被拿去画分隔线的字符。
        Some(b'=') => {
            if is_body_boundary(bytes.get(1)) {
                (CommentKind::Contract, trim_body(&rest[1..]))
            } else {
                (CommentKind::Line, trim_body(rest))
            }
        }
        // 第三个字符是别的东西（含空格）：普通行注释。
        Some(_) => (CommentKind::Line, trim_body(rest)),
    }
}

/// 去掉正文前导空白。记号与正文之间的一个空格是书写习惯，不是内容。
fn trim_body(s: &str) -> &str {
    s.trim_start_matches(' ')
}

/// 记号与正文的边界：空白，或行尾。用于把 `--= tailcall f` 与 `--===` 分开。
fn is_body_boundary(next: Option<&u8>) -> bool {
    match next {
        None => true,
        Some(b) => b.is_ascii_whitespace(),
    }
}

/// 词法分析入口。
pub fn tokenize(file: FileId, src: &str, sink: &mut DiagSink) -> Vec<Token> {
    let lexer = Lexer::new(file, src, sink);
    lexer.run()
}

struct Lexer<'a> {
    file: FileId,
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    sink: &'a mut DiagSink,
    tokens: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn new(file: FileId, src: &'a str, sink: &'a mut DiagSink) -> Self {
        Lexer {
            file,
            src,
            bytes: src.as_bytes(),
            pos: 0,
            sink,
            tokens: Vec::new(),
        }
    }

    fn run(mut self) -> Vec<Token> {
        while !self.is_eof() {
            let start = self.pos;
            match self.cur() {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    self.skip_whitespace();
                }
                b'-' if self.peek() == Some(b'-') => {
                    self.lex_comment(start);
                }
                b'/' if self.peek() == Some(b'/') => {
                    self.lex_comment(start);
                }
                b'_' | b'a'..=b'z' | b'A'..=b'Z' => {
                    self.lex_ident_or_keyword(start);
                }
                b'0'..=b'9' => {
                    self.lex_number(start);
                }
                b'"' => {
                    self.lex_string(start);
                }
                b'`' => {
                    self.lex_raw_string(start);
                }
                b'\'' => {
                    self.lex_char(start);
                }
                _ => {
                    self.lex_punct(start);
                }
            }
        }
        let end = self.src.len() as u32;
        self.tokens
            .push(Token::new(TokenKind::Eof, Span::new(self.file, end, end)));
        self.tokens
    }

    fn cur(&self) -> u8 {
        self.bytes[self.pos]
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    #[allow(dead_code)]
    fn advance(&mut self) -> u8 {
        let b = self.bytes[self.pos];
        self.pos += 1;
        b
    }

    fn skip_whitespace(&mut self) {
        while !self.is_eof() && matches!(self.cur(), b' ' | b'\t' | b'\r' | b'\n') {
            self.pos += 1;
        }
    }

    fn lex_comment(&mut self, start: usize) {
        debug_assert!(
            (self.cur() == b'-' && self.peek() == Some(b'-')) ||
            (self.cur() == b'/' && self.peek() == Some(b'/'))
        );
        let line_start = start;
        // 注释内容可能包含 UTF-8 字符，按字符迭代而不是字节
        while !self.is_eof() {
            let remaining = &self.src[self.pos..];
            if let Some(ch) = remaining.chars().next() {
                if ch == '\n' {
                    break;
                }
                self.pos += ch.len_utf8();
            } else {
                // 无效 UTF-8，跳过单个字节
                self.pos += 1;
            }
        }
        let text = &self.src[line_start..self.pos];
        let (kind, body) = classify_comment(text);
        let span = Span::new(self.file, start as u32, self.pos as u32);
        self.tokens
            .push(Token::new(TokenKind::Comment(kind, body.into()), span));
    }

    fn lex_ident_or_keyword(&mut self, start: usize) {
        while !self.is_eof() && is_ident_continue(self.cur()) {
            self.pos += 1;
        }
        let word = &self.src[start..self.pos];
        let kind = if is_keyword(word) {
            TokenKind::Keyword(Box::leak(word.to_string().into_boxed_str()))
        } else {
            TokenKind::Ident(word.into())
        };
        let span = Span::new(self.file, start as u32, self.pos as u32);
        self.tokens.push(Token::new(kind, span));
    }

    fn lex_number(&mut self, start: usize) {
        if self.cur() == b'0' && matches!(self.peek(), Some(b'x' | b'o' | b'b')) {
            self.pos += 2;
            while !self.is_eof() && is_hex_digit_or_underscore(self.cur()) {
                self.pos += 1;
            }
        } else {
            while !self.is_eof() && (self.cur().is_ascii_digit() || self.cur() == b'_') {
                self.pos += 1;
            }
            if !self.is_eof() && self.cur() == b'.' && self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
                while !self.is_eof() && (self.cur().is_ascii_digit() || self.cur() == b'_') {
                    self.pos += 1;
                }
            }
        }

        let num_end = self.pos;
        let _suffix_start = self.pos;
        while !self.is_eof() && is_ident_continue(self.cur()) {
            self.pos += 1;
        }

        let num_text = &self.src[start..num_end];
        let has_dot = num_text.contains('.');
        let kind = if has_dot {
            TokenKind::Float(num_text.into())
        } else {
            TokenKind::Int(num_text.into())
        };

        let span = Span::new(self.file, start as u32, self.pos as u32);
        self.tokens.push(Token::new(kind, span));
    }

    fn lex_string(&mut self, start: usize) {
        debug_assert!(self.cur() == b'"');
        self.pos += 1;
        let mut s = String::new();
        while !self.is_eof() {
            // 先检查 UTF-8 字符
            let remaining = &self.src[self.pos..];
            let ch = if let Some(c) = remaining.chars().next() {
                c
            } else {
                // 无效 UTF-8，跳过并继续
                self.pos += 1;
                continue;
            };

            if ch == '"' {
                break;
            }
            if ch == '\n' {
                let span = Span::new(self.file, start as u32, self.pos as u32);
                self.sink.emit(crate::diag::Diagnostic::error(
                    2001,
                    "字符串字面量不能跨行",
                    crate::diag::DiagLoc::At(span),
                ));
                break;
            }
            if ch == '\\' {
                self.pos += ch.len_utf8();
                if self.is_eof() {
                    break;
                }
                let escape_char = match self.cur() {
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    b'0' => '\0',
                    b'\\' => '\\',
                    b'"' => '"',
                    b'\'' => '\'',
                    b'x' => {
                        self.pos += 1;
                        let byte = self.read_hex_byte();
                        byte as char
                    }
                    b'u' if self.peek() == Some(b'{') => {
                        self.pos += 2;
                        let code = self.read_unicode();
                        std::char::from_u32(code).unwrap_or('\u{FFFD}')
                    }
                    _ => {
                        let span = Span::new(self.file, (self.pos - 1) as u32, self.pos as u32);
                        self.sink.emit(crate::diag::Diagnostic::error(
                            2002,
                            "未识别的转义序列",
                            crate::diag::DiagLoc::At(span),
                        ));
                        '?'
                    }
                };
                s.push(escape_char);
                self.pos += 1;
            } else {
                s.push(ch);
                self.pos += ch.len_utf8();
            }
        }
        if !self.is_eof() && self.cur() == b'"' {
            self.pos += 1;
        } else if self.is_eof() {
            // 到达 EOF 仍未闭合。跨行的情况已在循环内报过 E2001，
            // 不再叠加第二条诊断。
            let span = Span::new(self.file, start as u32, self.pos as u32);
            self.sink.emit(crate::diag::Diagnostic::error(
                4002,
                "未闭合的字符串字面量",
                crate::diag::DiagLoc::At(span),
            ));
        }
        let span = Span::new(self.file, start as u32, self.pos as u32);
        self.tokens.push(Token::new(TokenKind::Str(s), span));
    }

    fn lex_raw_string(&mut self, start: usize) {
        debug_assert!(self.cur() == b'`');
        self.pos += 1;
        let content_start = self.pos;
        while !self.is_eof() && self.cur() != b'`' {
            self.pos += 1;
        }
        let s = self.src[content_start..self.pos].to_string();
        if !self.is_eof() {
            self.pos += 1;
        }
        let span = Span::new(self.file, start as u32, self.pos as u32);
        self.tokens.push(Token::new(TokenKind::Str(s), span));
    }

    fn lex_char(&mut self, start: usize) {
        debug_assert!(self.cur() == b'\'');
        self.pos += 1;
        if self.is_eof() {
            let span = Span::new(self.file, start as u32, self.pos as u32);
            self.sink.emit(crate::diag::Diagnostic::error(
                2003,
                "未闭合的字符字面量",
                crate::diag::DiagLoc::At(span),
            ));
            return;
        }

        let byte_val = if self.cur() == b'\\' {
            self.pos += 1;
            if self.is_eof() {
                0
            } else {
                match self.cur() {
                    b'n' => { self.pos += 1; b'\n' }
                    b'r' => { self.pos += 1; b'\r' }
                    b't' => { self.pos += 1; b'\t' }
                    b'0' => { self.pos += 1; 0 }
                    b'\\' => { self.pos += 1; b'\\' }
                    b'\'' => { self.pos += 1; b'\'' }
                    b'"' => { self.pos += 1; b'"' }
                    _ => {
                        let span = Span::new(self.file, (self.pos - 1) as u32, self.pos as u32);
                        self.sink.emit(crate::diag::Diagnostic::error(
                            2002,
                            "未识别的转义序列",
                            crate::diag::DiagLoc::At(span),
                        ));
                        self.pos += 1;
                        b'?'
                    }
                }
            }
        } else {
            let b = self.cur();
            self.pos += 1;
            b
        };

        if !self.is_eof() && self.cur() == b'\'' {
            self.pos += 1;
        }

        let span = Span::new(self.file, start as u32, self.pos as u32);
        self.tokens.push(Token::new(TokenKind::Int(format!("{}", byte_val)), span));
    }

    fn lex_punct(&mut self, start: usize) {
        let first = self.cur();
        let second = self.peek();

        let punct = match (first, second) {
            (b':', Some(b':')) => { self.pos += 2; "::" }
            (b':', Some(b'=')) => { self.pos += 2; ":=" }
            (b':', _) => { self.pos += 1; ":" }
            (b'=', Some(b'>')) => { self.pos += 2; "=>" }
            (b'=', Some(b'=')) => { self.pos += 2; "==" }
            (b'=', _) => { self.pos += 1; "=" }
            (b'!', Some(b'=')) => { self.pos += 2; "!=" }
            (b'!', _) => { self.pos += 1; "!" }
            (b'<', Some(b'=')) => { self.pos += 2; "<=" }
            (b'<', Some(b'<')) => { self.pos += 2; "<<" }
            (b'<', _) => { self.pos += 1; "<" }
            (b'>', Some(b'=')) => { self.pos += 2; ">=" }
            (b'>', Some(b'>')) => { self.pos += 2; ">>" }
            (b'>', _) => { self.pos += 1; ">" }
            (b'&', Some(b'&')) => { self.pos += 2; "&&" }
            (b'&', _) => { self.pos += 1; "&" }
            (b'|', Some(b'|')) => { self.pos += 2; "||" }
            (b'|', _) => { self.pos += 1; "|" }
            (b'+', _) => { self.pos += 1; "+" }
            (b'-', _) => { self.pos += 1; "-" }
            (b'*', _) => { self.pos += 1; "*" }
            (b'/', _) => { self.pos += 1; "/" }
            (b'%', _) => { self.pos += 1; "%" }
            (b'~', _) => { self.pos += 1; "~" }
            (b'^', _) => { self.pos += 1; "^" }
            (b'?', _) => { self.pos += 1; "?" }
            (b'@', _) => { self.pos += 1; "@" }
            (b'(', _) => { self.pos += 1; "(" }
            (b')', _) => { self.pos += 1; ")" }
            (b'{', _) => { self.pos += 1; "{" }
            (b'}', _) => { self.pos += 1; "}" }
            (b'[', _) => { self.pos += 1; "[" }
            (b']', _) => { self.pos += 1; "]" }
            (b',', _) => { self.pos += 1; "," }
            (b';', _) => { self.pos += 1; ";" }
            (b'.', _) => { self.pos += 1; "." }
            (ch, _) => {
                self.pos += 1;
                let span = Span::new(self.file, start as u32, self.pos as u32);
                self.sink.emit(crate::diag::Diagnostic::error(
                    2004,
                    format!("未识别的字符: {}", ch as char),
                    crate::diag::DiagLoc::At(span),
                ));
                return;
            }
        };
        let span = Span::new(self.file, start as u32, self.pos as u32);
        self.tokens.push(Token::new(TokenKind::Punct(punct), span));
    }

    fn read_hex_byte(&mut self) -> u8 {
        let mut val = 0u8;
        for _ in 0..2 {
            if self.is_eof() {
                break;
            }
            val = val * 16 + hex_value(self.cur());
            self.pos += 1;
        }
        val
    }

    fn read_unicode(&mut self) -> u32 {
        let mut val = 0u32;
        while !self.is_eof() && self.cur() != b'}' {
            val = val * 16 + hex_value(self.cur()) as u32;
            self.pos += 1;
        }
        if !self.is_eof() && self.cur() == b'}' {
            self.pos += 1;
        }
        val
    }
}

fn is_ident_continue(b: u8) -> bool {
    matches!(b, b'_' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9')
}

fn is_hex_digit_or_underscore(b: u8) -> bool {
    matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F' | b'_')
}

fn hex_value(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

/// 判定标识符形状的词是否为关键字。转发给 `keywords`，让 `parser` 只依赖
/// `lexer` 一个模块。
pub fn is_keyword(word: &str) -> bool {
    keywords::is_keyword(word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_five_sigils_are_distinguished() {
        assert_eq!(classify_comment("-- 普通注释").0, CommentKind::Line);
        assert_eq!(classify_comment("--- 项文档").0, CommentKind::ItemDoc);
        assert_eq!(classify_comment("--! 模块文档").0, CommentKind::ModuleDoc);
        assert_eq!(classify_comment("--~ E4001 类型不匹配").0, CommentKind::TestAnnot);
        assert_eq!(classify_comment("--= tailcall count").0, CommentKind::Contract);
    }

    #[test]
    fn bodies_drop_the_sigil_and_leading_space() {
        assert_eq!(classify_comment("--~ E4001 类型不匹配").1, "E4001 类型不匹配");
        assert_eq!(classify_comment("--= volatile-load u32").1, "volatile-load u32");
        assert_eq!(classify_comment("--- 描述下一项").1, "描述下一项");
    }

    #[test]
    fn bare_dashes_at_end_of_line_are_line_comments() {
        assert_eq!(classify_comment("--"), (CommentKind::Line, ""));
    }

    #[test]
    fn four_dashes_are_a_divider_not_item_doc() {
        // 分隔线不该把文档挂到下一项上。
        assert_eq!(classify_comment("----").0, CommentKind::Line);
        assert_eq!(classify_comment("--------").0, CommentKind::Line);
    }

    #[test]
    fn dash_space_is_a_line_comment() {
        assert_eq!(classify_comment("-- ").0, CommentKind::Line);
        assert_eq!(classify_comment("--x").0, CommentKind::Line);
    }

    #[test]
    fn sigil_without_body_still_classifies() {
        assert_eq!(classify_comment("--~"), (CommentKind::TestAnnot, ""));
        assert_eq!(classify_comment("--="), (CommentKind::Contract, ""));
        assert_eq!(classify_comment("--!"), (CommentKind::ModuleDoc, ""));
        assert_eq!(classify_comment("---"), (CommentKind::ItemDoc, ""));
    }

    #[test]
    fn equals_divider_is_a_line_comment_not_a_contract() {
        // ADR 010:325：`--===` 之类的分隔线按普通行注释处理，否则检查点会
        // 对着 `== ...` 报「未识别断言种类」。
        assert_eq!(classify_comment("--===").0, CommentKind::Line);
        assert_eq!(classify_comment("--========").0, CommentKind::Line);
        assert_eq!(classify_comment("--=== 分节 ===").0, CommentKind::Line);
        // 紧跟空白才是契约断言。
        assert_eq!(classify_comment("--= tailcall f").0, CommentKind::Contract);
        assert_eq!(classify_comment("--=\ttailcall f").0, CommentKind::Contract);
    }

    #[test]
    fn sigil_is_the_third_character_only() {
        // `--x=` 的第三个字符是 `x`，不是契约断言。
        assert_eq!(classify_comment("--x= tailcall f").0, CommentKind::Line);
        // 记号后再出现别的记号只是正文。
        assert_eq!(classify_comment("--~ --= 混在一起").0, CommentKind::TestAnnot);
    }

    #[test]
    fn tokenize_yields_eof_without_panicking() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), "main :: () void => {}", &mut sink);
        assert!(toks.len() > 1, "应产出多个 token，最后一个是 EOF");
        assert_eq!(toks.last().unwrap().kind, TokenKind::Eof);
        assert!(!sink.has_errors());
    }

    // ── 数字字面量 ────────────────────────────────────────────────────────────

    #[test]
    fn lex_number_float() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), "3.14", &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Float(_)));
        assert!(!sink.has_errors());
    }

    #[test]
    fn lex_number_hex() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), "0xFF", &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Int(_)));
        if let TokenKind::Int(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "0xFF");
        }
    }

    #[test]
    fn lex_number_binary() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), "0b1010", &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Int(_)));
        if let TokenKind::Int(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "0b1010");
        }
    }

    #[test]
    fn lex_number_octal() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), "0o755", &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Int(_)));
    }

    #[test]
    fn lex_number_with_underscores() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), "1_000_000", &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Int(_)));
    }

    #[test]
    fn lex_number_float_with_underscores() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), "3.14_15", &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Float(_)));
    }

    // ── 字符串字面量 ──────────────────────────────────────────────────────────

    #[test]
    fn lex_string_escape_newline() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#""\n""#, &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Str(_)));
        if let TokenKind::Str(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "\n");
        }
    }

    #[test]
    fn lex_string_escape_tab() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#""\t""#, &mut sink);
        if let TokenKind::Str(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "\t");
        }
    }

    #[test]
    fn lex_string_escape_cr() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#""\r""#, &mut sink);
        if let TokenKind::Str(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "\r");
        }
    }

    #[test]
    fn lex_string_escape_null() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#""\0""#, &mut sink);
        if let TokenKind::Str(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "\0");
        }
    }

    #[test]
    fn lex_string_escape_backslash() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#""\\""#, &mut sink);
        if let TokenKind::Str(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "\\");
        }
    }

    #[test]
    fn lex_string_escape_quote() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#""\"""#, &mut sink);
        if let TokenKind::Str(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "\"");
        }
    }

    #[test]
    fn lex_string_escape_single_quote() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#""\'""#, &mut sink);
        if let TokenKind::Str(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "'");
        }
    }

    #[test]
    fn lex_string_escape_hex() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#""\x41""#, &mut sink);
        if let TokenKind::Str(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "A");
        }
    }

    #[test]
    fn lex_string_escape_unicode() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#""\u{1F600}""#, &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Str(_)));
    }

    #[test]
    fn lex_string_unknown_escape_emits_e2002() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#""\q""#, &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Str(_)));
        let diags = sink.finish();
        assert!(diags.iter().any(|d| d.code == 2002));
    }

    #[test]
    fn lex_string_multiline_emits_e2001() {
        let mut sink = DiagSink::new();
        let _toks = tokenize(FileId(0), "\"abc\ndef", &mut sink);
        let diags = sink.finish();
        assert!(diags.iter().any(|d| d.code == 2001));
    }

    // ── raw 字符串 ────────────────────────────────────────────────────────────

    #[test]
    fn lex_raw_string_basic() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), "`hello world`", &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Str(_)));
        if let TokenKind::Str(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "hello world");
        }
    }

    #[test]
    fn lex_raw_string_unclosed_at_eof() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), "`unclosed", &mut sink);
        // raw string 遇到 EOF 静默停止，生成一个 Str token
        assert!(matches!(toks[0].kind, TokenKind::Str(_)));
    }

    #[test]
    fn lex_raw_string_backslash_not_escape() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#"`a\nb`"#, &mut sink);
        if let TokenKind::Str(s) = &toks[0].kind {
            assert_eq!(s.as_str(), r"a\nb");
        }
    }

    // ── 字符字面量 ────────────────────────────────────────────────────────────

    #[test]
    fn lex_char_plain() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), "'A'", &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Int(_)));
        if let TokenKind::Int(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "65"); // 'A' = 65
        }
    }

    #[test]
    fn lex_char_escape_newline() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r"'\n'", &mut sink);
        if let TokenKind::Int(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "10"); // '\n' = 10
        }
    }

    #[test]
    fn lex_char_escape_tab() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r"'\t'", &mut sink);
        if let TokenKind::Int(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "9");
        }
    }

    #[test]
    fn lex_char_escape_cr() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r"'\r'", &mut sink);
        if let TokenKind::Int(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "13");
        }
    }

    #[test]
    fn lex_char_escape_null() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r"'\0'", &mut sink);
        if let TokenKind::Int(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "0");
        }
    }

    #[test]
    fn lex_char_escape_backslash() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r"'\\'", &mut sink);
        if let TokenKind::Int(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "92"); // '\\' = 92
        }
    }

    #[test]
    fn lex_char_escape_single_quote() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r"'\''", &mut sink);
        if let TokenKind::Int(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "39"); // '\'' = 39
        }
    }

    #[test]
    fn lex_char_escape_double_quote() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#"'\"'"#, &mut sink);
        if let TokenKind::Int(s) = &toks[0].kind {
            assert_eq!(s.as_str(), "34"); // '"' = 34
        }
    }

    #[test]
    fn lex_char_unknown_escape_emits_e2002() {
        let mut sink = DiagSink::new();
        let _toks = tokenize(FileId(0), r"'\q'", &mut sink);
        let diags = sink.finish();
        assert!(diags.iter().any(|d| d.code == 2002));
    }

    #[test]
    fn lex_char_eof_after_open_emits_e2003() {
        let mut sink = DiagSink::new();
        let _toks = tokenize(FileId(0), "'", &mut sink);
        let diags = sink.finish();
        assert!(diags.iter().any(|d| d.code == 2003));
    }

    // ── 双字符运算符 ──────────────────────────────────────────────────────────

    #[test]
    fn lex_shift_left() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), "<<", &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Punct("<<")));
    }

    #[test]
    fn lex_shift_right() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), ">>", &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Punct(">>")));
    }

    #[test]
    fn lex_logical_and() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), "&&", &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Punct("&&")));
    }

    #[test]
    fn lex_logical_or() {
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), "||", &mut sink);
        assert!(matches!(toks[0].kind, TokenKind::Punct("||")));
    }

    #[test]
    fn lex_unknown_char_emits_e2004() {
        let mut sink = DiagSink::new();
        let _toks = tokenize(FileId(0), "$", &mut sink);
        let diags = sink.finish();
        assert!(diags.iter().any(|d| d.code == 2004));
    }

    // ── hex_value 内部函数覆盖 ────────────────────────────────────────────────

    #[test]
    fn lex_string_hex_lowercase_af() {
        // 触发 hex_value 里 b'a'..=b'f' 分支
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#""\xaf""#, &mut sink);
        if let TokenKind::Str(s) = &toks[0].kind {
            let ch = s.chars().next().unwrap();
            assert_eq!(ch as u32, 0xAF);
        }
    }

    #[test]
    fn lex_string_hex_uppercase_af() {
        // 触发 hex_value 里 b'A'..=b'F' 分支
        let mut sink = DiagSink::new();
        let toks = tokenize(FileId(0), r#""\xAF""#, &mut sink);
        if let TokenKind::Str(s) = &toks[0].kind {
            let ch = s.chars().next().unwrap();
            assert_eq!(ch as u32, 0xAF);
        }
    }
}
