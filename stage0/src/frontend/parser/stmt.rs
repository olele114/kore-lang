//! 语句语法分析。
//!
//! 绑定形式的判定（`::` 编译期、`:`/`:=` 运行期）在这里。

use super::expr::parse_expr;
use super::parser_impl::Parser;
use crate::frontend::ast::{Expr, Stmt};
use crate::frontend::lexer::TokenKind;

use super::decl::parse_type;

/// 语句入口。识别各类语句形式。
pub fn parse_stmt(p: &mut Parser) -> Stmt {
    let start = p.peek_span();

    // defer expr
    if p.eat_keyword("defer") {
        let expr = parse_expr(p);
        let span = start.extend(expr.span());
        return Stmt::Defer(expr, span);
    }

    // 不可变绑定：x : T = expr 或 x := expr
    // 只有当 Ident 后紧跟 : 或 := 时才是绑定
    if let TokenKind::Ident(_) = p.peek() {
        let next = p.peek_ahead(1);
        if matches!(next, TokenKind::Punct(":" | ":=")) {
            let (name, _) = p.eat_ident().unwrap();
            let kind = bind_kind(p.peek()).unwrap();
            p.bump();

            match kind {
                BindKind::Comptime => {
                    p.unexpected("运行期语句中不允许编译期绑定");
                    return Stmt::Expr(parse_expr(p));
                }
                BindKind::RuntimeTyped => {
                    let ty = Some(parse_type(p));
                    p.expect_punct("=");
                    let init = parse_expr(p);
                    let span = start.extend(init.span());
                    return Stmt::Let { name, is_mut: false, ty, init, span };
                }
                BindKind::RuntimeInferred => {
                    let init = parse_expr(p);
                    let span = start.extend(init.span());
                    return Stmt::Let { name, is_mut: false, ty: None, init, span };
                }
            }
        }
    }

    // 赋值或表达式语句：先解析左侧表达式，再判断是否有 = 或 :=
    let expr = parse_expr(p);

    if p.eat_punct("=") {
        let value = parse_expr(p);
        let span = start.extend(value.span());
        return Stmt::Assign { target: expr, value, span };
    }

    if p.eat_punct(":=") {
        // 推断类型的绑定：可能是 x := expr 或 ~x, ~y := expr1, expr2
        let value = parse_expr(p);
        let span = start.extend(value.span());

        // 简单情况：单个标识符或 ~标识符
        match &expr {
            Expr::Path(path, _) if path.len() == 1 => {
                return Stmt::Let {
                    name: path[0].clone(),
                    is_mut: false,
                    ty: None,
                    init: value,
                    span
                };
            }
            Expr::Unary { op: "~", operand, .. } => {
                if let Expr::Path(path, _) = &**operand
                    && path.len() == 1 {
                        return Stmt::Let {
                            name: path[0].clone(),
                            is_mut: true,
                            ty: None,
                            init: value,
                            span
                        };
                    }
            }
            _ => {}
        }

        // 复杂情况：元组解构绑定 ~x, ~y := a, b
        // 展开为多个 Let 语句，但当前 AST 只支持单语句，所以需要特殊处理
        // 暂时将其转换为赋值语句，语义分析阶段再处理
        return Stmt::Assign { target: expr, value, span };
    }

    // 默认：表达式语句
    Stmt::Expr(expr)
}

/// 绑定形式。哪种记号引出哪种绑定是语言定义，先立表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindKind {
    /// `::` 编译期绑定。
    Comptime,
    /// `:` 带类型的运行期绑定。
    RuntimeTyped,
    /// `:=` 推断类型的运行期绑定。
    RuntimeInferred,
}

/// 从记号判定绑定形式。
pub fn bind_kind(k: &TokenKind) -> Option<BindKind> {
    match k {
        TokenKind::Punct("::") => Some(BindKind::Comptime),
        TokenKind::Punct(":") => Some(BindKind::RuntimeTyped),
        TokenKind::Punct(":=") => Some(BindKind::RuntimeInferred),
        _ => None,
    }
}

/// 判定一个关键字是否引出跳转类表达式。这几个都是 never 类型的表达式，
/// 在表达式解析器中处理。此函数保留用于向后兼容。
pub fn is_jump_keyword(k: &str) -> bool {
    matches!(k, "ret" | "jmp" | "stop" | "skip")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_binding_forms_are_distinguished() {
        assert_eq!(bind_kind(&TokenKind::Punct("::")), Some(BindKind::Comptime));
        assert_eq!(bind_kind(&TokenKind::Punct(":")), Some(BindKind::RuntimeTyped));
        assert_eq!(
            bind_kind(&TokenKind::Punct(":=")),
            Some(BindKind::RuntimeInferred)
        );
    }

    #[test]
    fn other_puncts_are_not_bindings() {
        for p in ["=", "?", "@", "^", "~", "!"] {
            assert!(bind_kind(&TokenKind::Punct(leak(p))).is_none(), "{p}");
        }
    }

    #[test]
    fn jump_keywords_are_exactly_four() {
        let mut n = 0;
        for k in crate::frontend::lexer::KEYWORDS {
            if is_jump_keyword(k) {
                n += 1;
            }
        }
        assert_eq!(n, 4, "跳转关键字应为 ret/jmp/stop/skip");
        // defer 不是跳转：它登记延迟执行，控制流不在此处转移。
        assert!(!is_jump_keyword("defer"));
    }

    /// 测试里需要 `&'static str`，把字面量借出去。
    fn leak(s: &str) -> &'static str {
        match s {
            "=" => "=",
            "?" => "?",
            "@" => "@",
            "^" => "^",
            "~" => "~",
            _ => "!",
        }
    }
}
