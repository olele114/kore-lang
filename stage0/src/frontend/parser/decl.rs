//! 顶层项与类型表达式的语法分析。
//!
//! 顶层项一律是 `名字 :: ...`，右侧形状决定它是函数、结构体还是联合。

use super::expr::{parse_block, parse_expr};
use super::parser_impl::Parser;
use crate::frontend::ast::{Field, Func, Item, Param, StructDef, TypeExpr, UnionDef, UsePath, Variant};
use crate::frontend::lexer::TokenKind;

/// 顶层项入口。识别 `名字 :: 右侧` 的模式。
pub fn parse_item(p: &mut Parser) -> Option<Item> {
    // 识别 pub 标记
    let is_public = p.eat_keyword("pub");

    // 识别 use 导入
    if p.eat_keyword("use") {
        return Some(Item::Use(parse_use(p)));
    }

    // 识别顶层项：名字 :: 右侧
    let (name, _) = p.eat_ident()?;
    if !p.eat_punct("::") {
        p.unexpected("::");
        p.recover_to_item();
        return None;
    }

    // 根据右侧首记号判定形状
    let first = match p.peek() {
        TokenKind::Punct(s) => *s,
        _ => {
            p.unexpected("( 或 { 或 .");
            p.recover_to_item();
            return None;
        }
    };

    match item_shape(first) {
        Some(ItemShape::Func) => Some(Item::Func(parse_func(p, name, is_public))),
        Some(ItemShape::Struct) => Some(Item::Struct(parse_struct(p, name, is_public))),
        Some(ItemShape::Union) => Some(Item::Union(parse_union(p, name, is_public))),
        None => {
            p.unexpected("( 或 { 或 .");
            p.recover_to_item();
            None
        }
    }
}

/// 解析函数：`f :: (参数..) 返回类型 => 体`
fn parse_func(p: &mut Parser, name: String, is_public: bool) -> Func {
    let start = p.peek_span();

    // 参数表 (a T, b T, ..)
    p.expect_punct("(");
    let mut params = Vec::new();
    while !p.eat_punct(")") {
        if p.at_eof() {
            break;
        }

        // ~name : type
        let is_mut = p.eat_punct("~");
        if let Some((pname, pspan)) = p.eat_ident() {
            let ty = parse_type(p);
            params.push(Param { name: pname, ty, is_mut, span: pspan });

            if !p.eat_punct(",") && !matches!(p.peek(), TokenKind::Punct(")")) {
                break;
            }
        } else {
            p.unexpected("参数名");
            break;
        }
    }

    // 返回类型（可选）
    let ret = if matches!(p.peek(), TokenKind::Punct("=>")) {
        None
    } else {
        Some(parse_type(p))
    };

    // 错误类型（可选）: T ! E
    let err = if p.eat_punct("!") {
        Some(parse_type(p))
    } else {
        None
    };

    // 函数体
    if !p.expect_punct("=>") {
        p.recover_to_item();
        let span = start.extend(p.peek_span());
        return Func { name, params, ret, err, body: parse_expr(p), span, is_public };
    }

    let body = if matches!(p.peek(), TokenKind::Punct("{")) {
        parse_block(p)
    } else {
        parse_expr(p)
    };

    let span = start.extend(body.span());
    Func { name, params, ret, err, body, span, is_public }
}

/// 解析结构体：`Vec3 :: {x, y, z f32}` 或 `Point :: {x, y i32}`
fn parse_struct(p: &mut Parser, name: String, is_public: bool) -> StructDef {
    let start = p.peek_span();
    p.expect_punct("{");

    let mut fields = Vec::new();
    while !p.eat_punct("}") {
        if p.at_eof() {
            break;
        }

        // 收集逗号分隔的字段名：`x, y, z`
        let mut field_names = Vec::new();
        if let Some((fname, fspan)) = p.eat_ident() {
            field_names.push((fname, fspan));

            // 继续收集逗号分隔的字段名
            // 策略：如果看到逗号后跟标识符，且再往后是逗号或非标识符，则这是字段名列表
            while matches!(p.peek(), TokenKind::Punct(",")) {
                let ahead = p.peek_ahead(1);
                if matches!(ahead, TokenKind::Ident(_)) {
                    let ahead2 = p.peek_ahead(2);
                    // 如果是 `, name ,` 或 `, name type`，其中 type 不是逗号/右括号
                    // 那么 name 后面跟的可能是类型
                    if matches!(ahead2, TokenKind::Punct(",") | TokenKind::Punct("}")) {
                        // `, name ,` 或 `, name }`，name 是另一个字段名
                        p.bump(); // 吃掉逗号
                        if let Some((next_name, next_span)) = p.eat_ident() {
                            field_names.push((next_name, next_span));
                        }
                    } else if matches!(ahead2, TokenKind::Ident(_)) {
                        // `, name type_name`，name 是最后一个字段名，type_name 是类型
                        p.bump(); // 吃掉逗号
                        if let Some((next_name, next_span)) = p.eat_ident() {
                            field_names.push((next_name, next_span));
                        }
                        break;
                    } else {
                        // 其他情况，停止收集字段名
                        break;
                    }
                } else {
                    // 逗号后不是标识符，停止
                    break;
                }
            }

            // 解析共享类型
            let ty = parse_type(p);

            // 为所有字段名创建字段
            for (fname, fspan) in field_names {
                fields.push(Field { name: fname, ty: ty.clone(), span: fspan });
            }

            // 可选的尾随逗号
            p.eat_punct(",");
        } else {
            p.unexpected("字段名");
            break;
        }
    }

    let span = start.extend(p.peek_span());
    StructDef { name, fields, span, is_public }
}

/// 解析联合：`Shape :: .Circle(f32) | .Rect(f32, f32)`
fn parse_union(p: &mut Parser, name: String, is_public: bool) -> UnionDef {
    let start = p.peek_span();
    let mut variants = Vec::new();

    loop {
        if !p.eat_punct(".") {
            break;
        }

        if let Some((vname, vspan)) = p.eat_ident() {
            let mut payload = Vec::new();

            // 载荷（可选）
            if p.eat_punct("(") {
                while !p.eat_punct(")") {
                    if p.at_eof() {
                        break;
                    }
                    payload.push(parse_type(p));
                    if !p.eat_punct(",") && !matches!(p.peek(), TokenKind::Punct(")")) {
                        break;
                    }
                }
            }

            variants.push(Variant { name: vname, payload, span: vspan });
        } else {
            p.unexpected("变体名");
            break;
        }

        if !p.eat_punct("|") {
            break;
        }
    }

    let span = start.extend(p.peek_span());
    UnionDef { name, variants, span, is_public }
}

/// 解析导入：`use std.io`
fn parse_use(p: &mut Parser) -> UsePath {
    let start = p.peek_span();
    let mut segments = Vec::new();

    if let Some((seg, _)) = p.eat_ident() {
        segments.push(seg);
    }

    while p.eat_punct(".") {
        if let Some((seg, _)) = p.eat_ident() {
            segments.push(seg);
        } else {
            p.unexpected("模块名");
            break;
        }
    }

    let span = start.extend(p.peek_span());
    UsePath { segments, span }
}

/// 类型表达式解析
pub fn parse_type(p: &mut Parser) -> TypeExpr {
    let start = p.peek_span();

    // own 修饰符
    if p.eat_keyword("own") {
        p.expect_punct("^");
        let inner = parse_type(p);
        let span = start.extend(p.peek_span());
        return TypeExpr::Own(Box::new(inner), span);
    }

    // 借用指针 ^T
    if p.eat_punct("^") {
        let inner = parse_type(p);
        let span = start.extend(p.peek_span());
        return TypeExpr::Borrow(Box::new(inner), span);
    }

    // 数组 [N]T 或切片 []T
    if p.eat_punct("[") {
        // 检查是固定大小数组还是切片
        let size = if let TokenKind::Int(n) = p.peek() {
            let num = n.parse::<u64>().unwrap_or(0);
            p.bump();
            Some(num)
        } else {
            None
        };
        p.expect_punct("]");
        let elem = parse_type(p);
        let span = start.extend(p.peek_span());

        return if let Some(n) = size {
            TypeExpr::Array(Box::new(elem), n, span)
        } else {
            TypeExpr::Slice(Box::new(elem), span)
        };
    }

    // 具名类型
    if let Some((name, _)) = p.eat_ident() {
        let span = start.extend(p.peek_span());

        // 检查错误联合 T ! E
        if p.eat_punct("!") {
            let err_ty = parse_type(p);
            let span = start.extend(p.peek_span());
            return TypeExpr::ErrUnion(Box::new(TypeExpr::Named(name, span)), Box::new(err_ty), span);
        }

        return TypeExpr::Named(name, span);
    }

    // 失败：返回占位类型
    TypeExpr::Named(String::new(), p.peek_span())
}

/// 顶层项的三种右侧形状。判定规则是语言定义，先记下来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemShape {
    /// `(参数..) 返回类型 => 体`
    Func,
    /// `{字段..}`
    Struct,
    /// `.变体 | .变体`
    Union,
}

/// 从 `::` 右侧的首个记号判定项的形状。
pub fn item_shape(first: &str) -> Option<ItemShape> {
    match first {
        "(" => Some(ItemShape::Func),
        "{" => Some(ItemShape::Struct),
        "." => Some(ItemShape::Union),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_shapes_from_the_first_token() {
        assert_eq!(item_shape("("), Some(ItemShape::Func));
        assert_eq!(item_shape("{"), Some(ItemShape::Struct));
        assert_eq!(item_shape("."), Some(ItemShape::Union));
        assert_eq!(item_shape("["), None);
    }
}
