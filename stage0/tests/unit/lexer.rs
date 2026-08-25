//! 测试词法分析器的错误恢复和边界情况。

use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::lexer::token::TokenKind;

#[test]
fn tokenize_empty_string() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "", &mut sink);

    assert_eq!(tokens.len(), 1, "应该只有 EOF");
    assert_eq!(tokens[0].kind, TokenKind::Eof);
    assert!(!sink.has_errors());
}

#[test]
fn tokenize_whitespace_only() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "   \n\t  \n  ", &mut sink);

    assert_eq!(tokens.len(), 1, "空白符应该被跳过，只剩 EOF");
    assert_eq!(tokens[0].kind, TokenKind::Eof);
}

#[test]
fn tokenize_single_line_comment() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "-- this is a comment", &mut sink);

    // 注释被保留为 token
    assert_eq!(tokens.len(), 2, "1 注释 + EOF");
    assert!(matches!(tokens[0].kind, TokenKind::Comment(..)));
    assert_eq!(tokens[1].kind, TokenKind::Eof);
}

#[test]
fn tokenize_multiple_comments() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "-- line 1\n-- line 2\n-- line 3", &mut sink);

    // 3 个注释 + EOF
    assert_eq!(tokens.len(), 4);
    assert!(matches!(tokens[0].kind, TokenKind::Comment(..)));
    assert!(matches!(tokens[1].kind, TokenKind::Comment(..)));
    assert!(matches!(tokens[2].kind, TokenKind::Comment(..)));
    assert_eq!(tokens[3].kind, TokenKind::Eof);
}

#[test]
fn tokenize_identifiers() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "foo bar_123 _underscore", &mut sink);

    assert_eq!(tokens.len(), 4, "3 标识符 + EOF");
    assert!(matches!(tokens[0].kind, TokenKind::Ident(_)));
    assert!(matches!(tokens[1].kind, TokenKind::Ident(_)));
    assert!(matches!(tokens[2].kind, TokenKind::Ident(_)));
}

#[test]
fn tokenize_keywords() {
    // Kore 的 23 个关键字来自 docs/spec/01-overview.md §3；
    // fn/if/else/loop/while 不在其中，会被词法为 Ident。
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "pub impl trait use own", &mut sink);

    assert_eq!(tokens.len(), 6, "5 关键字 + EOF");
    assert!(matches!(tokens[0].kind, TokenKind::Keyword("pub")));
    assert!(matches!(tokens[1].kind, TokenKind::Keyword("impl")));
    assert!(matches!(tokens[2].kind, TokenKind::Keyword("trait")));
    assert!(matches!(tokens[3].kind, TokenKind::Keyword("use")));
    assert!(matches!(tokens[4].kind, TokenKind::Keyword("own")));
}

#[test]
fn non_kore_words_lex_as_idents() {
    // fn/if/else 等 C/Rust 关键字在 Kore 中是普通标识符
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "fn if else loop while", &mut sink);

    assert_eq!(tokens.len(), 6, "5 标识符 + EOF");
    for tok in &tokens[..5] {
        assert!(matches!(tok.kind, TokenKind::Ident(_)));
    }
    assert!(!sink.has_errors());
}

#[test]
fn tokenize_integer_literals() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "0 42 123456", &mut sink);

    assert_eq!(tokens.len(), 4, "3 整数 + EOF");
    assert!(matches!(tokens[0].kind, TokenKind::Int(_)));
    assert!(matches!(tokens[1].kind, TokenKind::Int(_)));
    assert!(matches!(tokens[2].kind, TokenKind::Int(_)));
}

#[test]
fn tokenize_string_literals() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), r#""hello" "world""#, &mut sink);

    assert_eq!(tokens.len(), 3, "2 字符串 + EOF");
    assert!(matches!(tokens[0].kind, TokenKind::Str(_)));
    assert!(matches!(tokens[1].kind, TokenKind::Str(_)));
}

#[test]
fn tokenize_unclosed_string() {
    let mut sink = DiagSink::new();
    let _ = tokenize(FileId(0), r#""unterminated"#, &mut sink);

    assert!(sink.has_errors(), "未闭合的字符串应该产生错误");
    let diags = sink.finish();
    assert_eq!(diags[0].code, 4002);
}

#[test]
fn tokenize_punctuation() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "( ) { } [ ] , ; :", &mut sink);

    assert_eq!(tokens.len(), 10, "9 标点 + EOF");
    assert!(matches!(tokens[0].kind, TokenKind::Punct("(")));
    assert!(matches!(tokens[1].kind, TokenKind::Punct(")")));
    assert!(matches!(tokens[2].kind, TokenKind::Punct("{")));
    assert!(matches!(tokens[3].kind, TokenKind::Punct("}")));
    assert!(matches!(tokens[4].kind, TokenKind::Punct("[")));
    assert!(matches!(tokens[5].kind, TokenKind::Punct("]")));
    assert!(matches!(tokens[6].kind, TokenKind::Punct(",")));
    assert!(matches!(tokens[7].kind, TokenKind::Punct(";")));
    assert!(matches!(tokens[8].kind, TokenKind::Punct(":")));
}

#[test]
fn tokenize_operators() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "+ - * / % == != < > <= >=", &mut sink);

    assert_eq!(tokens.len(), 12, "11 操作符 + EOF");
    assert!(matches!(tokens[0].kind, TokenKind::Punct("+")));
    assert!(matches!(tokens[1].kind, TokenKind::Punct("-")));
    assert!(matches!(tokens[2].kind, TokenKind::Punct("*")));
    assert!(matches!(tokens[3].kind, TokenKind::Punct("/")));
}

#[test]
fn tokenize_double_colon() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "x :: i32", &mut sink);

    assert_eq!(tokens.len(), 4, "ident + :: + ident + EOF");
    assert!(matches!(tokens[0].kind, TokenKind::Ident(_)));
    assert!(matches!(tokens[1].kind, TokenKind::Punct("::")));
    assert!(matches!(tokens[2].kind, TokenKind::Ident(_)));
}

#[test]
fn tokenize_fat_arrow() {
    // Kore 只有 =>（函数体引导符），没有 ->；docs/spec/02-syntax.md 全篇未出现 ->
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "=>", &mut sink);

    assert_eq!(tokens.len(), 2, "=> + EOF");
    assert!(matches!(tokens[0].kind, TokenKind::Punct("=>")));
    assert!(!sink.has_errors());
}

#[test]
fn tokenize_unicode_identifier() {
    // is_ident_continue 仅接受 ASCII；非 ASCII 字节通过 lex_punct 降级，
    // 每个字节产生 E2004，不生成 Ident token。
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "变量", &mut sink);

    // "变量" 共 6 个 UTF-8 字节，每个产生一次 E2004
    assert!(sink.has_errors(), "非 ASCII 字节应产生 E2004");
    let diags = sink.finish();
    assert!(diags.iter().all(|d| d.code == 2004));
    assert_eq!(tokens.len(), 1, "只有 EOF，无 Ident token");
    assert_eq!(tokens[0].kind, TokenKind::Eof);
}

#[test]
fn tokenize_mixed_content() {
    let source = r#"
        fn main() {
            let x: i32 = 42;
            -- comment
            if x > 0 {
                return x + 1;
            }
        }
    "#;

    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), source, &mut sink);

    assert!(!sink.has_errors());
    assert!(tokens.len() > 20, "应该产生多个 token");
    assert_eq!(tokens.last().unwrap().kind, TokenKind::Eof);
}

#[test]
fn tokenize_consecutive_operators() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "x+-y", &mut sink);

    assert_eq!(tokens.len(), 5, "ident + + - ident + EOF");
    assert!(matches!(tokens[0].kind, TokenKind::Ident(_)));
    assert!(matches!(tokens[1].kind, TokenKind::Punct("+")));
    assert!(matches!(tokens[2].kind, TokenKind::Punct("-")));
    assert!(matches!(tokens[3].kind, TokenKind::Ident(_)));
}

#[test]
fn tokenize_number_followed_by_identifier() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "42x", &mut sink);

    // "42x" 可能被解析为数字字面量（如果支持后缀），或者数字 + 标识符
    assert!(tokens.len() >= 2);
}

#[test]
fn tokenize_escaped_characters_in_string() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), r#""hello\nworld""#, &mut sink);

    assert_eq!(tokens.len(), 2, "1 字符串 + EOF");
    assert!(matches!(tokens[0].kind, TokenKind::Str(_)));
}

#[test]
fn tokenize_empty_string_literal() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), r#""""#, &mut sink);

    assert_eq!(tokens.len(), 2, "1 空字符串 + EOF");
    assert!(matches!(tokens[0].kind, TokenKind::Str(_)));
}

#[test]
fn tokenize_line_doc_comment() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "--! This is a doc comment", &mut sink);

    // 文档注释可能被保留为特殊 token，或者被跳过
    assert!(!tokens.is_empty());
    assert_eq!(tokens.last().unwrap().kind, TokenKind::Eof);
}

#[test]
fn tokenize_preserves_spans() {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), "x y", &mut sink);

    assert_eq!(tokens[0].span.lo, 0);
    assert_eq!(tokens[0].span.hi, 1);
    assert_eq!(tokens[1].span.lo, 2);
    assert_eq!(tokens[1].span.hi, 3);
}
