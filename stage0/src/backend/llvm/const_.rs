//! 常量表达式代码生成。
//!
//! 用于全局变量初始化器等编译期常量求值。

use inkwell::values::{BasicValueEnum, BasicValue};
use crate::middleend::hir::Const;
use super::{CodegenContext, CodegenError};

/// 生成编译期常量（用于全局变量初始化等）
pub fn codegen_const_expr<'ctx>(
    ctx: &CodegenContext<'ctx>,
    constant: &Const,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    match constant {
        Const::Void => {
            // Void 不产生值，这里不应该被调用
            Err(CodegenError::BuildError("void constant has no value".to_string()))
        }

        Const::Bool(b) => {
            let bool_val = ctx.context.bool_type().const_int(*b as u64, false);
            Ok(bool_val.as_basic_value_enum())
        }

        Const::Int(val) => {
            // 默认使用 i64 表示整数常量
            let int_val = ctx.context.i64_type().const_int(*val as u64, true);
            Ok(int_val.as_basic_value_enum())
        }

        Const::Float(val) => {
            // 默认使用 f64 表示浮点常量
            let float_val = ctx.context.f64_type().const_float(*val);
            Ok(float_val.as_basic_value_enum())
        }

        Const::Str(s) => {
            // 字符串常量构造为全局常量 + 胖指针 {ptr, len}
            let str_global = ctx.context.const_string(s.as_bytes(), false);
            let str_ptr = ctx.module.add_global(str_global.get_type(), None, "str_const");
            str_ptr.set_initializer(&str_global);
            str_ptr.set_constant(true);

            let ptr_val = str_ptr.as_pointer_value();
            let len_val = ctx.context.i64_type().const_int(s.len() as u64, false);

            // 构造 {ptr, len} 结构体
            let i8_ptr = ctx.context.ptr_type(inkwell::AddressSpace::default());
            let struct_ty = ctx.context.struct_type(&[i8_ptr.into(), ctx.context.i64_type().into()], false);

            let str_struct = struct_ty.const_named_struct(&[
                ptr_val.as_basic_value_enum(),
                len_val.as_basic_value_enum(),
            ]);

            Ok(str_struct.as_basic_value_enum())
        }

        Const::Nil => {
            // Nil 表示空指针
            let ptr_val = ctx.context.ptr_type(inkwell::AddressSpace::default()).const_zero();
            Ok(ptr_val.as_basic_value_enum())
        }
    }
}
