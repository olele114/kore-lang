//! HIR 语句代码生成。
//!
//! 将 HirStmt（Assign, Call, Drop）转换为 LLVM IR 指令。

use inkwell::values::{PointerValue, BasicValueEnum};
use std::collections::HashMap;

use crate::middleend::hir::{HirStmt, HirLocal, LocalId};
use super::{CodegenContext, CodegenError};

/// 从值中提取 C 字符串指针
///
/// 处理两种情况：
/// 1. 常量字符串：直接是指针
/// 2. Kore 字符串结构体 {ptr, len}：提取第一个字段
fn extract_c_string_ptr<'ctx>(
    ctx: &CodegenContext<'ctx>,
    val: BasicValueEnum<'ctx>,
) -> Result<PointerValue<'ctx>, CodegenError> {
    if val.is_pointer_value() {
        Ok(val.into_pointer_value())
    } else if val.is_struct_value() {
        let str_struct = val.into_struct_value();
        let ptr = ctx.builder.build_extract_value(str_struct, 0, "str_ptr")
            .map_err(|e| CodegenError::BuildError(e.to_string()))?;
        Ok(ptr.into_pointer_value())
    } else {
        Err(CodegenError::BuildError(format!("expected string value, got {:?}", val.get_type())))
    }
}


/// 生成单条语句
pub fn codegen_stmt<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
    locals: &HashMap<LocalId, PointerValue<'ctx>>,
    local_types: &[HirLocal],
    stmt: &HirStmt,
    current_fn: inkwell::values::FunctionValue<'ctx>,
) -> Result<(), CodegenError> {
    match stmt {
        HirStmt::Assign { lhs, rhs, .. } => {
            // 计算左值地址
            let lhs_ptr = super::place::codegen_place(ctx, locals, local_types, lhs)?;

            // 计算右值
            let rhs_val = super::rvalue::codegen_rvalue(ctx, locals, local_types, rhs)?;

            // 存储
            ctx.builder.build_store(lhs_ptr, rhs_val)
                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

            Ok(())
        }

        HirStmt::Call { dest, func, args, .. } => {
            use crate::middleend::hir::HirOperand;

            // 检查是否为内置函数调用
            if let HirOperand::FuncRef(func_id) = func {
                let _hir = ctx.hir_module
                    .ok_or_else(|| CodegenError::BuildError("HIR module not set".to_string()))?;

                // 内置函数处理
                match func_id.0 {
                    0 => {
                        // print(arg) -> 根据类型选择打印方式
                        if args.len() != 1 {
                            return Err(CodegenError::BuildError(
                                format!("print() expects 1 argument, got {}", args.len())
                            ));
                        }

                        let arg_val = super::rvalue::codegen_operand(ctx, locals, local_types, &args[0])?;

                        // 根据参数类型判断
                        if arg_val.is_int_value() {
                            // 整数：使用 printf("%d", val)
                            let printf_fn = super::runtime::declare_printf(ctx)?;
                            let fmt_str = ctx.builder.build_global_string_ptr("%d", "int_fmt")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            ctx.builder.build_call(printf_fn, &[fmt_str.as_pointer_value().into(), arg_val.into()], "print_int")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                        } else {
                            // 字符串：直接使用 printf("%s", str) 不带换行（不使用格式化字符串）
                            let printf_fn = super::runtime::declare_printf(ctx)?;
                            let str_ptr = extract_c_string_ptr(ctx, arg_val)?;

                            // 检查字符串指针是否为 null
                            let null_ptr = ctx.context.ptr_type(inkwell::AddressSpace::default()).const_null();
                            let is_null = ctx.builder.build_int_compare(
                                inkwell::IntPredicate::EQ,
                                str_ptr,
                                null_ptr,
                                "is_null_str"
                            ).map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            let then_bb = ctx.context.append_basic_block(current_fn, "print_null");
                            let else_bb = ctx.context.append_basic_block(current_fn, "print_ok");
                            let cont_bb = ctx.context.append_basic_block(current_fn, "print_cont");

                            ctx.builder.build_conditional_branch(is_null, then_bb, else_bb)
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // Null 分支：不打印或打印空字符串
                            ctx.builder.position_at_end(then_bb);
                            ctx.builder.build_unconditional_branch(cont_bb)
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // 正常分支：打印字符串
                            ctx.builder.position_at_end(else_bb);
                            ctx.builder.build_call(printf_fn, &[str_ptr.into()], "print")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            ctx.builder.build_unconditional_branch(cont_bb)
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // 继续执行
                            ctx.builder.position_at_end(cont_bb);
                        }

                        return Ok(());
                    }
                    1 => {
                        // println(arg) -> 根据类型选择打印方式
                        if args.len() != 1 {
                            return Err(CodegenError::BuildError(
                                format!("println() expects 1 argument, got {}", args.len())
                            ));
                        }

                        let arg_val = super::rvalue::codegen_operand(ctx, locals, local_types, &args[0])?;

                        // 根据参数类型判断
                        if arg_val.is_int_value() {
                            // 整数：使用 printf("%d\n", val)
                            let printf_fn = super::runtime::declare_printf(ctx)?;
                            let fmt_str = ctx.builder.build_global_string_ptr("%d\n", "int_fmt")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            ctx.builder.build_call(printf_fn, &[fmt_str.as_pointer_value().into(), arg_val.into()], "println_int")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                        } else {
                            // 字符串：使用 puts(str)，但先检查是否为 null
                            let puts_fn = super::runtime::declare_puts(ctx)?;
                            let str_ptr = extract_c_string_ptr(ctx, arg_val)?;

                            // 检查字符串指针是否为 null
                            let null_ptr = ctx.context.ptr_type(inkwell::AddressSpace::default()).const_null();
                            let is_null = ctx.builder.build_int_compare(
                                inkwell::IntPredicate::EQ,
                                str_ptr,
                                null_ptr,
                                "is_null_str"
                            ).map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            let then_bb = ctx.context.append_basic_block(current_fn, "println_null");
                            let else_bb = ctx.context.append_basic_block(current_fn, "println_ok");
                            let cont_bb = ctx.context.append_basic_block(current_fn, "println_cont");

                            ctx.builder.build_conditional_branch(is_null, then_bb, else_bb)
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // Null 分支：打印空字符串或什么都不做
                            ctx.builder.position_at_end(then_bb);
                            let empty_str = ctx.builder.build_global_string_ptr("", "empty_str")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            ctx.builder.build_call(puts_fn, &[empty_str.as_pointer_value().into()], "println_empty")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            ctx.builder.build_unconditional_branch(cont_bb)
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // 正常分支：打印字符串
                            ctx.builder.position_at_end(else_bb);
                            ctx.builder.build_call(puts_fn, &[str_ptr.into()], "println")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            ctx.builder.build_unconditional_branch(cont_bb)
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // 继续执行
                            ctx.builder.position_at_end(cont_bb);
                        }

                        return Ok(());
                    }
                    2 => {
                        // read_file(path) -> 调用 C 的文件读取
                        if args.len() != 1 {
                            return Err(CodegenError::BuildError(
                                format!("read_file() expects 1 argument, got {}", args.len())
                            ));
                        }

                        let read_file_fn = super::runtime::declare_read_file(ctx)?;
                        let path_val = super::rvalue::codegen_operand(ctx, locals, local_types, &args[0])?;
                        let path_ptr = extract_c_string_ptr(ctx, path_val)?;

                        let result = ctx.builder.build_call(read_file_fn, &[path_ptr.into()], "read_file")
                            .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                        // read_file 返回 C 字符串指针，需要包装成 Kore 字符串结构体
                        if let Some(dest_place) = dest {
                            let dest_ptr = super::place::codegen_place(ctx, locals, local_types, dest_place)?;
                            if let Some(return_val) = result.try_as_basic_value().basic() {
                                let c_str_ptr = return_val.into_pointer_value();

                                // 检查空指针（文件读取失败）
                                let null_ptr = ctx.context.ptr_type(inkwell::AddressSpace::default()).const_null();
                                let is_null = ctx.builder.build_int_compare(
                                    inkwell::IntPredicate::EQ,
                                    c_str_ptr,
                                    null_ptr,
                                    "is_null"
                                ).map_err(|e| CodegenError::BuildError(e.to_string()))?;

                                let then_bb = ctx.context.append_basic_block(current_fn, "read_null");
                                let else_bb = ctx.context.append_basic_block(current_fn, "read_ok");
                                let cont_bb = ctx.context.append_basic_block(current_fn, "read_cont");

                                ctx.builder.build_conditional_branch(is_null, then_bb, else_bb)
                                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                                // 空指针分支：返回空字符串 {null, 0}
                                ctx.builder.position_at_end(then_bb);
                                let str_ty = ctx.context.struct_type(&[
                                    ctx.context.ptr_type(inkwell::AddressSpace::default()).into(),
                                    ctx.context.i64_type().into(),
                                ], false);
                                let mut empty_str = str_ty.get_undef();
                                empty_str = ctx.builder.build_insert_value(empty_str, null_ptr, 0, "empty_ptr")
                                    .map_err(|e| CodegenError::BuildError(e.to_string()))?
                                    .into_struct_value();
                                empty_str = ctx.builder.build_insert_value(empty_str, ctx.context.i64_type().const_zero(), 1, "empty_len")
                                    .map_err(|e| CodegenError::BuildError(e.to_string()))?
                                    .into_struct_value();
                                ctx.builder.build_store(dest_ptr, empty_str)
                                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                                ctx.builder.build_unconditional_branch(cont_bb)
                                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                                // 正常分支：计算长度并构造字符串
                                ctx.builder.position_at_end(else_bb);
                                let strlen_fn = super::runtime::declare_strlen(ctx)?;
                                let strlen_result = ctx.builder.build_call(strlen_fn, &[c_str_ptr.into()], "strlen")
                                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                                let len_val = strlen_result.try_as_basic_value().basic()
                                    .ok_or_else(|| CodegenError::BuildError("strlen must return a value".to_string()))?
                                    .into_int_value();

                                let mut str_val = str_ty.get_undef();
                                str_val = ctx.builder.build_insert_value(str_val, c_str_ptr, 0, "insert_ptr")
                                    .map_err(|e| CodegenError::BuildError(e.to_string()))?
                                    .into_struct_value();
                                str_val = ctx.builder.build_insert_value(str_val, len_val, 1, "insert_len")
                                    .map_err(|e| CodegenError::BuildError(e.to_string()))?
                                    .into_struct_value();
                                ctx.builder.build_store(dest_ptr, str_val)
                                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                                ctx.builder.build_unconditional_branch(cont_bb)
                                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                                // 继续执行
                                ctx.builder.position_at_end(cont_bb);
                            } else {
                                return Err(CodegenError::BuildError("read_file must return a value".to_string()));
                            }
                        }

                        return Ok(());
                    }
                    3 => {
                        // write_file(path, content) -> 调用 C 的文件写入
                        if args.len() != 2 {
                            return Err(CodegenError::BuildError(
                                format!("write_file() expects 2 arguments, got {}", args.len())
                            ));
                        }

                        let write_file_fn = super::runtime::declare_write_file(ctx)?;
                        let path_val = super::rvalue::codegen_operand(ctx, locals, local_types, &args[0])?;
                        let content_val = super::rvalue::codegen_operand(ctx, locals, local_types, &args[1])?;
                        let path_ptr = extract_c_string_ptr(ctx, path_val)?;
                        let content_ptr = extract_c_string_ptr(ctx, content_val)?;

                        let result = ctx.builder.build_call(write_file_fn, &[path_ptr.into(), content_ptr.into()], "write_file")
                            .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                        // 如果有目标位置，存储返回值（i32）
                        if let Some(dest_place) = dest {
                            let dest_ptr = super::place::codegen_place(ctx, locals, local_types, dest_place)?;
                            if let Some(return_val) = result.try_as_basic_value().basic() {
                                ctx.builder.build_store(dest_ptr, return_val)
                                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            } else {
                                return Err(CodegenError::BuildError("write_file must return a value".to_string()));
                            }
                        }

                        return Ok(());
                    }
                    4 => {
                        // eprint(arg) -> fprintf(stderr, "%s", str)
                        if args.len() != 1 {
                            return Err(CodegenError::BuildError(
                                format!("eprint() expects 1 argument, got {}", args.len())
                            ));
                        }

                        let arg_val = super::rvalue::codegen_operand(ctx, locals, local_types, &args[0])?;

                        // 根据参数类型判断
                        let file_ptr_ty = ctx.context.ptr_type(inkwell::AddressSpace::default());

                        if arg_val.is_int_value() {
                            // 整数：使用 fprintf(stderr, "%d", val)
                            let fprintf_fn = super::runtime::declare_fprintf(ctx)?;
                            let stderr_ptr = super::runtime::get_stderr(ctx)?;
                            let stderr_val = ctx.builder.build_load(file_ptr_ty, stderr_ptr, "stderr")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            let fmt_str = ctx.builder.build_global_string_ptr("%d", "int_fmt_err")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            ctx.builder.build_call(fprintf_fn, &[stderr_val.into(), fmt_str.as_pointer_value().into(), arg_val.into()], "eprint_int")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                        } else {
                            // 字符串：使用 fprintf(stderr, "%s", str)
                            let fprintf_fn = super::runtime::declare_fprintf(ctx)?;
                            let stderr_ptr = super::runtime::get_stderr(ctx)?;
                            let stderr_val = ctx.builder.build_load(file_ptr_ty, stderr_ptr, "stderr")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            let str_ptr = extract_c_string_ptr(ctx, arg_val)?;
                            let fmt_str = ctx.builder.build_global_string_ptr("%s", "str_fmt_err")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // 检查字符串指针是否为 null
                            let null_ptr = ctx.context.ptr_type(inkwell::AddressSpace::default()).const_null();
                            let is_null = ctx.builder.build_int_compare(
                                inkwell::IntPredicate::EQ,
                                str_ptr,
                                null_ptr,
                                "is_null_str"
                            ).map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            let then_bb = ctx.context.append_basic_block(current_fn, "eprint_null");
                            let else_bb = ctx.context.append_basic_block(current_fn, "eprint_ok");
                            let cont_bb = ctx.context.append_basic_block(current_fn, "eprint_cont");

                            ctx.builder.build_conditional_branch(is_null, then_bb, else_bb)
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // Null 分支：不打印
                            ctx.builder.position_at_end(then_bb);
                            ctx.builder.build_unconditional_branch(cont_bb)
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // 正常分支：打印字符串到 stderr
                            ctx.builder.position_at_end(else_bb);
                            ctx.builder.build_call(fprintf_fn, &[stderr_val.into(), fmt_str.as_pointer_value().into(), str_ptr.into()], "eprint")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            ctx.builder.build_unconditional_branch(cont_bb)
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // 继续执行
                            ctx.builder.position_at_end(cont_bb);
                        }

                        return Ok(());
                    }
                    5 => {
                        // eprintln(arg) -> fprintf(stderr, "%s\n", str)
                        if args.len() != 1 {
                            return Err(CodegenError::BuildError(
                                format!("eprintln() expects 1 argument, got {}", args.len())
                            ));
                        }

                        let arg_val = super::rvalue::codegen_operand(ctx, locals, local_types, &args[0])?;
                        let file_ptr_ty = ctx.context.ptr_type(inkwell::AddressSpace::default());

                        // 根据参数类型判断
                        if arg_val.is_int_value() {
                            // 整数：使用 fprintf(stderr, "%d\n", val)
                            let fprintf_fn = super::runtime::declare_fprintf(ctx)?;
                            let stderr_ptr = super::runtime::get_stderr(ctx)?;
                            let stderr_val = ctx.builder.build_load(file_ptr_ty, stderr_ptr, "stderr")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            let fmt_str = ctx.builder.build_global_string_ptr("%d\n", "int_fmt_err_ln")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            ctx.builder.build_call(fprintf_fn, &[stderr_val.into(), fmt_str.as_pointer_value().into(), arg_val.into()], "eprintln_int")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                        } else {
                            // 字符串：使用 fprintf(stderr, "%s\n", str)
                            let fprintf_fn = super::runtime::declare_fprintf(ctx)?;
                            let stderr_ptr = super::runtime::get_stderr(ctx)?;
                            let stderr_val = ctx.builder.build_load(file_ptr_ty, stderr_ptr, "stderr")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            let str_ptr = extract_c_string_ptr(ctx, arg_val)?;
                            let fmt_str = ctx.builder.build_global_string_ptr("%s\n", "str_fmt_err_ln")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // 检查字符串指针是否为 null
                            let null_ptr = ctx.context.ptr_type(inkwell::AddressSpace::default()).const_null();
                            let is_null = ctx.builder.build_int_compare(
                                inkwell::IntPredicate::EQ,
                                str_ptr,
                                null_ptr,
                                "is_null_str"
                            ).map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            let then_bb = ctx.context.append_basic_block(current_fn, "eprintln_null");
                            let else_bb = ctx.context.append_basic_block(current_fn, "eprintln_ok");
                            let cont_bb = ctx.context.append_basic_block(current_fn, "eprintln_cont");

                            ctx.builder.build_conditional_branch(is_null, then_bb, else_bb)
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // Null 分支：仅打印换行
                            ctx.builder.position_at_end(then_bb);
                            let newline_str = ctx.builder.build_global_string_ptr("\n", "newline_err")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            let stderr_val_then = ctx.builder.build_load(file_ptr_ty, stderr_ptr, "stderr")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            let fmt_newline = ctx.builder.build_global_string_ptr("%s", "str_fmt_newline")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            ctx.builder.build_call(fprintf_fn, &[stderr_val_then.into(), fmt_newline.as_pointer_value().into(), newline_str.as_pointer_value().into()], "eprintln_empty")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            ctx.builder.build_unconditional_branch(cont_bb)
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // 正常分支：打印字符串和换行到 stderr
                            ctx.builder.position_at_end(else_bb);
                            ctx.builder.build_call(fprintf_fn, &[stderr_val.into(), fmt_str.as_pointer_value().into(), str_ptr.into()], "eprintln")
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                            ctx.builder.build_unconditional_branch(cont_bb)
                                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                            // 继续执行
                            ctx.builder.position_at_end(cont_bb);
                        }

                        return Ok(());
                    }
                    _ => {
                        // 其他内置函数或用户定义函数，继续常规处理
                    }
                }
            }

            // 常规函数调用
            // 计算函数操作数
            let func_val = super::rvalue::codegen_operand(ctx, locals, local_types, func)?;
            let func_ptr = func_val.into_pointer_value();

            // 计算参数
            let arg_vals: Result<Vec<_>, _> = args
                .iter()
                .map(|arg| super::rvalue::codegen_operand(ctx, locals, local_types, arg).map(|v| v.into()))
                .collect();
            let arg_vals = arg_vals?;

            // 需要从 HirOperand 中提取函数类型信息
            // 对于 FuncRef，从 ctx.functions 获取 FunctionValue 并提取类型
            let fn_type = match func {
                HirOperand::FuncRef(func_id) => {
                    // 从 HIR 模块中查找函数实际名称
                    let hir = ctx.hir_module
                        .ok_or_else(|| CodegenError::BuildError("HIR module not set".to_string()))?;
                    let func_name = &hir.functions.get(func_id.0)
                        .ok_or_else(|| CodegenError::SymbolNotFound(format!("func_{}", func_id.0)))?
                        .name;
                    let fn_val = ctx.functions.get(func_name)
                        .ok_or_else(|| CodegenError::SymbolNotFound(func_name.clone()))?;
                    fn_val.get_type()
                }
                _ => {
                    // 对于其他情况（如函数指针变量），需要从 HIR 类型系统获取
                    return Err(CodegenError::BuildError(
                        "indirect call with non-FuncRef operand not yet supported".to_string()
                    ));
                }
            };

            // 调用函数
            let call_site = ctx.builder.build_indirect_call(
                fn_type,
                func_ptr,
                &arg_vals,
                "call"
            ).map_err(|e| CodegenError::BuildError(e.to_string()))?;

            // 存储返回值（如果有）
            if let Some(dest_place) = dest {
                if let Some(ret_val) = call_site.try_as_basic_value().basic() {
                    let dest_ptr = super::place::codegen_place(ctx, locals, local_types, dest_place)?;
                    ctx.builder.build_store(dest_ptr, ret_val)
                        .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                }
            }

            Ok(())
        }

        HirStmt::Drop { place, .. } => {
            use crate::middleend::hir::HirPlace;

            // 获取被 drop 的变量类型
            let local_id = match place {
                HirPlace::Local(id) => *id,
                _ => {
                    // 当前只处理局部变量的 drop
                    return Err(CodegenError::BuildError(
                        "Drop of non-local place not yet supported".to_string()
                    ));
                }
            };

            // 从 local_types 获取类型
            let hir_ty = &local_types[local_id.0].ty;

            // 检查是否为 owned 指针
            use crate::middleend::hir::ty::HirType;
            match hir_ty {
                HirType::Ptr { owned: true, .. } => {
                    // 生成 free() 调用
                    let free_fn = super::runtime::declare_free(ctx)
                        .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                    // 加载指针值
                    let place_ptr = super::place::codegen_place(ctx, locals, local_types, place)?;
                    let ptr_ty = ctx.context.ptr_type(inkwell::AddressSpace::default());
                    let ptr_val = ctx.builder.build_load(ptr_ty, place_ptr, "drop.ptr")
                        .map_err(|e| CodegenError::BuildError(e.to_string()))?
                        .into_pointer_value();

                    // 转换为 i8* (LLVM 15+ 所有指针类型相同，cast 变为 no-op)
                    let i8_ptr_ty = ctx.context.ptr_type(inkwell::AddressSpace::default());
                    let ptr_val_casted = ctx.builder.build_pointer_cast(
                        ptr_val,
                        i8_ptr_ty,
                        "drop.cast"
                    ).map_err(|e| CodegenError::BuildError(e.to_string()))?;

                    // 调用 free
                    ctx.builder.build_call(free_fn, &[ptr_val_casted.into()], "drop.free")
                        .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                    Ok(())
                }
                _ => {
                    // 非 owned 指针，无需析构
                    Ok(())
                }
            }
        }
    }
}
