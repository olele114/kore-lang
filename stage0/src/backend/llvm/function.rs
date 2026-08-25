//! 函数级代码生成。
//!
//! 负责将 HIR 函数体（CFG 基本块）转换为 LLVM IR。

use inkwell::values::{FunctionValue, PointerValue, AnyValue};
use inkwell::basic_block::BasicBlock;
use std::collections::HashMap;

use crate::middleend::hir::{HirBody, HirLocal, HirParam, BlockId, LocalId};
use super::{CodegenContext, CodegenError};

/// 函数代码生成器
pub struct FunctionCodegen<'a, 'ctx> {
    /// 全局代码生成上下文
    ctx: &'a mut CodegenContext<'ctx>,
    /// 当前函数
    function: FunctionValue<'ctx>,
    /// 局部变量 ID → alloca 指针
    locals: HashMap<LocalId, PointerValue<'ctx>>,
    /// HIR 基本块 ID → LLVM 基本块
    blocks: HashMap<BlockId, BasicBlock<'ctx>>,
    /// 局部变量类型信息（用于 load 时查询）
    local_types: Vec<HirLocal>,
}

impl<'a, 'ctx> FunctionCodegen<'a, 'ctx> {
    /// 创建新的函数代码生成器
    fn new(ctx: &'a mut CodegenContext<'ctx>, function: FunctionValue<'ctx>, local_types: Vec<HirLocal>) -> Self {
        Self {
            ctx,
            function,
            locals: HashMap::new(),
            blocks: HashMap::new(),
            local_types,
        }
    }

    /// 生成函数体
    fn codegen_body(mut self, body: &HirBody, params: &[HirParam]) -> Result<(), CodegenError> {
        // 1. 创建入口块并分配所有局部变量
        let entry_block = self.ctx.context.append_basic_block(self.function, "entry");
        self.ctx.builder.position_at_end(entry_block);

        // body.locals 的长度就是 LocalId 的数量，我们需要为每个 LocalId 分配栈空间
        for local_idx in 0..body.locals.len() {
            let local_id = LocalId(local_idx);
            let local = &body.locals[local_idx];

            // Skip void-typed locals - they don't need stack allocation
            if local.ty.is_void() {
                continue;
            }
            let ty = self.ctx.convert_type(&local.ty)?;
            let name = local.name.as_deref().unwrap_or("tmp");
            let alloca = self.ctx.builder.build_alloca(ty, &format!("_{}_{}", name, local_idx))
                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
            self.locals.insert(local_id, alloca);
        }

        // 1.5. 将函数参数存储到对应的局部变量中
        // HIR 降级时已经为每个参数创建了对应的局部变量（LocalId 0, 1, 2...）
        for (param_idx, _param) in params.iter().enumerate() {
            let param_value = self.function.get_nth_param(param_idx as u32)
                .ok_or_else(|| CodegenError::BuildError(format!("param {} not found", param_idx)))?;
            let local_id = LocalId(param_idx);
            if let Some(alloca) = self.locals.get(&local_id) {
                self.ctx.builder.build_store(*alloca, param_value)
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
            }
        }

        // 1.6. 如果是 main 函数，注入命令行参数初始化
        if self.function.get_name().to_str() == Ok("main") && params.len() == 2 {
            self.inject_cmdline_args_init()?;
        }

        // 2. 预先创建所有 LLVM 基本块
        for block in &body.blocks {
            let bb_name = format!("bb{}", block.id.0);
            let llvm_block = self.ctx.context.append_basic_block(self.function, &bb_name);
            self.blocks.insert(block.id, llvm_block);
        }

        // 3. 从 entry 跳转到 HIR 入口块
        let hir_entry = self.blocks
            .get(&body.entry_block)
            .ok_or_else(|| CodegenError::BuildError("entry block not found".to_string()))?;
        self.ctx.builder.build_unconditional_branch(*hir_entry)
            .map_err(|e| CodegenError::BuildError(e.to_string()))?;

        // 4. 生成每个基本块的内容
        for block in &body.blocks {
            let llvm_block = self.blocks[&block.id];
            self.ctx.builder.position_at_end(llvm_block);

            // 生成语句
            for stmt in &block.stmts {
                self.codegen_stmt(stmt)?;
            }

            // 生成终结符
            self.codegen_terminator(&block.terminator)?;
        }

        Ok(())
    }

    /// 生成语句
    fn codegen_stmt(&mut self, stmt: &crate::middleend::hir::HirStmt) -> Result<(), CodegenError> {
        super::stmt::codegen_stmt(self.ctx, &self.locals, &self.local_types, stmt, self.function)
    }

    /// 生成终结符
    fn codegen_terminator(
        &mut self,
        term: &crate::middleend::hir::HirTerminator,
    ) -> Result<(), CodegenError> {
        use crate::middleend::hir::HirTerminator;

        match term {
            HirTerminator::Goto(target) => {
                let target_block = self.blocks
                    .get(target)
                    .ok_or_else(|| CodegenError::BuildError(format!("block {:?} not found", target)))?;
                self.ctx.builder.build_unconditional_branch(*target_block)
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
            }

            HirTerminator::Return(val) => {
                if let Some(operand) = val {
                    let ret_val = super::rvalue::codegen_operand(self.ctx, &self.locals, &self.local_types, operand)?;
                    self.ctx.builder.build_return(Some(&ret_val))
                        .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                } else {
                    self.ctx.builder.build_return(None)
                        .map_err(|e| CodegenError::BuildError(e.to_string()))?;
                }
            }

            HirTerminator::Switch { discr, targets, otherwise } => {
                let discr_val = super::rvalue::codegen_operand(self.ctx, &self.locals, &self.local_types, discr)?;
                let otherwise_block = self.blocks
                    .get(otherwise)
                    .ok_or_else(|| CodegenError::BuildError(format!("block {:?} not found", otherwise)))?;

                // case 常量类型必须与 discriminant 一致（联合判别值为 i32，
                // 循环/条件分支的布尔判别值为 i1）
                let discr_int_ty = discr_val.into_int_value().get_type();
                let cases: Result<Vec<_>, _> = targets
                    .iter()
                    .map(|(val, target)| {
                        let target_block = self.blocks
                            .get(target)
                            .ok_or_else(|| CodegenError::BuildError(format!("block {:?} not found", target)))?;
                        let const_val = discr_int_ty.const_int(*val, false);
                        Ok((const_val, *target_block))
                    })
                    .collect();
                let cases = cases?;

                self.ctx.builder.build_switch(discr_val.into_int_value(), *otherwise_block, &cases)
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
            }

            HirTerminator::Unreachable => {
                self.ctx.builder.build_unreachable()
                    .map_err(|e| CodegenError::BuildError(e.to_string()))?;
            }
        }

        Ok(())
    }

    /// 为 main 函数注入命令行参数初始化代码
    ///
    /// 调用 kore_get_argc() 和 kore_get_argv() 并存储到参数对应的局部变量
    fn inject_cmdline_args_init(&mut self) -> Result<(), CodegenError> {
        // 声明运行时函数
        let get_argc_fn = super::runtime::declare_get_argc(self.ctx)?;
        let get_argv_fn = super::runtime::declare_get_argv(self.ctx)?;

        // 调用 kore_get_argc() 并存储到 LocalId(0)
        let call_result = self.ctx.builder.build_call(get_argc_fn, &[], "argc")
            .map_err(|e| CodegenError::BuildError(e.to_string()))?;
        let argc_val = call_result.as_any_value_enum().into_int_value();

        if let Some(argc_alloca) = self.locals.get(&LocalId(0)) {
            self.ctx.builder.build_store(*argc_alloca, argc_val)
                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
        }

        // 调用 kore_get_argv() 获取 char** 指针
        let call_result = self.ctx.builder.build_call(get_argv_fn, &[], "argv_ptr")
            .map_err(|e| CodegenError::BuildError(e.to_string()))?;
        let argv_ptr = call_result.as_any_value_enum().into_pointer_value();

        // 构造切片 {ptr: char**, len: argc}
        // 切片类型是 {ptr, len}
        let slice_ty = self.ctx.context.struct_type(
            &[
                self.ctx.context.ptr_type(inkwell::AddressSpace::default()).into(),
                self.ctx.context.i64_type().into(),
            ],
            false,
        );

        // 将 i32 argc 扩展为 i64 len
        let argc_i64 = self.ctx.builder.build_int_z_extend(
            argc_val,
            self.ctx.context.i64_type(),
            "argc_i64",
        ).map_err(|e| CodegenError::BuildError(e.to_string()))?;

        // 构造切片结构
        let mut slice_val = slice_ty.get_undef();
        slice_val = self.ctx.builder.build_insert_value(slice_val, argv_ptr, 0, "slice_ptr")
            .map_err(|e| CodegenError::BuildError(e.to_string()))?
            .into_struct_value();
        slice_val = self.ctx.builder.build_insert_value(slice_val, argc_i64, 1, "slice_len")
            .map_err(|e| CodegenError::BuildError(e.to_string()))?
            .into_struct_value();

        // 存储切片到 LocalId(1)
        if let Some(argv_alloca) = self.locals.get(&LocalId(1)) {
            self.ctx.builder.build_store(*argv_alloca, slice_val)
                .map_err(|e| CodegenError::BuildError(e.to_string()))?;
        }

        Ok(())
    }
}

/// 生成函数（入口点）
pub fn codegen_function<'ctx>(
    ctx: &mut CodegenContext<'ctx>,
    function: FunctionValue<'ctx>,
    body: &HirBody,
    params: &[HirParam],
) -> Result<(), CodegenError> {
    let codegen = FunctionCodegen::new(ctx, function, body.locals.clone());
    codegen.codegen_body(body, params)
}
