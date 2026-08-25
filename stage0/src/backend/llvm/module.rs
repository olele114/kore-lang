//! HIR 模块级代码生成。
//!
//! 实现两趟扫描：第一趟注册所有类型和函数声明，第二趟生成函数体。

use inkwell::types::BasicType;
use inkwell::values::AnyValue;

use super::{CodegenContext, CodegenError};
use crate::middleend::hir::{HirModule, HirFunction};

/// 生成整个 HIR 模块的 LLVM IR
pub fn codegen_module<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
    hir: &'ctx HirModule,
) -> Result<(), CodegenError> {
    // 设置 HIR 模块引用（用于类型转换时查询联合体/结构体定义）
    ctx.hir_module = Some(hir);

    // 第一趟：注册所有全局符号
    register_globals(ctx, hir)?;
    register_functions(ctx, hir)?;

    // 第二趟：生成函数体（跳过内置函数）
    for func in &hir.functions {
        if func.body.is_some() {
            codegen_function_body(ctx, func)?;
        }
    }

    // 如果有 main 函数，生成 C main 包装器
    if let Some(main_func) = hir.functions.iter().find(|f| f.name == "main") {
        generate_main_wrapper(ctx, main_func)?;
    }

    Ok(())
}

/// 注册全局变量声明
fn register_globals<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
    hir: &HirModule,
) -> Result<(), CodegenError> {
    for global in &hir.globals {
        let ty = ctx.convert_type(&global.ty)?;
        let global_val = ctx.module.add_global(ty, None, &global.name);

        // 设置初始化器
        if let Some(init) = &global.init {
            let init_val = super::const_::codegen_const_expr(ctx, init)?;
            global_val.set_initializer(&init_val);
        }

        // 默认链接性：internal
        global_val.set_linkage(inkwell::module::Linkage::Internal);
    }

    Ok(())
}

/// 注册所有函数声明
fn register_functions<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
    hir: &HirModule,
) -> Result<(), CodegenError> {
    for func in &hir.functions {
        let param_types: Result<Vec<_>, _> = func
            .params
            .iter()
            .map(|p| ctx.convert_type(&p.ty).map(|t| t.into()))
            .collect();
        let param_types = param_types?;

        let fn_type = if !func.ret_type.is_void() {
            let ret = ctx.convert_type(&func.ret_type)?;
            ret.fn_type(&param_types, false)
        } else {
            ctx.context.void_type().fn_type(&param_types, false)
        };

        // main 函数重命名为 kore_main
        let fn_name = if func.name == "main" {
            "kore_main"
        } else {
            &func.name
        };

        let fn_val = ctx.module.add_function(fn_name, fn_type, None);
        ctx.functions.insert(func.name.clone(), fn_val);
    }

    Ok(())
}

/// 生成单个函数的函数体
fn codegen_function_body<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
    func: &HirFunction,
) -> Result<(), CodegenError> {
    let fn_val = ctx
        .functions
        .get(&func.name)
        .ok_or_else(|| CodegenError::SymbolNotFound(func.name.clone()))?;

    // 内置函数没有函数体，跳过
    let body = func.body.as_ref().ok_or_else(|| {
        CodegenError::BuildError(format!("Function {} has no body", func.name))
    })?;

    // 委托给 function.rs 的 FunctionCodegen
    super::function::codegen_function(ctx, *fn_val, body, &func.params)
}

/// 生成 C main 包装器函数
///
/// 生成的 main 函数会：
/// 1. 调用 kore_init_cmdline_args(argc, argv)
/// 2. 调用 kore_main(...)
/// 3. 返回结果
fn generate_main_wrapper<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
    main_func: &HirFunction,
) -> Result<(), CodegenError> {
    // 声明 kore_init_cmdline_args 函数
    let init_fn = super::runtime::declare_init_cmdline_args(ctx)?;

    // 获取 kore_main 函数
    let kore_main = ctx
        .functions
        .get("main")
        .ok_or_else(|| CodegenError::SymbolNotFound("main".into()))?;

    // 创建 C main 函数：int main(int argc, char** argv)
    let i32_type = ctx.context.i32_type();
    let ptr_type = ctx.context.ptr_type(inkwell::AddressSpace::default());
    let main_type = i32_type.fn_type(&[i32_type.into(), ptr_type.into()], false);
    let main_fn = ctx.module.add_function("main", main_type, None);

    // 创建入口基本块
    let entry = ctx.context.append_basic_block(main_fn, "entry");
    ctx.builder.position_at_end(entry);

    // 获取 argc 和 argv 参数
    let argc = main_fn.get_nth_param(0).unwrap().into_int_value();
    let argv = main_fn.get_nth_param(1).unwrap().into_pointer_value();

    // 调用 kore_init_cmdline_args(argc, argv)
    ctx.builder
        .build_call(init_fn, &[argc.into(), argv.into()], "")
        .map_err(|e| CodegenError::BuildError(e.to_string()))?;

    // 调用 kore_main，传递原始参数
    let mut kore_main_args = Vec::new();
    for (i, param) in main_func.params.iter().enumerate() {
        if i == 0 {
            // 第一个参数：argc (i32)
            kore_main_args.push(argc.into());
        } else if i == 1 {
            // 第二个参数：argv，需要构造切片 { ptr, len }
            if matches!(param.ty, crate::middleend::hir::ty::HirType::Slice { .. }) {
                // 创建切片结构
                let slice_ty = ctx.context.struct_type(
                    &[
                        ctx.context.ptr_type(inkwell::AddressSpace::default()).into(),
                        ctx.context.i64_type().into(),
                    ],
                    false,
                );
                let slice_val = slice_ty.const_zero();

                // 设置 ptr 字段
                let slice_with_ptr = ctx.builder
                    .build_insert_value(slice_val, argv, 0, "slice_ptr")
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;

                // 设置 len 字段（argc 转为 i64）
                let len = ctx.builder
                    .build_int_cast(argc, ctx.context.i64_type(), "argc_as_i64")
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                let slice_complete = ctx.builder
                    .build_insert_value(slice_with_ptr.into_struct_value(), len, 1, "slice_complete")
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?
                    .into_struct_value();

                kore_main_args.push(slice_complete.into());
            } else {
                kore_main_args.push(argv.into());
            }
        }
    }

    let call_result = ctx.builder
        .build_call(*kore_main, &kore_main_args, "result")
        .map_err(|e| CodegenError::BuildError(e.to_string()))?;

    // 返回结果（如果 kore_main 返回整数）或返回 0
    if !main_func.ret_type.is_void() {
        // 检查返回值是否为整数类型
        match call_result.as_any_value_enum() {
            inkwell::values::AnyValueEnum::IntValue(int_val) => {
                ctx.builder.build_return(Some(&int_val))
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
            }
            _ => {
                // 非整数返回类型，返回 0（忽略返回值）
                let zero = i32_type.const_int(0, false);
                ctx.builder.build_return(Some(&zero))
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
            }
        }
    } else {
        let zero = i32_type.const_int(0, false);
        ctx.builder.build_return(Some(&zero))
            .map_err(|e| CodegenError::BuildError(e.to_string()))?;
    }

    Ok(())
}
