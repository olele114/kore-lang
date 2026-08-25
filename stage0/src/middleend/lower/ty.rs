//! 类型转换：frontend::Type → HirType。
//!
//! Frontend 类型包含具名类型（通过字符串引用），降级时需要：
//! 1. 解析具名类型到结构体/联合体 ID
//! 2. 展平错误联合（T ! E 在 HIR 中表示为 ErrUnion 辅助类型）
//! 3. 转换指针类型（^T vs own ^T）

use crate::frontend::typecheck::Type as FrontendType;
use crate::frontend::ast::node::TypeExpr;
use crate::middleend::hir::{ty::HirType, StructId, UnionId};
use crate::diag::{DiagSink, Diagnostic, DiagLoc, ErrorCode, Span};
use std::collections::HashMap;

/// 类型转换上下文，维护类型名 → ID 映射。
pub struct TypeConverter<'a> {
    /// 结构体名 → ID 映射。
    struct_map: &'a HashMap<String, StructId>,
    /// 联合体名 → ID 映射。
    union_map: &'a HashMap<String, UnionId>,
    /// 诊断接收器。
    diag: &'a mut DiagSink,
}

impl<'a> TypeConverter<'a> {
    pub fn new(
        struct_map: &'a HashMap<String, StructId>,
        union_map: &'a HashMap<String, UnionId>,
        diag: &'a mut DiagSink,
    ) -> Self {
        Self { struct_map, union_map, diag }
    }

    /// 转换 TypeExpr 到 HIR 类型（用于 let 声明等）。
    pub fn convert_type_expr(&mut self, ty: &TypeExpr, _span: Span) -> HirType {
        match ty {
            TypeExpr::Named(name, span) => {
                // 尝试解析基础类型
                match name.as_str() {
                    "i8" => HirType::Int { width: 8, signed: true },
                    "i16" => HirType::Int { width: 16, signed: true },
                    "i32" => HirType::Int { width: 32, signed: true },
                    "i64" => HirType::Int { width: 64, signed: true },
                    "u8" => HirType::Int { width: 8, signed: false },
                    "u16" => HirType::Int { width: 16, signed: false },
                    "u32" => HirType::Int { width: 32, signed: false },
                    "u64" => HirType::Int { width: 64, signed: false },
                    "f32" => HirType::Float { width: 32 },
                    "f64" => HirType::Float { width: 64 },
                    "bool" => HirType::Bool,
                    "str" => HirType::Str,
                    "void" => HirType::Void,
                    "never" => HirType::Never,
                    _ => {
                        // 尝试查找结构体或联合体
                        if let Some(&id) = self.struct_map.get(name) {
                            HirType::Struct(id)
                        } else if let Some(&id) = self.union_map.get(name) {
                            HirType::Union(id)
                        } else {
                            self.diag.emit(Diagnostic::error(
                                ErrorCode::UndefinedName.as_u16(),
                                format!("未找到类型 `{}`", name),
                                DiagLoc::At(*span),
                            ));
                            HirType::Void
                        }
                    }
                }
            }
            TypeExpr::Borrow(inner, span) => {
                let pointee = self.convert_type_expr(inner, *span);
                HirType::Ptr {
                    pointee: Box::new(pointee),
                    owned: false,
                }
            }
            TypeExpr::Own(inner, span) => {
                let pointee = self.convert_type_expr(inner, *span);
                HirType::Ptr {
                    pointee: Box::new(pointee),
                    owned: true,
                }
            }
            TypeExpr::Array(elem, len, span) => {
                let elem_ty = self.convert_type_expr(elem, *span);
                HirType::Array {
                    elem: Box::new(elem_ty),
                    len: *len as usize,
                }
            }
            TypeExpr::Slice(elem, span) => {
                let elem_ty = self.convert_type_expr(elem, *span);
                HirType::Slice {
                    elem: Box::new(elem_ty),
                }
            }
            TypeExpr::ErrUnion(ok, err, span) => {
                let ok_ty = self.convert_type_expr(ok, *span);
                let err_ty = self.convert_type_expr(err, *span);
                HirType::err_union(ok_ty, err_ty)
            }
        }
    }

    /// 转换 frontend 类型到 HIR 类型。
    pub fn convert(&mut self, ty: &FrontendType, span: Span) -> HirType {
        match ty {
            FrontendType::Int { signed, width } => {
                HirType::Int { width: *width, signed: *signed }
            }
            FrontendType::Float { width } => {
                HirType::Float { width: *width }
            }
            FrontendType::Bool => HirType::Bool,
            FrontendType::Str => HirType::Str,
            FrontendType::Void => HirType::Void,
            FrontendType::Never => HirType::Never,
            FrontendType::Borrow(inner) => {
                let pointee = self.convert(inner, span);
                HirType::Ptr {
                    pointee: Box::new(pointee),
                    owned: false,
                }
            }
            FrontendType::Own(inner) => {
                let pointee = self.convert(inner, span);
                HirType::Ptr {
                    pointee: Box::new(pointee),
                    owned: true,
                }
            }
            FrontendType::Array { elem, len } => {
                let elem_ty = self.convert(elem, span);
                HirType::Array {
                    elem: Box::new(elem_ty),
                    len: *len as usize,
                }
            }
            FrontendType::Slice { elem } => {
                let elem_ty = self.convert(elem, span);
                HirType::Slice {
                    elem: Box::new(elem_ty),
                }
            }
            FrontendType::Struct(name) => {
                if let Some(&id) = self.struct_map.get(name) {
                    HirType::Struct(id)
                } else {
                    self.diag.emit(Diagnostic::error(
                        ErrorCode::UndefinedName.as_u16(),
                        format!("未找到结构体类型 `{}`", name),
                        DiagLoc::At(span),
                    ));
                    HirType::Void  // 错误恢复
                }
            }
            FrontendType::Union(name) => {
                if let Some(&id) = self.union_map.get(name) {
                    HirType::Union(id)
                } else {
                    self.diag.emit(Diagnostic::error(
                        ErrorCode::UndefinedName.as_u16(),
                        format!("未找到联合体类型 `{}`", name),
                        DiagLoc::At(span),
                    ));
                    HirType::Void  // 错误恢复
                }
            }
            FrontendType::ErrUnion { ok, err } => {
                let ok_ty = self.convert(ok, span);
                let err_ty = self.convert(err, span);
                HirType::err_union(ok_ty, err_ty)
            }
            FrontendType::Func { params, ret, err } => {
                let param_tys = params.iter()
                    .map(|p| self.convert(p, span))
                    .collect();
                let ret_ty = self.convert(ret, span);

                if err.is_some() {
                    self.diag.emit(Diagnostic::error(
                        ErrorCode::InternalCompilerError.as_u16(),
                        "带错误返回的函数类型在 HIR 中尚未完整实现".to_string(),
                        DiagLoc::At(span),
                    ));
                }

                HirType::FnPtr {
                    params: param_tys,
                    ret: Box::new(ret_ty),
                }
            }
            FrontendType::Unknown => {
                self.diag.emit(Diagnostic::error(
                    ErrorCode::InternalCompilerError.as_u16(),
                    "遇到未知类型（类型检查应该已经失败）".to_string(),
                    DiagLoc::At(span),
                ));
                HirType::Void
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::FileId;

    fn make_span() -> Span {
        Span::new(FileId(0), 0, 1)
    }

    #[test]
    fn convert_basic_types() {
        let mut diag = DiagSink::new();
        let structs = HashMap::new();
        let unions = HashMap::new();
        let mut conv = TypeConverter::new(&structs, &unions, &mut diag);

        let span = make_span();
        assert_eq!(conv.convert(&FrontendType::i32(), span), HirType::i32());
        assert_eq!(conv.convert(&FrontendType::Bool, span), HirType::Bool);
        assert_eq!(conv.convert(&FrontendType::Void, span), HirType::Void);
        assert_eq!(conv.convert(&FrontendType::Never, span), HirType::Never);
    }

    #[test]
    fn convert_pointer_types() {
        let mut diag = DiagSink::new();
        let structs = HashMap::new();
        let unions = HashMap::new();
        let mut conv = TypeConverter::new(&structs, &unions, &mut diag);

        let span = make_span();
        let borrow = FrontendType::Borrow(Box::new(FrontendType::i32()));
        let own = FrontendType::Own(Box::new(FrontendType::i32()));

        assert_eq!(conv.convert(&borrow, span), HirType::ptr(HirType::i32()));
        assert_eq!(conv.convert(&own, span), HirType::own_ptr(HirType::i32()));
    }

    #[test]
    fn convert_array_type() {
        let mut diag = DiagSink::new();
        let structs = HashMap::new();
        let unions = HashMap::new();
        let mut conv = TypeConverter::new(&structs, &unions, &mut diag);

        let span = make_span();
        let arr = FrontendType::Array {
            elem: Box::new(FrontendType::i32()),
            len: 10,
        };

        assert_eq!(conv.convert(&arr, span), HirType::array(HirType::i32(), 10));
    }
}
