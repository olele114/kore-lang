//! 表达式语法分析。
//!
//! Pratt 解析器处理二元运算符优先级，后缀运算符 `^`（解引用）和 `!`（传播）
//! 通过 parse_postfix 递归处理。分支 `?` 和循环 `@` 是表达式级语法。

use super::parser_impl::Parser;
use super::stmt::parse_stmt;
use crate::frontend::ast::{Arm, Expr, Pattern, Stmt};
use crate::frontend::lexer::TokenKind;

/// 表达式入口。使用 Pratt 解析器处理二元运算符。
pub fn parse_expr(p: &mut Parser) -> Expr {
    parse_expr_bp(p, 0)
}

/// 带最小约束力的表达式解析（Pratt 算法）。
fn parse_expr_bp(p: &mut Parser, min_bp: u8) -> Expr {
    let mut lhs = parse_prefix(p);

    loop {
        // 后缀运算符：^ ! () [] .
        lhs = parse_postfix(p, lhs);

        // 检查关键字运算符 and/or（优先级固定）
        let (op, bp) = match p.peek() {
            crate::frontend::lexer::TokenKind::Keyword("or") => ("or", 10),
            crate::frontend::lexer::TokenKind::Keyword("and") => ("and", 15),
            crate::frontend::lexer::TokenKind::Punct(s) => {
                let Some(bp) = binding_power(s) else {
                    break;
                };
                (*s, bp)
            }
            _ => break,
        };

        if bp < min_bp {
            break;
        }

        p.bump();
        let rhs = parse_expr_bp(p, bp + 1);
        let span = lhs.span().extend(rhs.span());
        lhs = Expr::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        };
    }

    lhs
}

/// 解析前缀（一元运算、字面量、路径、分支、循环、块）。
fn parse_prefix(p: &mut Parser) -> Expr {
    use crate::frontend::lexer::TokenKind;

    match p.peek() {
        TokenKind::Int(s) => {
            let s = s.clone();
            let span = p.bump();
            Expr::Int(s, span)
        }
        TokenKind::Float(s) => {
            let s = s.clone();
            let span = p.bump();
            Expr::Float(s, span)
        }
        TokenKind::Str(s) => {
            let s = s.clone();
            let span = p.bump();
            Expr::Str(s, span)
        }
        TokenKind::Keyword("true") => {
            let span = p.bump();
            Expr::Bool(true, span)
        }
        TokenKind::Keyword("false") => {
            let span = p.bump();
            Expr::Bool(false, span)
        }
        TokenKind::Keyword("nil") => {
            let span = p.bump();
            Expr::Nil(span)
        }
        TokenKind::Keyword("ret") => parse_ret(p),
        TokenKind::Keyword("stop") => parse_stop(p),
        TokenKind::Keyword("skip") => {
            let start = p.bump();
            let label = if matches!(p.peek(), TokenKind::Punct("@")) {
                p.bump();
                if let TokenKind::Ident(name) = p.peek() {
                    let lbl = name.to_string();
                    p.bump();
                    Some(lbl)
                } else {
                    p.unexpected("标签名");
                    None
                }
            } else {
                None
            };
            Expr::Skip { label, span: start }
        }
        TokenKind::Punct("?") => parse_branch(p),
        TokenKind::Punct("@") => parse_loop(p),
        TokenKind::Punct("{") => parse_block(p),
        TokenKind::Punct("(") => {
            p.bump();
            let e = parse_expr(p);
            p.expect_punct(")");
            e
        }
        TokenKind::Punct("-") => {
            let start = p.bump();
            let operand = parse_prefix(p);
            let span = start.extend(operand.span());
            Expr::Unary {
                op: "-",
                operand: Box::new(operand),
                span,
            }
        }
        TokenKind::Keyword("not") => {
            let start = p.bump();
            let operand = parse_prefix(p);
            let span = start.extend(operand.span());
            Expr::Unary {
                op: "not",
                operand: Box::new(operand),
                span,
            }
        }
        TokenKind::Punct("~") => {
            let start = p.bump();
            let operand = parse_prefix(p);
            let span = start.extend(operand.span());
            Expr::Unary {
                op: "~",
                operand: Box::new(operand),
                span,
            }
        }
        TokenKind::Punct("*") => {
            let start = p.bump();
            let operand = parse_prefix(p);
            let span = start.extend(operand.span());
            Expr::Deref(Box::new(operand), span)
        }
        TokenKind::Punct("[") => parse_array_lit(p),
        TokenKind::Ident(_) | TokenKind::Punct(".") => parse_path_or_struct_lit(p),
        _ => {
            p.unexpected("表达式");
            Expr::Nil(p.peek_span())
        }
    }
}

/// 解析后缀运算符：^ ! () [] .name
fn parse_postfix(p: &mut Parser, mut base: Expr) -> Expr {
    loop {
        match p.peek() {
            crate::frontend::lexer::TokenKind::Punct("^") => {
                p.bump();
                let span = base.span().extend(p.peek_span());
                base = Expr::Deref(Box::new(base), span);
            }
            crate::frontend::lexer::TokenKind::Punct("!") => {
                p.bump();
                let span = base.span().extend(p.peek_span());
                base = Expr::Propagate(Box::new(base), span);
            }
            crate::frontend::lexer::TokenKind::Punct("(") => {
                p.bump();
                let mut args = Vec::new();
                while !p.eat_punct(")") {
                    if p.at_eof() {
                        p.unexpected(")");
                        break;
                    }
                    // 使用更高优先级避免逗号被当作运算符
                    args.push(parse_expr_bp(p, 10));
                    if !p.eat_punct(",") && !matches!(p.peek(), crate::frontend::lexer::TokenKind::Punct(")")) {
                        p.unexpected(", 或 )");
                        break;
                    }
                }
                let span = base.span().extend(p.peek_span());
                base = Expr::Call {
                    callee: Box::new(base),
                    args,
                    span,
                };
            }
            crate::frontend::lexer::TokenKind::Punct("[") => {
                p.bump();
                let index = parse_expr(p);
                p.expect_punct("]");
                let span = base.span().extend(p.peek_span());
                base = Expr::Index {
                    base: Box::new(base),
                    index: Box::new(index),
                    span,
                };
            }
            crate::frontend::lexer::TokenKind::Punct(".") => {
                // 检查是否是变体模式（.Name 开头大写）
                // 如果是，停止 postfix 解析，避免把模式当作字段访问
                if let crate::frontend::lexer::TokenKind::Ident(name) = p.peek_ahead(1) {
                    if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                        // 这看起来是一个变体模式，停止解析
                        break;
                    }
                }

                p.bump();
                if let Some((name, name_span)) = p.eat_ident() {
                    let span = base.span().extend(name_span);
                    base = Expr::Field {
                        base: Box::new(base),
                        name,
                        span,
                    };
                } else {
                    p.unexpected("字段名");
                    break;
                }
            }
            crate::frontend::lexer::TokenKind::Punct("{") => {
                // `{` 总是终止 postfix 解析
                // 结构体字面量应该在 prefix 位置处理，而不是 postfix
                break;
            }
            _ => break,
        }
    }
    base
}

/// 解析路径或结构体字面量：ident 或 .Variant 或 Type{...}
fn parse_path_or_struct_lit(p: &mut Parser) -> Expr {
    let start = p.peek_span();

    if p.eat_punct(".") {
        // .Variant 形式：联合变体构造
        if let Some((name, _)) = p.eat_ident() {
            // 检查是否有 payload: .Variant(expr)
            let payload = if p.eat_punct("(") {
                let expr = parse_expr(p);
                p.expect_punct(")");
                Some(Box::new(expr))
            } else {
                None
            };
            let span = start.extend(p.peek_span());
            return Expr::VariantConstructor { name, payload, span };
        } else {
            p.unexpected("标识符");
            return Expr::Nil(start);
        }
    } else if let Some((name, _)) = p.eat_ident() {
        // 普通路径
        let span = start.extend(p.peek_span());
        return Expr::Path(vec![name], span);
    } else {
        p.unexpected("标识符");
        return Expr::Nil(start);
    }
}

/*
/// 解析结构体字面量 Type{field: value, ...}
/// 注意：当前暂未使用，因为 Type{...} 语法与块语法冲突
fn parse_struct_lit_with_name(p: &mut Parser, name: String, start: Span) -> Expr {
    p.expect_punct("{");
    let mut fields = Vec::new();

    while !p.eat_punct("}") {
        if p.at_eof() {
            p.unexpected("}");
            break;
        }

        // 解析字段名
        if let Some((field_name, _)) = p.eat_ident() {
            p.expect_punct(":");
            let value = parse_expr_bp(p, 10); // 低优先级避免吃掉逗号
            fields.push((field_name, value));

            if !p.eat_punct(",") && !matches!(p.peek(), crate::frontend::lexer::TokenKind::Punct("}")) {
                p.unexpected(", 或 }");
                break;
            }
        } else {
            p.unexpected("字段名");
            break;
        }
    }

    let span = start.extend(p.peek_span());
    Expr::StructLit { name, fields, span }
}
*/

/// 解析数组字面量 [elem1, elem2, ...]
fn parse_array_lit(p: &mut Parser) -> Expr {
    let start = p.bump(); // 吃掉 [
    let mut elements = Vec::new();

    while !p.eat_punct("]") {
        if p.at_eof() {
            p.unexpected("]");
            break;
        }

        elements.push(parse_expr_bp(p, 10)); // 低优先级避免吃掉逗号

        if !p.eat_punct(",") && !matches!(p.peek(), crate::frontend::lexer::TokenKind::Punct("]")) {
            p.unexpected(", 或 ]");
            break;
        }
    }

    let span = start.extend(p.peek_span());
    Expr::ArrayLit { elements, span }
}

/// 块 `{ .. }`。最后一个表达式是返回值。
pub fn parse_block(p: &mut Parser) -> Expr {
    let start = p.peek_span();
    p.expect_punct("{");

    let mut stmts = Vec::new();

    while !p.eat_punct("}") {
        if p.at_eof() {
            p.unexpected("}");
            break;
        }

        // 检查是否是连续的守卫语句 `? cond => body`
        // 如果是，收集所有连续的守卫到一个 Branch 表达式中
        if matches!(p.peek(), TokenKind::Punct("?")) {
            let mut arms = Vec::new();
            let guard_start = p.peek_span();

            // 收集所有连续的 `? cond => body` 守卫
            while matches!(p.peek(), TokenKind::Punct("?")) {
                // 向前看判断是否是守卫形式
                let next_tok = p.peek_ahead(1);

                // 如果是 `? {`，这是条件链语法，作为普通语句处理
                if matches!(next_tok, TokenKind::Punct("{")) {
                    break;
                }

                p.bump(); // 吃掉 ?

                // 解析条件表达式
                let cond = parse_expr_bp(p, 10);

                // 如果是 `? expr is {`，这是模式匹配语法
                if p.eat_keyword("is") {
                    p.expect_punct("{");
                    let arms = parse_arms(p);
                    p.expect_punct("}");
                    let span = guard_start.extend(p.peek_span());
                    let branch_expr = Expr::Branch {
                        scrutinee: Some(Box::new(cond)),
                        arms,
                        span,
                    };
                    stmts.push(Stmt::Expr(branch_expr));
                    continue;
                }

                // 必须是 `=>` 才是守卫语法
                if !p.eat_punct("=>") {
                    p.unexpected("=>");
                    break;
                }

                let body = parse_expr(p);
                let arm_span = guard_start.extend(body.span());
                arms.push(Arm {
                    pattern: Pattern::Cond(Box::new(cond)),
                    body,
                    span: arm_span,
                });
            }

            // 如果收集到了守卫 arms，创建一个 Branch 表达式语句
            if !arms.is_empty() {
                let branch_span = guard_start.extend(arms.last().unwrap().span);
                let branch_expr = Expr::Branch {
                    scrutinee: None,
                    arms,
                    span: branch_span,
                };
                stmts.push(Stmt::Expr(branch_expr));
                continue;
            }
        }

        let pos_before = p.pos();
        stmts.push(parse_stmt(p));

        // 如果 parse_stmt 没有推进 parser，强制跳过当前 token 避免无限循环
        if p.pos() == pos_before {
            p.bump();
        }
    }

    let span = start.extend(p.peek_span());
    Expr::Block { stmts, span }
}

/// 分支 `?`。三种形态：守卫、条件链、匹配。
pub fn parse_branch(p: &mut Parser) -> Expr {
    let start = p.bump(); // 吃掉 ?

    // 守卫：? cond => body（单臂，无花括号）
    if !matches!(p.peek(), crate::frontend::lexer::TokenKind::Punct("{")) {
        let cond = parse_expr_bp(p, 10); // 低优先级，=> 不会被吃掉
        if p.eat_punct("=>") {
            let body = parse_expr(p);
            let span = start.extend(body.span());
            return Expr::Branch {
                scrutinee: None,
                arms: vec![Arm {
                    pattern: Pattern::Cond(Box::new(cond)),
                    body,
                    span,
                }],
                span,
            };
        } else {
            // ? expr is { ... } 形式的匹配
            if p.eat_keyword("is") {
                p.expect_punct("{");
                let arms = parse_arms(p);
                p.expect_punct("}");
                let span = start.extend(p.peek_span());
                return Expr::Branch {
                    scrutinee: Some(Box::new(cond)),
                    arms,
                    span,
                };
            } else {
                p.unexpected("=> 或 is");
                return Expr::Nil(start);
            }
        }
    }

    // 条件链：? { cond => body, ... }
    p.bump(); // 吃掉 {
    let arms = parse_arms(p);
    p.expect_punct("}");
    let span = start.extend(p.peek_span());

    Expr::Branch {
        scrutinee: None,
        arms,
        span,
    }
}

/// 循环 `@`。三种形态：条件循环、范围循环、迭代。
pub fn parse_loop(p: &mut Parser) -> Expr {
    let start = p.bump(); // 吃掉第一个 @

    // 检查是否有标签 `@label @ { ... }`
    let label = if let TokenKind::Ident(name) = p.peek() {
        let lbl = name.to_string();
        p.bump();
        // 吃掉标签后的第二个 @
        if p.eat_punct("@") {
            Some(lbl)
        } else {
            // 如果没有第二个 @，这是一个错误的语法
            p.unexpected("@");
            Some(lbl)
        }
    } else {
        None
    };

    // @ { body } 无限循环
    if matches!(p.peek(), crate::frontend::lexer::TokenKind::Punct("{")) {
        let body = parse_block(p);
        let span = start.extend(body.span());
        return Expr::Loop {
            label,
            subject: None,
            body: Box::new(body),
            span,
        };
    }

    // @ cond { body } 或 @ x in range { body }
    let subject = parse_expr_bp(p, 10); // 低优先级避免吃掉 {
    let body = parse_block(p);
    let span = start.extend(body.span());

    Expr::Loop {
        label,
        subject: Some(Box::new(subject)),
        body: Box::new(body),
        span,
    }
}

/// 解析多条臂 pattern => body。
fn parse_arms(p: &mut Parser) -> Vec<Arm> {
    let mut arms = Vec::new();

    while !matches!(p.peek(), crate::frontend::lexer::TokenKind::Punct("}")) && !p.at_eof() {
        // 保存解析位置，用于检测是否是新的 arm
        let arm_start = p.pos();

        arms.push(parse_arm(p));

        // 如果位置没有前进，说明解析失败，避免死循环
        if p.pos() == arm_start {
            break;
        }

        // 臂之间可以用逗号或换行分隔，这里宽松处理
        p.eat_punct(",");
    }

    arms
}

/// 一条臂 `模式 => 表达式`。
pub fn parse_arm(p: &mut Parser) -> Arm {
    let start = p.peek_span();
    let pattern = parse_pattern(p);

    if !p.expect_punct("=>") {
        return Arm {
            pattern,
            body: Expr::Nil(p.peek_span()),
            span: start,
        };
    }

    // 解析 arm body:
    // - 如果是块表达式 { ... }，解析整个块
    // - 否则解析单个语句表达式（如 `ret v`），在换行或 `,` 或 `}` 处停止
    let body = if matches!(p.peek(), crate::frontend::lexer::TokenKind::Punct("{")) {
        parse_expr_bp(p, 10)
    } else {
        // 解析语句表达式：ret/skip/jmp/break 等，这些通常是单行
        parse_stmt_expr(p)
    };

    let span = start.extend(body.span());

    Arm { pattern, body, span }
}

/// 解析单个语句表达式（用于 arm body）
///
/// 这个函数特殊处理控制流关键字，避免它们的参数表达式吃掉后续的模式。
fn parse_stmt_expr(p: &mut Parser) -> Expr {
    use crate::frontend::lexer::TokenKind;

    match p.peek() {
        TokenKind::Keyword("ret") => parse_ret(p),
        TokenKind::Keyword("stop") => parse_stop(p),
        TokenKind::Keyword("skip" | "jmp" | "break") => parse_prefix(p),
        _ => parse_expr_bp(p, 10)
    }
}

/// 模式。Kore0 只要求认出变体、绑定、字面量与通配。
pub fn parse_pattern(p: &mut Parser) -> Pattern {
    use crate::frontend::lexer::TokenKind;

    let start = p.peek_span();

    match p.peek() {
        // .Variant 或 .Variant(bindings)
        TokenKind::Punct(".") => {
            p.bump();
            if let Some((name, _name_span)) = p.eat_ident() {
                let mut bindings = Vec::new();
                if p.eat_punct("(") {
                    while !p.eat_punct(")") {
                        if p.at_eof() {
                            p.unexpected(")");
                            break;
                        }
                        if let Some((binding, _)) = p.eat_ident() {
                            bindings.push(binding);
                        } else {
                            p.unexpected("标识符");
                            break;
                        }
                        if !p.eat_punct(",") && !matches!(p.peek(), TokenKind::Punct(")")) {
                            p.unexpected(", 或 )");
                            break;
                        }
                    }
                }
                let span = start.extend(p.peek_span());
                Pattern::Variant { name, bindings, span }
            } else {
                p.unexpected("变体名");
                Pattern::Wildcard(start)
            }
        }
        // 字面量模式
        TokenKind::Int(_) | TokenKind::Float(_) | TokenKind::Str(_) | TokenKind::Keyword("true" | "false" | "nil") => {
            let lit = parse_prefix(p);
            Pattern::Lit(Box::new(lit))
        }
        // 条件模式（用于条件链的臂）或绑定
        TokenKind::Ident(name) => {
            // true/false/nil 是字面量，不是绑定
            // 需要手动构造字面量表达式，因为 parse_prefix 会将它们解析为 Path
            if name == "true" {
                let span = p.bump();
                return Pattern::Lit(Box::new(Expr::Bool(true, span)));
            }
            if name == "false" {
                let span = p.bump();
                return Pattern::Lit(Box::new(Expr::Bool(false, span)));
            }
            if name == "nil" {
                let span = p.bump();
                return Pattern::Lit(Box::new(Expr::Nil(span)));
            }

            // Lookahead：peek 下一个 token，如果是二元运算符，解析表达式
            let next = p.peek_ahead(1);
            let is_binop = matches!(next, TokenKind::Punct(op) if binding_power(op).is_some());

            if is_binop {
                // 是条件表达式
                let expr = parse_expr(p);
                Pattern::Cond(Box::new(expr))
            } else {
                // 是绑定
                if let Some((name, span)) = p.eat_ident() {
                    Pattern::Bind(name, span)
                } else {
                    Pattern::Wildcard(start)
                }
            }
        }
        _ => {
            // 其他情况解析为条件表达式
            let expr = parse_expr(p);
            Pattern::Cond(Box::new(expr))
        }
    }
}

/// 二元运算符优先级。数字越大越紧。表先立在这里，因为优先级是语言定义的一
/// 部分，不该等到写分析器时才拍。
///
/// 逻辑运算用词 `and`/`or`/`not`，位运算用词 `xor`/`inv`/`rol`/`ror`
/// （见关键字表），所以它们不在这张记号表里。
pub fn binding_power(op: &str) -> Option<u8> {
    let bp = match op {
        "," => 5,   // 最低优先级：元组构造
        "*" | "/" | "%" => 70,
        "+" | "-" => 60,
        "<<" | ">>" => 50,
        "&" | "|" => 40,
        "<" | "<=" | ">" | ">=" => 30,
        "==" | "!=" => 20,
        _ => return None,
    };
    Some(bp)
}

/// 解析 `ret` 表达式：`ret` 或 `ret expr`
fn parse_ret(p: &mut Parser) -> Expr {
    let start = p.bump(); // 消费 ret

    // 如果后面是语句/表达式分隔符或块结束符，这是 `ret`（无值）
    if matches!(
        p.peek(),
        crate::frontend::lexer::TokenKind::Punct("}")
        | crate::frontend::lexer::TokenKind::Punct(";")
        | crate::frontend::lexer::TokenKind::Eof
    ) {
        return Expr::Ret(None, start);
    }

    // 使用 min_bp=10 解析表达式，避免吃掉逗号和后续 arm
    let val = parse_expr_bp(p, 10);
    let span = start.extend(val.span());
    Expr::Ret(Some(Box::new(val)), span)
}

/// 解析 `stop` 表达式：`stop` 或 `stop @label`
fn parse_stop(p: &mut Parser) -> Expr {
    let start = p.bump(); // 消费 stop

    // 检查是否有标签 `stop @label`
    let label = if p.eat_punct("@") {
        if let TokenKind::Ident(name) = p.peek() {
            let lbl = name.to_string();
            p.bump();
            Some(lbl)
        } else {
            p.unexpected("标签名");
            None
        }
    } else {
        None
    };

    Expr::Stop { label, span: start }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplication_binds_tighter_than_addition() {
        assert!(binding_power("*").unwrap() > binding_power("+").unwrap());
    }

    #[test]
    fn comparison_binds_looser_than_arithmetic() {
        assert!(binding_power("<").unwrap() < binding_power("-").unwrap());
        assert!(binding_power("==").unwrap() < binding_power("<").unwrap());
    }

    #[test]
    fn word_operators_are_not_in_the_punct_table() {
        // 逻辑与位运算是关键字，走另一条路径。
        for w in ["and", "or", "not", "xor", "inv", "rol", "ror"] {
            assert!(binding_power(w).is_none(), "{w} 不该出现在记号优先级表里");
        }
    }
}
