//! 类型上下文。管理类型定义和查询。

use crate::diag::DiagSink;
use crate::frontend::ast::TypeExpr;
use super::types::Type;
use std::collections::HashMap;

/// 类型上下文，存储结构体、联合和函数签名的定义。
#[derive(Debug, Clone)]
pub struct TypeContext {
    /// 结构体定义：name -> fields
    structs: HashMap<String, Vec<(String, Type)>>,
    /// 联合定义：name -> variants
    unions: HashMap<String, Vec<(String, Type)>>,
    /// 函数签名：name -> Type::Func
    funcs: HashMap<String, Type>,
}

impl TypeContext {
    pub fn new() -> Self {
        Self {
            structs: HashMap::new(),
            unions: HashMap::new(),
            funcs: HashMap::new(),
        }
    }

    /// 注册结构体定义。
    pub fn define_struct(&mut self, name: String, fields: Vec<(String, Type)>) {
        self.structs.insert(name, fields);
    }

    /// 注册联合定义。
    pub fn define_union(&mut self, name: String, variants: Vec<(String, Type)>) {
        self.unions.insert(name, variants);
    }

    /// 查询结构体定义。
    pub fn get_struct(&self, name: &str) -> Option<&Vec<(String, Type)>> {
        self.structs.get(name)
    }

    /// 查询联合定义。
    pub fn get_union(&self, name: &str) -> Option<&Vec<(String, Type)>> {
        self.unions.get(name)
    }

    /// 注册函数签名（Type::Func）。
    pub fn define_func(&mut self, name: String, func_ty: Type) {
        self.funcs.insert(name, func_ty);
    }

    /// 查询函数类型（返回 Type::Func）。
    pub fn get_func(&self, name: &str) -> Option<Type> {
        self.funcs.get(name).cloned()
    }

    /// 查询结构体字段类型。
    pub fn get_struct_field(&self, struct_name: &str, field_name: &str) -> Option<Type> {
        self.get_struct(struct_name)?
            .iter()
            .find(|(name, _)| name == field_name)
            .map(|(_, ty)| ty.clone())
    }

    /// 查找包含指定变体名的联合类型。
    /// 返回 (union_name, variant_payload_type)。
    pub fn find_variant_union(&self, variant_name: &str) -> Option<(String, Type)> {
        for (union_name, variants) in &self.unions {
            for (v_name, v_type) in variants {
                if v_name == variant_name {
                    return Some((union_name.clone(), v_type.clone()));
                }
            }
        }
        None
    }

    /// 从 AST 类型表达式解析为内部类型。
    pub fn resolve_type_expr(&self, type_expr: &TypeExpr, sink: &mut DiagSink) -> Type {
        match type_expr {
            TypeExpr::Named(name, _) => match name.as_str() {
                "i8"   => Type::Int { signed: true,  width: 8  },
                "i16"  => Type::Int { signed: true,  width: 16 },
                "i32"  => Type::i32(),
                "i64"  => Type::i64(),
                "u8"   => Type::Int { signed: false, width: 8  },
                "u16"  => Type::Int { signed: false, width: 16 },
                "u32"  => Type::u32(),
                "u64"  => Type::u64(),
                "f32"  => Type::Float { width: 32 },
                "f64"  => Type::f64(),
                "bool" => Type::Bool,
                "str"  => Type::Str,
                "void" => Type::Void,
                other => {
                    if self.structs.contains_key(other) {
                        Type::Struct(other.to_string())
                    } else if self.unions.contains_key(other) {
                        Type::Union(other.to_string())
                    } else {
                        // 名字未在类型上下文中注册，返回 Struct 占位，
                        // 诊断由名字消解 pass 负责，这里不重复报错。
                        Type::Struct(other.to_string())
                    }
                }
            },
            TypeExpr::Borrow(inner, _) => {
                Type::Borrow(Box::new(self.resolve_type_expr(inner, sink)))
            }
            TypeExpr::Own(inner, _) => {
                Type::Own(Box::new(self.resolve_type_expr(inner, sink)))
            }
            TypeExpr::Array(elem, len, _) => Type::Array {
                elem: Box::new(self.resolve_type_expr(elem, sink)),
                len: *len,
            },
            TypeExpr::Slice(elem, _) => Type::Slice {
                elem: Box::new(self.resolve_type_expr(elem, sink)),
            },
            TypeExpr::ErrUnion(ok, err, _) => Type::ErrUnion {
                ok:  Box::new(self.resolve_type_expr(ok, sink)),
                err: Box::new(self.resolve_type_expr(err, sink)),
            },
        }
    }
}

impl Default for TypeContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn define_and_query_struct() {
        let mut ctx = TypeContext::new();
        ctx.define_struct(
            "Point".to_string(),
            vec![
                ("x".to_string(), Type::i32()),
                ("y".to_string(), Type::i32()),
            ],
        );

        assert!(ctx.get_struct("Point").is_some());
        assert_eq!(ctx.get_struct_field("Point", "x"), Some(Type::i32()));
        assert_eq!(ctx.get_struct_field("Point", "z"), None);
    }

    #[test]
    fn query_nonexistent_struct() {
        let ctx = TypeContext::new();
        assert!(ctx.get_struct("Foo").is_none());
    }

    #[test]
    fn resolve_named_primitives() {
        use crate::diag::FileId;
        let ctx = TypeContext::new();
        let mut sink = DiagSink::new();
        let sp = crate::diag::Span::new(FileId(0), 0, 0);

        let cases: &[(&str, Type)] = &[
            ("i32",  Type::i32()),
            ("u64",  Type::u64()),
            ("f64",  Type::f64()),
            ("bool", Type::Bool),
            ("str",  Type::Str),
            ("void", Type::Void),
        ];
        for (name, expected) in cases {
            let te = TypeExpr::Named(name.to_string(), sp);
            assert_eq!(ctx.resolve_type_expr(&te, &mut sink), *expected, "failed for {name}");
        }
    }

    #[test]
    fn resolve_borrow_and_own() {
        use crate::diag::FileId;
        let ctx = TypeContext::new();
        let mut sink = DiagSink::new();
        let sp = crate::diag::Span::new(FileId(0), 0, 0);

        let borrow = TypeExpr::Borrow(Box::new(TypeExpr::Named("i32".to_string(), sp)), sp);
        assert_eq!(
            ctx.resolve_type_expr(&borrow, &mut sink),
            Type::Borrow(Box::new(Type::i32()))
        );

        let own = TypeExpr::Own(Box::new(TypeExpr::Named("i32".to_string(), sp)), sp);
        assert_eq!(
            ctx.resolve_type_expr(&own, &mut sink),
            Type::Own(Box::new(Type::i32()))
        );
    }

    #[test]
    fn resolve_array_and_err_union() {
        use crate::diag::FileId;
        let ctx = TypeContext::new();
        let mut sink = DiagSink::new();
        let sp = crate::diag::Span::new(FileId(0), 0, 0);

        let arr = TypeExpr::Array(Box::new(TypeExpr::Named("i32".to_string(), sp)), 4, sp);
        assert_eq!(
            ctx.resolve_type_expr(&arr, &mut sink),
            Type::Array { elem: Box::new(Type::i32()), len: 4 }
        );

        let eu = TypeExpr::ErrUnion(
            Box::new(TypeExpr::Named("i32".to_string(), sp)),
            Box::new(TypeExpr::Named("str".to_string(), sp)),
            sp,
        );
        assert_eq!(
            ctx.resolve_type_expr(&eu, &mut sink),
            Type::ErrUnion { ok: Box::new(Type::i32()), err: Box::new(Type::Str) }
        );
    }
}
