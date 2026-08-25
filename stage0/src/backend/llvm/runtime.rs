//! 运行时外部函数声明。
//!
//! 为 Kore 语言提供 C 标准库函数的 LLVM 声明。

use inkwell::values::FunctionValue;
use inkwell::AddressSpace;

use super::{CodegenContext, CodegenError};

/// 声明 puts 函数：int puts(const char *s)
///
/// 标准 C 库函数，输出字符串并自动追加换行符。
pub fn declare_puts<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    if let Some(fn_val) = ctx.module.get_function("puts") {
        return Ok(fn_val);
    }

    let i32_ty = ctx.context.i32_type();
    let i8_ptr_ty = ctx.context.ptr_type(AddressSpace::default());
    let puts_ty = i32_ty.fn_type(&[i8_ptr_ty.into()], false);
    let puts_fn = ctx.module.add_function("puts", puts_ty, None);

    Ok(puts_fn)
}

/// 声明 printf 函数：int printf(const char *format, ...)
///
/// 标准 C 库函数，格式化输出。
pub fn declare_printf<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    if let Some(fn_val) = ctx.module.get_function("printf") {
        return Ok(fn_val);
    }

    let i32_ty = ctx.context.i32_type();
    let i8_ptr_ty = ctx.context.ptr_type(AddressSpace::default());
    let printf_ty = i32_ty.fn_type(&[i8_ptr_ty.into()], true); // 可变参数
    let printf_fn = ctx.module.add_function("printf", printf_ty, None);

    Ok(printf_fn)
}

/// 声明 free 函数：void free(void *ptr)
///
/// 标准 C 库函数，释放动态分配的内存。
pub fn declare_free<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    if let Some(fn_val) = ctx.module.get_function("free") {
        return Ok(fn_val);
    }

    let void_ty = ctx.context.void_type();
    let i8_ptr_ty = ctx.context.ptr_type(AddressSpace::default());
    let free_ty = void_ty.fn_type(&[i8_ptr_ty.into()], false);
    let free_fn = ctx.module.add_function("free", free_ty, None);

    Ok(free_fn)
}

/// 声明 read_file 函数：char* read_file(const char *path)
///
/// 自定义运行时函数，读取文件内容并返回字符串。
/// 返回 NULL 表示读取失败。
pub fn declare_read_file<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    if let Some(fn_val) = ctx.module.get_function("kore_read_file") {
        return Ok(fn_val);
    }

    let i8_ptr_ty = ctx.context.ptr_type(AddressSpace::default());
    let read_file_ty = i8_ptr_ty.fn_type(&[i8_ptr_ty.into()], false);
    let read_file_fn = ctx.module.add_function("kore_read_file", read_file_ty, None);

    Ok(read_file_fn)
}

/// 声明 write_file 函数：int write_file(const char *path, const char *content)
///
/// 自定义运行时函数，将字符串写入文件。
/// 返回 0 表示成功，-1 表示失败。
pub fn declare_write_file<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    if let Some(fn_val) = ctx.module.get_function("kore_write_file") {
        return Ok(fn_val);
    }

    let i32_ty = ctx.context.i32_type();
    let i8_ptr_ty = ctx.context.ptr_type(AddressSpace::default());
    let write_file_ty = i32_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into()], false);
    let write_file_fn = ctx.module.add_function("kore_write_file", write_file_ty, None);

    Ok(write_file_fn)
}

/// 声明 strlen 函数：size_t strlen(const char *s)
///
/// 标准 C 库函数，计算字符串长度。
pub fn declare_strlen<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    if let Some(fn_val) = ctx.module.get_function("strlen") {
        return Ok(fn_val);
    }

    let i64_ty = ctx.context.i64_type();
    let i8_ptr_ty = ctx.context.ptr_type(AddressSpace::default());
    let strlen_ty = i64_ty.fn_type(&[i8_ptr_ty.into()], false);
    let strlen_fn = ctx.module.add_function("strlen", strlen_ty, None);

    Ok(strlen_fn)
}

/// 声明 fprintf 函数：int fprintf(FILE *stream, const char *format, ...)
///
/// 标准 C 库函数，向指定流格式化输出。
pub fn declare_fprintf<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    if let Some(fn_val) = ctx.module.get_function("fprintf") {
        return Ok(fn_val);
    }

    let i32_ty = ctx.context.i32_type();
    let i8_ptr_ty = ctx.context.ptr_type(AddressSpace::default());
    let fprintf_ty = i32_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into()], true); // 可变参数
    let fprintf_fn = ctx.module.add_function("fprintf", fprintf_ty, None);

    Ok(fprintf_fn)
}

/// 获取 stderr 文件流的全局指针。
///
/// 通过声明外部全局变量 `stderr` 来访问标准错误流。
pub fn get_stderr<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
) -> Result<inkwell::values::PointerValue<'ctx>, CodegenError> {
    if let Some(global) = ctx.module.get_global("stderr") {
        return Ok(global.as_pointer_value());
    }

    let i8_ptr_ty = ctx.context.ptr_type(AddressSpace::default());
    let stderr_global = ctx.module.add_global(i8_ptr_ty, Some(AddressSpace::default()), "stderr");
    stderr_global.set_externally_initialized(true);

    Ok(stderr_global.as_pointer_value())
}

/// 声明 kore_get_argc 函数：i32 kore_get_argc()
///
/// 自定义运行时函数，返回命令行参数个数。
pub fn declare_get_argc<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    if let Some(fn_val) = ctx.module.get_function("kore_get_argc") {
        return Ok(fn_val);
    }

    let i32_ty = ctx.context.i32_type();
    let get_argc_ty = i32_ty.fn_type(&[], false);
    let get_argc_fn = ctx.module.add_function("kore_get_argc", get_argc_ty, None);

    Ok(get_argc_fn)
}

/// 声明 kore_get_argv 函数：char** kore_get_argv()
///
/// 自定义运行时函数，返回命令行参数数组指针。
pub fn declare_get_argv<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    if let Some(fn_val) = ctx.module.get_function("kore_get_argv") {
        return Ok(fn_val);
    }

    let ptr_ty = ctx.context.ptr_type(AddressSpace::default());
    let get_argv_ty = ptr_ty.fn_type(&[], false);
    let get_argv_fn = ctx.module.add_function("kore_get_argv", get_argv_ty, None);

    Ok(get_argv_fn)
}

/// 声明 kore_init_cmdline_args 函数：void kore_init_cmdline_args(int argc, char** argv)
///
/// 自定义运行时函数，初始化命令行参数存储。
pub fn declare_init_cmdline_args<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    if let Some(fn_val) = ctx.module.get_function("kore_init_cmdline_args") {
        return Ok(fn_val);
    }

    let void_ty = ctx.context.void_type();
    let i32_ty = ctx.context.i32_type();
    let ptr_ty = ctx.context.ptr_type(AddressSpace::default());
    let init_ty = void_ty.fn_type(&[i32_ty.into(), ptr_ty.into()], false);
    let init_fn = ctx.module.add_function("kore_init_cmdline_args", init_ty, None);

    Ok(init_fn)
}
