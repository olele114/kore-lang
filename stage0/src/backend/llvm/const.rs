//! HIR 常量到 LLVM 常量的转换。
//!
//! 生成编译期常量值。

use inkwell::values::BasicValueEnum;

use crate::middleend::hir::HirConst;
use super::{CodegenContext, CodegenError};

/// 生成常量值
pub fn codegen_const<'ctx>(
    ctx: &CodegenContext<'ctx>,
    constant: &HirConst,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    match constant {
        HirConst::Int(val, ty) => {
            let llvm_ty = ctx.convert_type(ty)?;
            if let Some(int_ty) = llvm_ty.into_int_type() {
                let const_val = int_ty.const_int(*val as u64, false);
                Ok(const_val.into())
            } else {
                Err(CodegenError::TypeConversion("expected int type".to_string()))
            }
        }

        HirConst::Float(val, ty) => {
            let llvm_ty = ctx.convert_type(ty)?;
            if let Some(float_ty) = llvm_ty.into_float_type() {
                let const_val = float_ty.const_float(*val);
                Ok(const_val.into())
            } else {
                Err(CodegenError::TypeConversion("expected float type".to_string()))
            }
        }

        HirConst::Bool(val) => {
            let bool_ty = ctx.context.bool_type();
            let const_val = bool_ty.const_int(*val as u64, false);
            Ok(const_val.into())
        }

        HirConst::Char(val) => {
            let char_ty = ctx.context.i32_type(); // Unicode scalar
            let const_val = char_ty.const_int(*val as u64, false);
            Ok(const_val.into())
        }

        HirConst::Str(s) => {
            // 字符串常量：创建全局字符串
            let str_val = ctx.context.const_string(s.as_bytes(), true);
            let global = ctx.module.add_global(str_val.get_type(), None, ".str");
            global.set_initializer(&str_val);
            global.set_constant(true);

            Ok(global.as_pointer_value().into())
        }

        HirConst::Null(ty) => {
            let llvm_ty = ctx.convert_type(ty)?;
            if let Some(ptr_ty) = llvm_ty.into_pointer_type() {
                Ok(ptr_ty.const_null().into())
            } else {
                Err(CodegenError::TypeConversion("null expects pointer type".to_string()))
            }
        }
    }
}
