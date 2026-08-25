//! HIR 右值（Rvalue）和操作数（Operand）代码生成。
//!
//! 生成产生值的表达式：二元运算、一元运算、聚合体、类型转换等。

use inkwell::values::{PointerValue, BasicValueEnum};
use inkwell::IntPredicate;
use std::collections::HashMap;

use crate::middleend::hir::{
    HirRvalue, HirOperand, HirLocal, LocalId, BinOp, UnOp, Const, AggregateKind, HirPlace,
    ty::HirType,
};
use super::{CodegenContext, CodegenError};
use super::place::codegen_place;

/// 推断 place 的 HIR 类型
fn infer_place_type(
    place: &HirPlace,
    local_types: &[HirLocal],
    ctx: &CodegenContext,
) -> Result<HirType, CodegenError> {
    match place {
        HirPlace::Local(id) => {
            Ok(local_types[id.0].ty.clone())
        }
        HirPlace::Field { base, field } => {
            let base_ty = infer_place_type(base, local_types, ctx)?;
            match base_ty {
                HirType::Struct(struct_id) => {
                    // 从 HIR 模块查询结构体定义
                    let hir_module = ctx.hir_module
                        .ok_or_else(|| CodegenError::TypeConversion("HIR module not set".to_string()))?;
                    let struct_def = hir_module.structs.get(struct_id.0)
                        .ok_or_else(|| CodegenError::SymbolNotFound(format!("struct_{}", struct_id.0)))?;

                    struct_def.fields.get(*field)
                        .map(|f| f.ty.clone())
                        .ok_or_else(|| CodegenError::TypeConversion(format!("field {} not found", field)))
                }
                HirType::ErrUnion { ok, err } => {
                    // ErrUnion 布局：{ i64 tag, payload }
                    // field 0 = tag，field 1 = ok 载荷，field 2 = err 载荷
                    match field {
                        0 => Ok(HirType::Int { width: 64, signed: false }),
                        1 => Ok(*ok),
                        2 => Ok(*err),
                        _ => Err(CodegenError::TypeConversion(format!(
                            "invalid ErrUnion field index {}", field
                        ))),
                    }
                }
                _ => Err(CodegenError::TypeConversion("field access on non-struct".to_string()))
            }
        }
        HirPlace::Index { base, .. } => {
            let base_ty = infer_place_type(base, local_types, ctx)?;
            match base_ty {
                HirType::Array { elem, .. } => Ok(*elem),
                HirType::Slice { elem } => Ok(*elem),
                HirType::Ptr { pointee, .. } => Ok(*pointee),
                _ => Err(CodegenError::TypeConversion("index on non-array/ptr".to_string()))
            }
        }
        HirPlace::Deref(base) => {
            let base_ty = infer_place_type(base, local_types, ctx)?;
            match base_ty {
                HirType::Ptr { pointee, .. } => Ok(*pointee),
                _ => Err(CodegenError::TypeConversion("deref on non-pointer".to_string()))
            }
        }
    }
}

/// 生成操作数（返回值）
pub fn codegen_operand<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
    locals: &HashMap<LocalId, PointerValue<'ctx>>,
    local_types: &[HirLocal],
    operand: &HirOperand,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    match operand {
        HirOperand::Place(place) => {
            // 获取 place 的地址并加载
            let ptr = super::place::codegen_place(ctx, locals, local_types, place)?;

            let hir_type = infer_place_type(place, local_types, ctx)?;
            let load_type = ctx.convert_type(&hir_type)?;

            ctx.builder.build_load(load_type, ptr, "load")
                .map_err(|e| CodegenError::BuildError(e.to_string()))
        }

        HirOperand::Const(c) => {
            codegen_const(ctx, c)
        }

        HirOperand::FuncRef(func_id) => {
            // 从 HIR 模块中查找函数实际名称
            let hir = ctx.hir_module
                .ok_or_else(|| CodegenError::BuildError("HIR module not set".to_string()))?;
            let func_name = &hir.functions.get(func_id.0)
                .ok_or_else(|| CodegenError::SymbolNotFound(format!("function index {}", func_id.0)))?
                .name;
            ctx.functions.get(func_name)
                .ok_or_else(|| CodegenError::SymbolNotFound(func_name.clone()))
                .map(|f| (*f).as_global_value().as_pointer_value().into())
        }
    }
}

/// 生成常量值
fn codegen_const<'ctx>(
    ctx: &CodegenContext<'ctx>,
    constant: &Const,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    match constant {
        Const::Void => Err(CodegenError::TypeConversion("void has no value".to_string())),

        Const::Bool(val) => {
            let bool_ty = ctx.context.bool_type();
            let const_val = bool_ty.const_int(*val as u64, false);
            Ok(const_val.into())
        }

        Const::Int(val) => {
            // 默认使用 i32（与 lower/expr.rs 的类型推断一致）
            let int_ty = ctx.context.i32_type();
            let const_val = int_ty.const_int(*val as u64, true);
            Ok(const_val.into())
        }

        Const::Float(val) => {
            // 默认使用 f64
            let float_ty = ctx.context.f64_type();
            let const_val = float_ty.const_float(*val);
            Ok(const_val.into())
        }

        Const::Str(s) => {
            // 字符串常量：创建全局字符串 + {ptr, len} 结构体
            // 使用 true 参数确保字符串以 null 结尾，以便与 C 运行时兼容
            let str_val = ctx.context.const_string(s.as_bytes(), true);
            let global = ctx.module.add_global(str_val.get_type(), None, ".str");
            global.set_initializer(&str_val);
            global.set_constant(true);

            // 构造 {ptr, len} 结构体
            let ptr = global.as_pointer_value();
            // 长度不包括 null 终止符
            let len = ctx.context.i64_type().const_int(s.len() as u64, false);

            // 创建结构体类型
            let i8_ptr = ctx.context.ptr_type(inkwell::AddressSpace::default());
            let struct_ty = ctx.context.struct_type(&[i8_ptr.into(), ctx.context.i64_type().into()], false);

            // 构造常量结构体
            let str_struct = struct_ty.const_named_struct(&[ptr.into(), len.into()]);
            Ok(str_struct.into())
        }

        Const::Nil => {
            // Nil 表示空指针
            let ptr_ty = ctx.context.ptr_type(inkwell::AddressSpace::default());
            Ok(ptr_ty.const_null().into())
        }
    }
}

/// 生成右值
pub fn codegen_rvalue<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
    locals: &HashMap<LocalId, PointerValue<'ctx>>,
    local_types: &[HirLocal],
    rvalue: &HirRvalue,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    match rvalue {
        HirRvalue::Use(operand) => {
            codegen_operand(ctx, locals, local_types, operand)
        }

        HirRvalue::BinaryOp { op, lhs, rhs } => {
            let lhs_val = codegen_operand(ctx, locals, local_types, lhs)?;
            let rhs_val = codegen_operand(ctx, locals, local_types, rhs)?;
            codegen_binop(ctx, *op, lhs_val, rhs_val)
        }

        HirRvalue::UnaryOp { op, operand } => {
            let val = codegen_operand(ctx, locals, local_types, operand)?;
            codegen_unop(ctx, *op, val)
        }

        HirRvalue::Ref { place, owned: _ } => {
            // 取地址：直接返回 place 的指针
            let ptr = super::place::codegen_place(ctx, locals, local_types, place)?;
            Ok(ptr.into())
        }

        HirRvalue::Deref(operand) => {
            // 解引用：加载指针指向的值
            let ptr_val = codegen_operand(ctx, locals, local_types, operand)?;
            let ptr = ptr_val.into_pointer_value();
            let ty = ptr.get_type();
            ctx.builder.build_load(ty, ptr, "deref")
                .map_err(|e| CodegenError::BuildError(e.to_string()))
        }

        HirRvalue::Aggregate { kind, fields } => {
            codegen_aggregate(ctx, locals, local_types, kind, fields)
        }

        HirRvalue::Discriminant(place) => {
            // 提取联合体判别式（discriminant 字段）
            let ptr = super::place::codegen_place(ctx, locals, local_types, place)?;

            // 推断 place 的 HIR 类型
            let place_hir_ty = infer_place_type(place, local_types, ctx)?;

            // 转换为 LLVM 类型（联合体的结构体类型）
            let union_llvm_ty = ctx.convert_type(&place_hir_ty)?;

            // 联合体布局：{ i32 discriminant, payload }
            // 提取第 0 个字段（discriminant）
            let zero = ctx.context.i32_type().const_zero();
            let field_idx = ctx.context.i32_type().const_zero();

            let disc_ptr = unsafe {
                ctx.builder.build_in_bounds_gep(
                    union_llvm_ty,
                    ptr,
                    &[zero, field_idx],
                    "disc_ptr"
                )
            }.map_err(|e| CodegenError::BuildError(e.to_string()))?;

            let disc_ty = ctx.context.i32_type();
            let disc_val = ctx.builder
                .build_load(disc_ty, disc_ptr, "disc")
                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

            Ok(disc_val)
        }

        HirRvalue::ExtractPayload { place, variant_index } => {
            crate::trace!("=== ExtractPayload: place={:?}, variant_index={}", place, variant_index);

            // 提取联合体 payload（假设已验证 tag）
            let ptr = super::place::codegen_place(ctx, locals, local_types, place)?;
            crate::trace!("  Got place pointer: {:?}", ptr);

            // 定义 infer_place_type 辅助函数
            fn infer_place_type_simple(
                place: &crate::middleend::hir::HirPlace,
                local_types: &[crate::middleend::hir::HirLocal],
            ) -> Option<crate::middleend::hir::ty::HirType> {
                match place {
                    crate::middleend::hir::HirPlace::Local(id) => {
                        Some(local_types.get(id.0)?.ty.clone())
                    }
                    _ => None, // 简化版本，仅处理直接局部变量
                }
            }

            // 获取 place 的 HIR 类型以确定联合体定义
            let place_ty = infer_place_type_simple(place, local_types);
            crate::trace!("  Inferred place type: {:?}", place_ty);

            // 根据联合体定义查询 payload 类型
            let payload_hir_ty = match place_ty {
                Some(HirType::Union(union_id)) => {
                    crate::trace!("  Union ID: {:?}", union_id);
                    crate::trace!("  HIR module available: {}", ctx.hir_module.is_some());

                    // 从 HIR 模块查询联合体定义
                    let result = ctx.hir_module
                        .and_then(|hir| {
                            crate::trace!("  Unions count: {}", hir.unions.len());
                            hir.unions.get(union_id.0)
                        })
                        .and_then(|union_def| {
                            crate::trace!("  Union variants count: {}", union_def.variants.len());
                            union_def.variants.get(*variant_index)
                        })
                        .and_then(|variant| {
                            crate::trace!("  Variant payload: {:?}", variant.payload);
                            variant.payload.clone()
                        });

                    crate::trace!("  Final payload type: {:?}", result);
                    result
                }
                Some(HirType::ErrUnion { ref ok, ref err }) => {
                    crate::trace!("  ErrUnion: ok={:?}, err={:?}, variant_index={}", ok, err, variant_index);
                    // variant_index 0 = Ok, 1 = Err
                    Some(if *variant_index == 0 {
                        ok.as_ref().clone()
                    } else {
                        err.as_ref().clone()
                    })
                }
                _ => {
                    crate::trace!("  Not a union or error union type");
                    None
                }
            };

            // 联合体布局：{ i32 discriminant, payload }
            // 提取第 1 个字段（payload）
            let zero = ctx.context.i32_type().const_zero();
            let field_idx = ctx.context.i32_type().const_int(1, false);

            crate::trace!("  Building GEP for payload...");

            // 获取联合体的 LLVM 类型
            let union_llvm_ty = match place_ty {
                Some(HirType::Union(_)) | Some(HirType::ErrUnion { .. }) => {
                    ctx.convert_type(place_ty.as_ref().unwrap())?
                }
                _ => {
                    return Err(CodegenError::TypeConversion("ExtractPayload requires Union or ErrUnion type".to_string()));
                }
            };

            let payload_ptr = unsafe {
                ctx.builder.build_in_bounds_gep(
                    union_llvm_ty,
                    ptr,
                    &[zero, field_idx],
                    "payload_ptr"
                )
            }.map_err(|e| CodegenError::BuildError(e.to_string()))?;
            crate::trace!("  Payload pointer: {:?}", payload_ptr);

            // 根据 variant 的 payload 类型加载
            if let Some(hir_ty) = payload_hir_ty {
                crate::trace!("  Converting HIR type to LLVM: {:?}", hir_ty);
                let payload_llvm_ty = ctx.convert_type(&hir_ty)?;
                crate::trace!("  LLVM type: {:?}", payload_llvm_ty);

                // 对于 ErrUnion，payload 是字节数组，需要 bitcast 到目标类型的指针
                let typed_ptr = if matches!(place_ty, Some(HirType::ErrUnion { .. })) {
                    crate::trace!("  Bitcasting byte array to typed pointer...");
                    ctx.builder.build_pointer_cast(
                        payload_ptr,
                        ctx.context.ptr_type(inkwell::AddressSpace::default()),
                        "typed_ptr"
                    ).map_err(|e| CodegenError::BuildError(e.to_string()))?
                } else {
                    payload_ptr
                };

                crate::trace!("  Building load...");
                let payload_val = ctx.builder
                    .build_load(payload_llvm_ty, typed_ptr, "payload")
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                crate::trace!("  Loaded payload: {:?}", payload_val);
                Ok(payload_val)
            } else {
                crate::trace!("  No payload - returning zero");
                // 无 payload 的变体（如 .None），返回 unit 值
                Ok(ctx.context.i32_type().const_zero().into())
            }
        }

        HirRvalue::ArrayToSlice { array, elem_ty, len } => {
            // 数组转切片：获取数组指针和长度，构造切片结构
            // 注意：我们需要数组的地址，而不是加载数组的值
            let array_ptr = match array {
                HirOperand::Place(place) => {
                    // 获取 place 的地址（不加载）
                    codegen_place(ctx, locals, local_types, place)?
                }
                _ => {
                    return Err(CodegenError::BuildError(
                        "ArrayToSlice expects a Place operand".to_string()
                    ));
                }
            };

            // 将数组指针转换为元素指针
            let _elem_llvm_ty = ctx.convert_type(elem_ty)?;
            let slice_elem_ptr = ctx.builder
                .build_pointer_cast(
                    array_ptr,
                    ctx.context.ptr_type(inkwell::AddressSpace::default()),
                    "slice_ptr",
                )
                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

            // 使用提供的长度字段
            let len_val = ctx.context.i64_type().const_int(*len as u64, false);

            // 构造切片结构 {ptr, len}
            let slice_ty = ctx.context.struct_type(
                &[
                    ctx.context.ptr_type(inkwell::AddressSpace::default()).into(),
                    ctx.context.i64_type().into(),
                ],
                false,
            );

            let mut slice_val = slice_ty.get_undef();
            slice_val = ctx.builder
                .build_insert_value(slice_val, slice_elem_ptr, 0, "slice.ptr")
                .map_err(|e| CodegenError::BuildError(e.to_string()))?
                .into_struct_value();
            slice_val = ctx.builder
                .build_insert_value(slice_val, len_val, 1, "slice.len")
                .map_err(|e| CodegenError::BuildError(e.to_string()))?
                .into_struct_value();

            Ok(slice_val.into())
        }
    }
}

/// 生成二元运算
fn codegen_binop<'ctx>(
    ctx: &CodegenContext<'ctx>,
    op: BinOp,
    lhs: BasicValueEnum<'ctx>,
    rhs: BasicValueEnum<'ctx>,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    crate::trace!("DEBUG codegen_binop: op={:?}, lhs_type={:?}, rhs_type={:?}",
              op, lhs.get_type(), rhs.get_type());

    // 整数运算
    if lhs.is_int_value() && rhs.is_int_value() {
        let lhs = lhs.into_int_value();
        let rhs = rhs.into_int_value();

        let result = match op {
            BinOp::Add => ctx.builder.build_int_add(lhs, rhs, "add"),
            BinOp::Sub => ctx.builder.build_int_sub(lhs, rhs, "sub"),
            BinOp::Mul => ctx.builder.build_int_mul(lhs, rhs, "mul"),
            BinOp::Div => ctx.builder.build_int_signed_div(lhs, rhs, "div"),
            BinOp::Rem => ctx.builder.build_int_signed_rem(lhs, rhs, "rem"),

            BinOp::BitAnd => ctx.builder.build_and(lhs, rhs, "and"),
            BinOp::BitOr => ctx.builder.build_or(lhs, rhs, "or"),
            BinOp::BitXor => ctx.builder.build_xor(lhs, rhs, "xor"),
            BinOp::Shl => ctx.builder.build_left_shift(lhs, rhs, "shl"),
            BinOp::Shr => ctx.builder.build_right_shift(lhs, rhs, true, "shr"),

            BinOp::Eq => ctx.builder.build_int_compare(IntPredicate::EQ, lhs, rhs, "eq"),
            BinOp::Ne => ctx.builder.build_int_compare(IntPredicate::NE, lhs, rhs, "ne"),
            BinOp::Lt => ctx.builder.build_int_compare(IntPredicate::SLT, lhs, rhs, "lt"),
            BinOp::Le => ctx.builder.build_int_compare(IntPredicate::SLE, lhs, rhs, "le"),
            BinOp::Gt => ctx.builder.build_int_compare(IntPredicate::SGT, lhs, rhs, "gt"),
            BinOp::Ge => ctx.builder.build_int_compare(IntPredicate::SGE, lhs, rhs, "ge"),

            _ => return Err(CodegenError::BuildError(format!("unsupported int binop: {:?}", op))),
        }.map_err(|e| CodegenError::BuildError(e.to_string()))?;

        return Ok(result.into());
    }

    // 浮点运算
    if lhs.is_float_value() && rhs.is_float_value() {
        let lhs = lhs.into_float_value();
        let rhs = rhs.into_float_value();

        let result = match op {
            BinOp::Add => ctx.builder.build_float_add(lhs, rhs, "fadd"),
            BinOp::Sub => ctx.builder.build_float_sub(lhs, rhs, "fsub"),
            BinOp::Mul => ctx.builder.build_float_mul(lhs, rhs, "fmul"),
            BinOp::Div => ctx.builder.build_float_div(lhs, rhs, "fdiv"),
            BinOp::Rem => ctx.builder.build_float_rem(lhs, rhs, "frem"),

            _ => return Err(CodegenError::BuildError(format!("unsupported float binop: {:?}", op))),
        }.map_err(|e| CodegenError::BuildError(e.to_string()))?;

        return Ok(result.into());
    }

    Err(CodegenError::BuildError("type mismatch in binop".to_string()))
}

/// 生成一元运算
fn codegen_unop<'ctx>(
    ctx: &CodegenContext<'ctx>,
    op: UnOp,
    val: BasicValueEnum<'ctx>,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    match op {
        UnOp::Neg => {
            if val.is_int_value() {
                let result = ctx.builder.build_int_neg(val.into_int_value(), "neg")
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                Ok(result.into())
            } else if val.is_float_value() {
                let result = ctx.builder.build_float_neg(val.into_float_value(), "fneg")
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                Ok(result.into())
            } else {
                Err(CodegenError::BuildError("neg expects int or float".to_string()))
            }
        }

        UnOp::Not => {
            if val.is_int_value() {
                let result = ctx.builder.build_not(val.into_int_value(), "not")
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                Ok(result.into())
            } else {
                Err(CodegenError::BuildError("not expects int".to_string()))
            }
        }

        UnOp::BitNot => {
            if val.is_int_value() {
                let result = ctx.builder.build_not(val.into_int_value(), "bitnot")
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                Ok(result.into())
            } else {
                Err(CodegenError::BuildError("bitnot expects int".to_string()))
            }
        }
    }
}

/// 生成聚合体构造
fn codegen_aggregate<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
    locals: &HashMap<LocalId, PointerValue<'ctx>>,
    local_types: &[HirLocal],
    kind: &AggregateKind,
    fields: &[HirOperand],
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    match kind {
        AggregateKind::Struct(struct_id) => {
            // 获取结构体类型
            let hir_module = ctx.hir_module
                .ok_or_else(|| CodegenError::TypeConversion("HIR module not set".to_string()))?;

            let struct_def = hir_module.structs.get(struct_id.0)
                .ok_or_else(|| CodegenError::SymbolNotFound(format!("struct_{}", struct_id.0)))?;

            // 转换字段类型
            let field_types: Result<Vec<_>, _> = struct_def.fields
                .iter()
                .map(|f| ctx.convert_type(&f.ty))
                .collect();
            let field_types = field_types?;

            let struct_ty = ctx.context.struct_type(&field_types, false);

            // 计算字段值
            let field_values: Result<Vec<_>, _> = fields
                .iter()
                .map(|f| codegen_operand(ctx, locals, local_types, f))
                .collect();
            let field_values = field_values?;

            // 构造结构体值
            let mut struct_val = struct_ty.get_undef();
            for (i, field_val) in field_values.iter().enumerate() {
                struct_val = ctx.builder
                    .build_insert_value(struct_val, *field_val, i as u32, "field")
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?
                    .into_struct_value();
            }

            Ok(struct_val.into())
        }

        AggregateKind::Union(union_id, variant_idx) => {
            // 联合体布局：{ i32 discriminant, payload }
            let discriminant_ty = ctx.context.i32_type();
            let discriminant_val = discriminant_ty.const_int(*variant_idx as u64, false);

            // 获取 payload 类型（通过索引直接访问）
            let hir_module = ctx.hir_module
                .ok_or_else(|| CodegenError::TypeConversion("HIR module not set".to_string()))?;

            let union_def = hir_module.unions.get(union_id.0)
                .ok_or_else(|| CodegenError::SymbolNotFound(format!("union with id {}", union_id.0)))?;

            let variant = union_def.variants.get(*variant_idx)
                .ok_or_else(|| CodegenError::BuildError(format!("Invalid variant index: {}", variant_idx)))?;

            // 获取联合类型的规范 LLVM 类型 { i32, largest_variant_payload }
            let canonical_union_ty = ctx.convert_type(&crate::middleend::hir::ty::HirType::Union(*union_id))?
                .into_struct_type();

            // 提取规范 payload 类型（第二个字段）
            let canonical_payload_ty = canonical_union_ty.get_field_type_at_index(1)
                .ok_or_else(|| CodegenError::BuildError("Union type missing payload field".to_string()))?;

            let payload_val = if let Some(_payload_ty) = &variant.payload {
                // 有 payload：计算值
                if fields.is_empty() {
                    return Err(CodegenError::BuildError("Union variant with payload requires field value".to_string()));
                }
                let val = codegen_operand(ctx, locals, local_types, &fields[0])?;

                // 如果实际 payload 类型与规范类型不同，需要转换
                if val.get_type() != canonical_payload_ty {
                    // 这种情况应该不会发生，但为安全起见做类型检查
                    return Err(CodegenError::BuildError(
                        format!("Payload type mismatch: expected {:?}, got {:?}",
                                canonical_payload_ty, val.get_type())
                    ));
                }
                val
            } else {
                // 无 payload：使用规范 payload 类型的零值
                match canonical_payload_ty {
                    inkwell::types::BasicTypeEnum::IntType(it) => it.const_zero().into(),
                    inkwell::types::BasicTypeEnum::FloatType(ft) => ft.const_zero().into(),
                    inkwell::types::BasicTypeEnum::PointerType(pt) => pt.const_null().into(),
                    inkwell::types::BasicTypeEnum::StructType(st) => st.const_zero().into(),
                    inkwell::types::BasicTypeEnum::ArrayType(at) => at.const_zero().into(),
                    inkwell::types::BasicTypeEnum::VectorType(vt) => vt.const_zero().into(),
                    inkwell::types::BasicTypeEnum::ScalableVectorType(svt) => svt.const_zero().into(),
                }
            };

            // 构造联合体值
            let mut union_val = canonical_union_ty.get_undef();
            union_val = ctx.builder.build_insert_value(union_val, discriminant_val, 0, "union.tag")
                .map_err(|e| CodegenError::BuildError(e.to_string()))?
                .into_struct_value();
            union_val = ctx.builder.build_insert_value(union_val, payload_val, 1, "union.payload")
                .map_err(|e| CodegenError::BuildError(e.to_string()))?
                .into_struct_value();

            Ok(union_val.into())
        }

        AggregateKind::ErrorUnion(variant_idx, declared_ty) => {
            // 错误联合布局：{ i64 tag, [i8 x N] payload }
            // tag: 0 = Ok, 1 = Err
            let tag_ty = ctx.context.i64_type();
            let tag_val = tag_ty.const_int(*variant_idx as u64, false);

            // 计算 payload 值
            let payload_val = if fields.is_empty() {
                return Err(CodegenError::BuildError("ErrorUnion variant requires payload".to_string()));
            } else {
                codegen_operand(ctx, locals, local_types, &fields[0])?
            };

            // payload 槽位大小必须来自声明的错误联合类型，而不是当前变体
            // 实际 payload 值的大小。例如 `i32 ! str`：Ok 侧是 4 字节，Err
            // 侧是 16 字节胖指针；若按实际值取大小，构造 .Err 时会把 16 字节
            // 存进 8 字节 alloca（越界写 + len 截断），且 Ok/Err 两条路径产出
            // 的结构体类型不一致。
            //
            // 直接复用 convert_type 得到的布局，保证与 ty.rs / place.rs /
            // ExtractPayload 使用的是同一个类型。
            // 先确认声明类型确实是错误联合：convert_type 只对 ErrUnion 保证
            // `{ i64, [i8 x N] }` 布局，直接 into_struct_type() 会在类型不符时
            // panic，这里换成可诊断的错误。
            if !matches!(declared_ty, HirType::ErrUnion { .. }) {
                return Err(CodegenError::TypeConversion(format!(
                    "ErrorUnion aggregate declared type must be an error union, got {:?}",
                    declared_ty
                )));
            }
            let declared_struct = ctx.convert_type(declared_ty)?.into_struct_type();
            let byte_array_ty = declared_struct
                .get_field_type_at_index(1)
                .ok_or_else(|| CodegenError::TypeConversion(
                    "ErrUnion layout missing payload field".to_string()
                ))?
                .into_array_type();

            // 分配字节数组临时空间（而非 payload 原始类型）
            let byte_array_ptr = ctx.builder.build_alloca(byte_array_ty, "byte_array_alloca")
                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

            // 将 payload 值存储到字节数组中（通过 bitcast）
            let payload_ptr = ctx.builder.build_pointer_cast(
                byte_array_ptr,
                ctx.context.ptr_type(inkwell::AddressSpace::default()),
                "payload_ptr"
            ).map_err(|e| CodegenError::BuildError(e.to_string()))?;
            ctx.builder.build_store(payload_ptr, payload_val)
                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

            // 加载字节数组
            let byte_array_val = ctx.builder.build_load(byte_array_ty, byte_array_ptr, "byte_array")
                .map_err(|e| CodegenError::BuildError(e.to_string()))?;

            // 直接用声明类型作为结果结构体，确保 Ok/Err 两条路径以及
            // 函数返回类型使用完全相同的 LLVM 类型
            let mut union_val = declared_struct.get_undef();
            union_val = ctx.builder.build_insert_value(union_val, tag_val, 0, "errunion.tag")
                .map_err(|e| CodegenError::BuildError(e.to_string()))?
                .into_struct_value();
            union_val = ctx.builder.build_insert_value(union_val, byte_array_val, 1, "errunion.payload")
                .map_err(|e| CodegenError::BuildError(e.to_string()))?
                .into_struct_value();

            Ok(union_val.into())
        }

        AggregateKind::Array(elem_ty, len) => {
            // 数组构造
            use inkwell::types::BasicType;
            let llvm_elem_ty = ctx.convert_type(elem_ty)?;
            let array_ty = llvm_elem_ty.array_type(*len as u32);

            // 计算元素值
            let elem_values: Result<Vec<_>, _> = fields
                .iter()
                .map(|f| codegen_operand(ctx, locals, local_types, f))
                .collect();
            let elem_values = elem_values?;

            if elem_values.len() != *len {
                return Err(CodegenError::BuildError(
                    format!("Array literal has {} elements, expected {}", elem_values.len(), len)
                ));
            }

            // 构造数组值
            let mut array_val = array_ty.get_undef();
            for (i, elem_val) in elem_values.iter().enumerate() {
                array_val = ctx.builder
                    .build_insert_value(array_val, *elem_val, i as u32, "elem")
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?
                    .into_array_value();
            }

            Ok(array_val.into())
        }
    }
}

/// 生成类型转换
#[allow(dead_code)]
fn codegen_cast<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
    val: BasicValueEnum<'ctx>,
    target_ty: &crate::middleend::hir::ty::HirType,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    use crate::middleend::hir::ty::HirType;

    let target_llvm_ty = ctx.convert_type(target_ty)?;

    // 整数类型转换
    if val.is_int_value() {
        let int_val = val.into_int_value();

        match target_ty {
            HirType::Int { width, signed } => {
                let target_int_ty = match width {
                    8 => ctx.context.i8_type(),
                    16 => ctx.context.i16_type(),
                    32 => ctx.context.i32_type(),
                    64 => ctx.context.i64_type(),
                    _ => return Err(CodegenError::TypeConversion(format!("Unsupported int width: {}", width))),
                };

                let source_width = int_val.get_type().get_bit_width();
                let target_width = *width as u32;

                let result = if source_width < target_width {
                    // 扩展
                    if *signed {
                        ctx.builder.build_int_s_extend(int_val, target_int_ty, "sext")
                    } else {
                        ctx.builder.build_int_z_extend(int_val, target_int_ty, "zext")
                    }
                } else if source_width > target_width {
                    // 截断
                    ctx.builder.build_int_truncate(int_val, target_int_ty, "trunc")
                } else {
                    // 相同位宽，直接返回
                    return Ok(int_val.into());
                };

                result.map(|v| v.into()).map_err(|e| CodegenError::BuildError(e.to_string()))
            }

            HirType::Float { width } => {
                // 整数转浮点
                let target_float_ty = match width {
                    32 => ctx.context.f32_type(),
                    64 => ctx.context.f64_type(),
                    _ => return Err(CodegenError::TypeConversion(format!("Unsupported float width: {}", width))),
                };

                ctx.builder.build_signed_int_to_float(int_val, target_float_ty, "sitofp")
                    .map(|v| v.into())
                    .map_err(|e| CodegenError::BuildError(e.to_string()))
            }

            HirType::Bool => {
                // 整数转布尔（非零为 true）
                let zero = int_val.get_type().const_zero();
                ctx.builder.build_int_compare(
                    inkwell::IntPredicate::NE,
                    int_val,
                    zero,
                    "tobool"
                ).map(|v| v.into()).map_err(|e| CodegenError::BuildError(e.to_string()))
            }

            HirType::Ptr { .. } => {
                // 整数转指针
                ctx.builder.build_int_to_ptr(
                    int_val,
                    ctx.context.ptr_type(inkwell::AddressSpace::default()),
                    "inttoptr"
                ).map(|v| v.into()).map_err(|e| CodegenError::BuildError(e.to_string()))
            }

            _ => Err(CodegenError::TypeConversion(format!("Cannot cast int to {:?}", target_ty)))
        }
    }
    // 浮点类型转换
    else if val.is_float_value() {
        let float_val = val.into_float_value();

        match target_ty {
            HirType::Float { width } => {
                let target_float_ty = match width {
                    32 => ctx.context.f32_type(),
                    64 => ctx.context.f64_type(),
                    _ => return Err(CodegenError::TypeConversion(format!("Unsupported float width: {}", width))),
                };

                let source_width = if float_val.get_type() == ctx.context.f32_type() { 32 } else { 64 };
                let target_width = *width;

                let result = if source_width < target_width {
                    // f32 -> f64
                    ctx.builder.build_float_ext(float_val, target_float_ty, "fpext")
                } else if source_width > target_width {
                    // f64 -> f32
                    ctx.builder.build_float_trunc(float_val, target_float_ty, "fptrunc")
                } else {
                    return Ok(float_val.into());
                };

                result.map(|v| v.into()).map_err(|e| CodegenError::BuildError(e.to_string()))
            }

            HirType::Int { width, signed } => {
                // 浮点转整数
                let target_int_ty = match width {
                    8 => ctx.context.i8_type(),
                    16 => ctx.context.i16_type(),
                    32 => ctx.context.i32_type(),
                    64 => ctx.context.i64_type(),
                    _ => return Err(CodegenError::TypeConversion(format!("Unsupported int width: {}", width))),
                };

                if *signed {
                    ctx.builder.build_float_to_signed_int(float_val, target_int_ty, "fptosi")
                } else {
                    ctx.builder.build_float_to_unsigned_int(float_val, target_int_ty, "fptoui")
                }.map(|v| v.into()).map_err(|e| CodegenError::BuildError(e.to_string()))
            }

            _ => Err(CodegenError::TypeConversion(format!("Cannot cast float to {:?}", target_ty)))
        }
    }
    // 指针类型转换
    else if val.is_pointer_value() {
        let ptr_val = val.into_pointer_value();

        match target_ty {
            HirType::Ptr { .. } => {
                // 指针到指针转换（LLVM opaque pointers 自动处理）
                Ok(ptr_val.into())
            }

            HirType::Int { width, .. } => {
                // 指针转整数
                let target_int_ty = match width {
                    8 => ctx.context.i8_type(),
                    16 => ctx.context.i16_type(),
                    32 => ctx.context.i32_type(),
                    64 => ctx.context.i64_type(),
                    _ => return Err(CodegenError::TypeConversion(format!("Unsupported int width: {}", width))),
                };

                ctx.builder.build_ptr_to_int(ptr_val, target_int_ty, "ptrtoint")
                    .map(|v| v.into())
                    .map_err(|e| CodegenError::BuildError(e.to_string()))
            }

            _ => Err(CodegenError::TypeConversion(format!("Cannot cast pointer to {:?}", target_ty)))
        }
    }
    // 布尔类型转换
    else if matches!(val.get_type(), inkwell::types::BasicTypeEnum::IntType(t) if t.get_bit_width() == 1) {
        let bool_val = val.into_int_value();

        match target_ty {
            HirType::Int { width, .. } => {
                // 布尔转整数（0 或 1）
                let target_int_ty = match width {
                    8 => ctx.context.i8_type(),
                    16 => ctx.context.i16_type(),
                    32 => ctx.context.i32_type(),
                    64 => ctx.context.i64_type(),
                    _ => return Err(CodegenError::TypeConversion(format!("Unsupported int width: {}", width))),
                };

                ctx.builder.build_int_z_extend(bool_val, target_int_ty, "zext")
                    .map(|v| v.into())
                    .map_err(|e| CodegenError::BuildError(e.to_string()))
            }

            HirType::Bool => Ok(bool_val.into()),

            _ => Err(CodegenError::TypeConversion(format!("Cannot cast bool to {:?}", target_ty)))
        }
    }
    else {
        Err(CodegenError::TypeConversion(format!(
            "Unsupported cast from {:?} to {:?}",
            val.get_type(),
            target_llvm_ty
        )))
    }
}
