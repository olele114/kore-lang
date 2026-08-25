//! HIR 类型到 LLVM 类型的转换。
//!
//! 实现带缓存的类型转换，避免重复创建相同的 LLVM 类型。

use inkwell::types::{BasicTypeEnum, BasicMetadataTypeEnum, BasicType};
use inkwell::AddressSpace;

use crate::middleend::hir::ty::HirType;
use super::{CodegenContext, CodegenError};

/// 将 offset 向上对齐到 align 的倍数（align 必须非零）
fn align_to(offset: u64, align: u64) -> u64 {
    if align <= 1 {
        return offset;
    }
    (offset + align - 1) / align * align
}

impl<'ctx> CodegenContext<'ctx> {
    /// 将 HIR 类型转换为 LLVM 类型（带缓存）
    pub fn convert_type(&mut self, ty: &HirType) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
        // 检查缓存
        if let Some(cached) = self.type_cache.get(ty) {
            return Ok(*cached);
        }

        // 转换类型
        let llvm_ty = self.convert_type_uncached(ty)?;

        // 存入缓存
        self.type_cache.insert(ty.clone(), llvm_ty);

        Ok(llvm_ty)
    }

    /// 计算 HIR 类型在目标平台上的字节大小（结构化计算，不依赖 LLVM data layout）
    ///
    /// 代码生成阶段模块还没有 data layout（只在 `backend/linker.rs` 里设置），
    /// 此时 LLVM 的 `size_of()` 对聚合类型只返回符号常量表达式，
    /// `get_zero_extended_constant()` 拿不到数值。因此联合体 / 错误联合的
    /// payload 槽位尺寸必须自己算，且算不出来时报错而不是静默取默认值：
    /// 尺寸偏小会直接产生越界写。
    ///
    /// 假设 64 位目标：指针 8 字节，最大对齐 8 字节。
    pub fn hir_type_size(&self, ty: &HirType) -> Result<u64, CodegenError> {
        Ok(match ty {
            HirType::Bool => 1,
            HirType::Never => 1, // 占位 i8
            HirType::Int { width, .. } | HirType::Float { width } => (*width / 8) as u64,
            HirType::Ptr { .. } | HirType::FnPtr { .. } => 8,
            // 胖指针 { ptr, i64 }
            HirType::Str | HirType::Slice { .. } => 16,

            HirType::Array { elem, len } => {
                let elem_size = self.hir_type_size(elem)?;
                let elem_align = self.hir_type_align(elem)?;
                align_to(elem_size, elem_align) * (*len as u64)
            }

            HirType::Struct(struct_id) => {
                let hir_module = self.hir_module.ok_or_else(|| {
                    CodegenError::TypeConversion("HIR module not set".to_string())
                })?;
                let struct_def = hir_module.structs.get(struct_id.0).ok_or_else(|| {
                    CodegenError::SymbolNotFound(format!("struct_{}", struct_id.0))
                })?;

                let mut offset = 0u64;
                let mut max_align = 1u64;
                for field in &struct_def.fields {
                    let f_align = self.hir_type_align(&field.ty)?;
                    max_align = max_align.max(f_align);
                    offset = align_to(offset, f_align) + self.hir_type_size(&field.ty)?;
                }
                align_to(offset, max_align)
            }

            HirType::Union(union_id) => {
                // 布局 { i32 discriminant, payload }
                let hir_module = self.hir_module.ok_or_else(|| {
                    CodegenError::TypeConversion("HIR module not set".to_string())
                })?;
                let union_def = hir_module.unions.get(union_id.0).ok_or_else(|| {
                    CodegenError::SymbolNotFound(format!("union with id {}", union_id.0))
                })?;

                let mut payload_size = 1u64;
                let mut payload_align = 1u64;
                for variant in &union_def.variants {
                    if let Some(p) = &variant.payload {
                        let size = self.hir_type_size(p)?;
                        if size > payload_size {
                            payload_size = size;
                            payload_align = self.hir_type_align(p)?;
                        }
                    }
                }
                let align = payload_align.max(4);
                align_to(align_to(4, payload_align) + payload_size, align)
            }

            HirType::ErrUnion { ok, err } => {
                // 布局 { i64 tag, [i8 x N] }，payload 是字节数组（对齐 1）
                let payload = self.hir_type_size(ok)?.max(self.hir_type_size(err)?);
                align_to(8 + payload, 8)
            }

            HirType::Void => {
                return Err(CodegenError::TypeConversion(
                    "void type has no size".to_string(),
                ));
            }
        })
    }

    /// 计算 HIR 类型的对齐要求（字节）
    fn hir_type_align(&self, ty: &HirType) -> Result<u64, CodegenError> {
        Ok(match ty {
            HirType::Bool | HirType::Never => 1,
            HirType::Int { width, .. } | HirType::Float { width } => (*width / 8) as u64,
            HirType::Ptr { .. } | HirType::FnPtr { .. } => 8,
            HirType::Str | HirType::Slice { .. } => 8,
            HirType::Array { elem, .. } => self.hir_type_align(elem)?,
            HirType::ErrUnion { .. } => 8, // i64 tag
            HirType::Struct(struct_id) => {
                let hir_module = self.hir_module.ok_or_else(|| {
                    CodegenError::TypeConversion("HIR module not set".to_string())
                })?;
                let struct_def = hir_module.structs.get(struct_id.0).ok_or_else(|| {
                    CodegenError::SymbolNotFound(format!("struct_{}", struct_id.0))
                })?;
                let mut max_align = 1u64;
                for field in &struct_def.fields {
                    max_align = max_align.max(self.hir_type_align(&field.ty)?);
                }
                max_align
            }
            HirType::Union(union_id) => {
                let hir_module = self.hir_module.ok_or_else(|| {
                    CodegenError::TypeConversion("HIR module not set".to_string())
                })?;
                let union_def = hir_module.unions.get(union_id.0).ok_or_else(|| {
                    CodegenError::SymbolNotFound(format!("union with id {}", union_id.0))
                })?;
                let mut max_align = 4u64; // i32 discriminant
                for variant in &union_def.variants {
                    if let Some(p) = &variant.payload {
                        max_align = max_align.max(self.hir_type_align(p)?);
                    }
                }
                max_align
            }
            HirType::Void => {
                return Err(CodegenError::TypeConversion(
                    "void type has no alignment".to_string(),
                ));
            }
        })
    }

    /// 将 HIR 类型转换为 LLVM 类型（无缓存）
    fn convert_type_uncached(&mut self, ty: &HirType) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
        match ty {
            HirType::Bool => Ok(self.context.bool_type().into()),

            HirType::Int { width, .. } => {
                match width {
                    8 => Ok(self.context.i8_type().into()),
                    16 => Ok(self.context.i16_type().into()),
                    32 => Ok(self.context.i32_type().into()),
                    64 => Ok(self.context.i64_type().into()),
                    _ => Err(CodegenError::TypeConversion(format!("unsupported int width: {}", width))),
                }
            }

            HirType::Float { width } => {
                match width {
                    32 => Ok(self.context.f32_type().into()),
                    64 => Ok(self.context.f64_type().into()),
                    _ => Err(CodegenError::TypeConversion(format!("unsupported float width: {}", width))),
                }
            }

            HirType::Str => {
                // 字符串表示为 {ptr, len}
                let i8_ptr = self.context.ptr_type(AddressSpace::default());
                let i64 = self.context.i64_type();
                Ok(self.context.struct_type(&[i8_ptr.into(), i64.into()], false).into())
            }

            HirType::Ptr { pointee, .. } => {
                let _inner_ty = self.convert_type(pointee)?;
                Ok(self.context.ptr_type(AddressSpace::default()).into())
            }

            HirType::Array { elem, len } => {
                let elem_ty = self.convert_type(elem)?;
                Ok(elem_ty.array_type(*len as u32).into())
            }

            HirType::Struct(struct_id) => {
                // 查询结构体定义
                let hir_module = self.hir_module
                    .ok_or_else(|| CodegenError::TypeConversion("HIR module not set".to_string()))?;

                let struct_def = hir_module.structs.get(struct_id.0)
                    .ok_or_else(|| CodegenError::SymbolNotFound(format!("struct_{}", struct_id.0)))?;

                // 转换所有字段类型
                let field_types: Result<Vec<_>, _> = struct_def.fields
                    .iter()
                    .map(|f| self.convert_type(&f.ty))
                    .collect();
                let field_types = field_types?;

                // 创建 LLVM 结构体类型
                Ok(self.context.struct_type(&field_types, false).into())
            }

            HirType::Union(union_id) => {
                // 查询联合体定义（通过索引直接访问）
                let hir_module = self.hir_module
                    .ok_or_else(|| CodegenError::TypeConversion("HIR module not set".to_string()))?;

                let union_def = hir_module.unions.get(union_id.0)
                    .ok_or_else(|| CodegenError::SymbolNotFound(format!("union with id {}", union_id.0)))?;

                // 联合体布局：{ discriminant: i32, payload: largest_variant }
                // 找出最大的变体类型作为 payload
                //
                // 尺寸用 hir_type_size 结构化计算，不能用 LLVM 的 size_of()：
                // 后者对聚合类型返回符号常量表达式，代码生成阶段模块还没有
                // data layout（只在 linker.rs 里设置），取不到具体数值就会退化
                // 成保守的 8 字节，从而选错最大变体。
                let variant_payloads: Vec<HirType> = union_def
                    .variants
                    .iter()
                    .filter_map(|v| v.payload.clone())
                    .collect();

                let mut largest_ty: BasicTypeEnum<'ctx> = self.context.i8_type().into();
                let mut largest_size = 1u64; // i8 的大小

                for payload_ty in &variant_payloads {
                    let ty_size = self.hir_type_size(payload_ty)?;
                    if ty_size > largest_size {
                        largest_size = ty_size;
                        largest_ty = self.convert_type(payload_ty)?;
                    }
                }

                // 构建联合体类型：{ i32, payload }
                let discriminant_ty = self.context.i32_type();
                Ok(self.context.struct_type(&[discriminant_ty.into(), largest_ty], false).into())
            }

            HirType::FnPtr { params, ret } => {
                let param_types: Result<Vec<_>, _> = params
                    .iter()
                    .map(|p| self.convert_type(p).map(|t| t.into()))
                    .collect();
                let param_types: Vec<BasicMetadataTypeEnum> = param_types?;

                let _fn_type = if ret.is_void() {
                    self.context.void_type().fn_type(&param_types, false)
                } else {
                    let ret_ty = self.convert_type(ret)?;
                    ret_ty.fn_type(&param_types, false)
                };

                // 函数类型作为指针返回
                Ok(self.context.ptr_type(AddressSpace::default()).into())
            }

            HirType::Void => {
                Err(CodegenError::TypeConversion(
                    "void type cannot be used as BasicType".to_string()
                ))
            }

            HirType::Never => {
                // Never 类型在 LLVM 中不产生值，使用 i8 占位
                Ok(self.context.i8_type().into())
            }

            HirType::Slice { .. } => {
                // 切片表示为 {ptr, len}
                let ptr = self.context.ptr_type(AddressSpace::default());
                let len = self.context.i64_type();
                Ok(self.context.struct_type(&[ptr.into(), len.into()], false).into())
            }

            HirType::ErrUnion { ok, err } => {
                // 错误联合布局：{ tag: i64, payload: [i8 x N] }
                // 使用 i64 作为 tag 确保 payload 对齐到 8 字节
                // 计算 payload 大小（取两者最大值）。
                //
                // 必须结构化计算：LLVM 的 size_of() 对 str/slice 这类聚合返回
                // 符号常量表达式，代码生成阶段模块尚未设置 data layout，
                // get_zero_extended_constant() 会返回 None。原先的 unwrap_or(8)
                // 把 `i32 ! str` 的 16 字节胖指针算成 8 字节，导致构造 .Err 时
                // 越界写 8 字节并截断 len。
                let ok_size = self.hir_type_size(ok)?;
                let err_size = self.hir_type_size(err)?;

                let max_size = ok_size.max(err_size);

                // 使用字节数组作为 payload
                let payload_ty = self.context.i8_type().array_type(max_size as u32);

                // 构建错误联合类型：{ i64, [i8 x N] }
                let tag_ty = self.context.i64_type();
                Ok(self.context.struct_type(&[tag_ty.into(), payload_ty.into()], false).into())
            }
        }
    }
}
