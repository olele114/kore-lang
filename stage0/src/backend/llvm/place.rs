//! HIR Place（左值）地址计算。
//!
//! 将 HirPlace 转换为 LLVM 指针，用于后续的 load/store 操作。

use inkwell::values::PointerValue;
use std::collections::HashMap;

use crate::middleend::hir::{HirPlace, LocalId};
use crate::middleend::hir::ty::HirType;
use super::{CodegenContext, CodegenError};

/// 获取 place 的 HIR 类型
fn get_place_type(
    place: &HirPlace,
    local_types: &[crate::middleend::hir::HirLocal],
    hir_module: Option<&crate::middleend::hir::HirModule>,
) -> Option<HirType> {
    match place {
        HirPlace::Local(local_id) => {
            local_types.get(local_id.0).map(|l| l.ty.clone())
        }
        HirPlace::Field { base, field } => {
            let base_ty = get_place_type(base, local_types, hir_module)?;
            match base_ty {
                HirType::Struct(struct_id) => {
                    // 从 HIR 模块查询结构体定义（StructId 是索引）
                    hir_module.and_then(|hir| {
                        hir.structs.get(struct_id.0)
                            .and_then(|s| s.fields.get(*field))
                            .map(|f| f.ty.clone())
                    })
                }
                HirType::ErrUnion { ok, err } => {
                    // ErrUnion 布局：{ i64 tag, payload }
                    match field {
                        0 => Some(HirType::Int { width: 64, signed: false }),
                        1 => Some(*ok),
                        2 => Some(*err),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        HirPlace::Index { base, .. } => {
            let base_ty = get_place_type(base, local_types, hir_module)?;
            match base_ty {
                HirType::Array { elem, .. } => Some(*elem),
                HirType::Slice { elem } => Some(*elem),
                _ => None,
            }
        }
        HirPlace::Deref(base) => {
            let base_ty = get_place_type(base, local_types, hir_module)?;
            match base_ty {
                HirType::Ptr { pointee, .. } => Some(*pointee),
                _ => None,
            }
        }
    }
}

/// 计算 place 的地址（返回指针）
pub fn codegen_place<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
    locals: &HashMap<LocalId, PointerValue<'ctx>>,
    local_types: &[crate::middleend::hir::HirLocal],
    place: &HirPlace,
) -> Result<PointerValue<'ctx>, CodegenError> {
    match place {
        HirPlace::Local(local_id) => {
            // 基础局部变量
            locals.get(local_id)
                .copied()
                .ok_or_else(|| CodegenError::SymbolNotFound(format!("local {:?}", local_id)))
        }

        HirPlace::Field { base, field } => {
            // 结构体/ErrUnion 字段访问：GEP
            let base_ptr = codegen_place(ctx, locals, local_types, base)?;

            // 获取 base 的 HIR 类型，转为 LLVM 类型
            let base_hir_ty = get_place_type(base, local_types, ctx.hir_module)
                .ok_or_else(|| CodegenError::TypeConversion("Cannot determine base type for field access".to_string()))?;
            let base_llvm_ty = ctx.convert_type(&base_hir_ty)?;

            let zero = ctx.context.i32_type().const_zero();
            let field_idx = ctx.context.i32_type().const_int(*field as u64, false);

            let ptr = unsafe {
                ctx.builder.build_in_bounds_gep(
                    base_llvm_ty,
                    base_ptr,
                    &[zero, field_idx],
                    "field"
                )
            }.map_err(|e| CodegenError::BuildError(e.to_string()))?;

            Ok(ptr)
        }

        HirPlace::Index { base, index } => {
            // 数组/切片索引访问
            let base_ptr = codegen_place(ctx, locals, local_types, base)?;
            let index_val = super::rvalue::codegen_operand(ctx, locals, local_types, index)?;
            let index_int = index_val.into_int_value();

            // 获取基础类型
            let base_hir_ty = get_place_type(base, local_types, ctx.hir_module)
                .ok_or_else(|| CodegenError::TypeConversion("Cannot determine base type for index".to_string()))?;

            match base_hir_ty {
                HirType::Array { .. } => {
                    // 数组索引：GEP [0, index]
                    let base_llvm_ty = ctx.convert_type(&base_hir_ty)?;
                    let zero = ctx.context.i32_type().const_zero();

                    let ptr = unsafe {
                        ctx.builder.build_in_bounds_gep(
                            base_llvm_ty,
                            base_ptr,
                            &[zero, index_int],
                            "array_index"
                        )
                    }.map_err(|e| CodegenError::BuildError(e.to_string()))?;

                    Ok(ptr)
                }
                HirType::Slice { ref elem } => {
                    // 切片索引：先加载切片结构体 {ptr, len}，然后 GEP
                    // 切片结构体布局: { i8*, i64 }
                    let slice_struct_ty = ctx.convert_type(&base_hir_ty)?;

                    // 加载切片结构体
                    let slice_val = ctx.builder.build_load(slice_struct_ty, base_ptr, "slice_load")
                        .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                    // 提取 ptr 字段（索引 0）
                    let data_ptr = ctx.builder.build_extract_value(slice_val.into_struct_value(), 0, "slice_ptr")
                        .map_err(|e| CodegenError::BuildError(e.to_string()))?
                        .into_pointer_value();

                    // 计算元素类型
                    let elem_llvm_ty = ctx.convert_type(elem)?;

                    // GEP: data_ptr[index]
                    let ptr = unsafe {
                        ctx.builder.build_in_bounds_gep(
                            elem_llvm_ty,
                            data_ptr,
                            &[index_int],
                            "slice_index"
                        )
                    }.map_err(|e| CodegenError::BuildError(e.to_string()))?;

                    Ok(ptr)
                }
                _ => {
                    Err(CodegenError::TypeConversion(
                        format!("Cannot index into type: {:?}", base_hir_ty)
                    ))
                }
            }
        }

        HirPlace::Deref(base) => {
            // 解引用：先计算 base place，得到指针值，再加载指针本身
            let base_ptr = codegen_place(ctx, locals, local_types, base)?;
            let ty = base_ptr.get_type();
            let ptr_val = ctx.builder.build_load(ty, base_ptr, "deref_load")
                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

            let ptr = ptr_val.into_pointer_value();
            Ok(ptr)
        }
    }
}
