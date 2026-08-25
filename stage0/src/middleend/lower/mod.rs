//! AST → HIR 降级。
//!
//! ADR 011：降级过程将 frontend AST 转换为显式 CFG 的 HIR。
//! - 表达式变成 Place/Operand/Rvalue
//! - 控制流变成 BasicBlock + Terminator
//! - Defer 在降级时展开

mod ty;
mod expr;
mod control;
mod func;
mod pattern;

#[cfg(test)]
mod tests;

use crate::diag::{DiagSink, Span};
use crate::frontend::ast::{Module as AstModule, Item, Expr, Stmt};
use crate::frontend::resolve::SymbolTable;
use crate::frontend::typecheck::TypeContext;
use crate::middleend::hir::{HirFunction, HirParam};
use crate::middleend::hir::ty::HirType;
use crate::middleend::hir::{
    HirModule, HirStruct, HirUnion,
    HirLocal, HirBlock, HirTerminator,
    BlockId, LocalId, StructId, UnionId, FuncId,
};
use std::collections::HashMap;

pub use ty::TypeConverter;
pub use expr::ExprResult;

/// 降级上下文，维护降级过程中的所有状态。
pub struct LoweringContext<'a> {
    /// 诊断接收器。
    pub diag: &'a mut DiagSink,

    /// 符号表（来自 resolve pass）。
    pub symbols: &'a SymbolTable,

    /// 类型上下文（来自 typecheck pass）。
    pub type_ctx: &'a TypeContext,

    /// 结构体名 → ID 映射。
    pub struct_map: HashMap<String, StructId>,

    /// 联合体名 → ID 映射。
    pub union_map: HashMap<String, UnionId>,

    /// 联合体定义引用（用于查询变体索引）。
    pub union_defs: HashMap<String, &'a crate::frontend::ast::UnionDef>,

    /// 函数名 → ID 映射。
    pub func_map: HashMap<String, FuncId>,

    /// 当前正在降级的函数的局部变量。
    pub locals: Vec<HirLocal>,

    /// 当前函数的基本块。
    pub blocks: Vec<HirBlock>,

    /// 当前正在构建的基本块 ID。
    pub current_block: Option<BlockId>,

    /// 局部变量名称 → LocalId 映射（用于名称解析）。
    local_map: HashMap<String, LocalId>,

    /// 下一个 LocalId。
    next_local: usize,

    /// 下一个 BlockId。
    next_block: usize,

    /// 循环上下文栈：每个元素为 (header_block, exit_block, scope_depth)。
    /// scope_depth 记录进入循环时的作用域深度，用于在 break/continue 时正确展开 defer 和 drop。
    loop_stack: Vec<(BlockId, BlockId, usize)>,

    /// 循环标签映射：label → (header_block, exit_block, scope_depth)。
    label_map: HashMap<String, (BlockId, BlockId, usize)>,

    /// defer 作用域栈：每个作用域有自己的 defer 表达式列表，在作用域退出时逆序执行。
    /// 块级 defer 语义：defer 在声明它的作用域退出时执行，而非函数返回时。
    defer_scopes: Vec<Vec<(crate::frontend::ast::Expr, Span)>>,

    /// 作用域栈：跟踪每个作用域内创建的 owned 指针，用于自动插入 drop。
    /// 每个元素为 (scope_locals)，其中 scope_locals 是该作用域内的 owned 指针 LocalId。
    scope_stack: Vec<Vec<LocalId>>,

    /// 已移动的局部变量集合（跳过 drop 以避免 double drop）。
    moved_locals: std::collections::HashSet<LocalId>,

    /// 当前函数的返回类型（用于推断错误联合类型）。
    current_function_return_type: Option<HirType>,
}

impl<'a> LoweringContext<'a> {
    pub fn new(
        diag: &'a mut DiagSink,
        symbols: &'a SymbolTable,
        type_ctx: &'a TypeContext,
    ) -> Self {
        Self {
            diag,
            symbols,
            type_ctx,
            struct_map: HashMap::new(),
            union_map: HashMap::new(),
            union_defs: HashMap::new(),
            func_map: HashMap::new(),
            locals: Vec::new(),
            blocks: Vec::new(),
            current_block: None,
            local_map: HashMap::new(),
            next_local: 0,
            next_block: 0,
            loop_stack: Vec::new(),
            label_map: HashMap::new(),
            defer_scopes: Vec::new(),
            scope_stack: Vec::new(),
            moved_locals: std::collections::HashSet::new(),
            current_function_return_type: None,
        }
    }

    /// 分配新的 LocalId。
    pub fn fresh_local(&mut self) -> LocalId {
        let id = LocalId(self.next_local);
        self.next_local += 1;
        id
    }

    /// 分配新的 BlockId。
    pub fn fresh_block(&mut self) -> BlockId {
        let id = BlockId(self.next_block);
        self.next_block += 1;
        id
    }

    /// 开始一个新的基本块。
    pub fn start_block(&mut self, span: Span) -> BlockId {
        let id = self.fresh_block();
        self.blocks.push(HirBlock {
            id,
            stmts: Vec::new(),
            terminator: HirTerminator::Unreachable,
            span,
        });
        self.current_block = Some(id);
        id
    }

    /// 获取当前基本块的可变引用。
    pub fn current_block_mut(&mut self) -> Option<&mut HirBlock> {
        let id = self.current_block?;
        // 通过 block.id 查找，而不是用 BlockId.0 作为索引
        self.blocks.iter_mut().find(|b| b.id == id)
    }

    /// 重置函数级状态（在开始降级新函数时调用）。
    pub fn reset_function_state(&mut self) {
        self.locals.clear();
        self.blocks.clear();
        self.current_block = None;
        self.local_map.clear();
        self.next_local = 0;
        self.next_block = 0;
        self.defer_scopes.clear();
        self.scope_stack.clear();
        self.moved_locals.clear();
    }

    /// 注册新的局部变量（绑定名称到 LocalId）。
    pub fn register_local(&mut self, name: String, id: LocalId) {
        self.local_map.insert(name, id);
    }

    /// 查找局部变量（名称 → LocalId）。
    pub fn lookup_local(&self, name: &str) -> Option<LocalId> {
        self.local_map.get(name).copied()
    }

    /// 查找函数（名称 → FuncId）。
    pub fn lookup_func(&self, name: &str) -> Option<FuncId> {
        self.func_map.get(name).copied()
    }

    /// 进入新作用域（开始跟踪该作用域内的 owned 指针和 defer 语句）。
    pub fn enter_scope(&mut self) {
        self.scope_stack.push(Vec::new());
        self.defer_scopes.push(Vec::new());
    }

    /// 退出作用域，返回该作用域内创建的所有 owned 指针和 defer 表达式。
    /// 返回值：(owned_locals, defers)
    pub fn exit_scope(&mut self) -> (Vec<LocalId>, Vec<(crate::frontend::ast::Expr, Span)>) {
        let owned_locals = self.scope_stack.pop().unwrap_or_default();
        let defers = self.defer_scopes.pop().unwrap_or_default();
        (owned_locals, defers)
    }

    /// 记录一个 owned 指针到当前作用域。
    pub fn track_owned_local(&mut self, local_id: LocalId) {
        if let Some(scope) = self.scope_stack.last_mut() {
            scope.push(local_id);
        }
    }

    /// 检查局部变量是否为 owned 指针。
    pub fn is_owned_local(&self, local_id: LocalId) -> bool {
        self.locals.get(local_id.0)
            .map(|local| matches!(local.ty, HirType::Ptr { owned: true, .. }))
            .unwrap_or(false)
    }

    /// 标记局部变量为已移动（避免 double drop）。
    pub fn mark_moved(&mut self, local_id: LocalId) {
        self.moved_locals.insert(local_id);
    }

    /// 检查局部变量是否已被移动。
    pub fn is_moved(&self, local_id: LocalId) -> bool {
        self.moved_locals.contains(&local_id)
    }

    /// 检测表达式是否总是终止（返回或发散）。
    ///
    /// 用于判断 match arm 是否会继续执行到 after_block。
    pub fn expr_always_terminates(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Ret(..) | Expr::Stop { .. } | Expr::Skip { .. } | Expr::Jmp { .. } => true,
            Expr::Block { stmts, .. } => {
                // 块终止条件：最后一个语句终止
                stmts.last()
                    .map(|last| self.stmt_always_terminates(last))
                    .unwrap_or(false)
            }
            Expr::Branch { arms, .. } => {
                // Branch (if/match) 终止条件：所有 arm 都终止
                !arms.is_empty() && arms.iter().all(|arm| self.expr_always_terminates(&arm.body))
            }
            _ => false,
        }
    }

    /// 检测语句是否总是终止。
    fn stmt_always_terminates(&self, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Expr(expr) => self.expr_always_terminates(expr),
            _ => false,
        }
    }

    /// 根据联合体名称和变体名称查询变体索引。
    ///
    /// # 参数
    /// - `union_name`: 联合体名称
    /// - `variant_name`: 变体名称
    ///
    /// # 返回
    /// - `Some(index)`: 变体索引（从 0 开始）
    /// - `None`: 未找到联合体或变体
    pub fn find_variant_index_by_union(&self, union_name: &str, variant_name: &str) -> Option<usize> {
        let union_def = self.union_defs.get(union_name)?;
        union_def.variants.iter()
            .position(|v| v.name == variant_name)
    }

    /// 根据 UnionId 查询联合体名称。
    pub fn get_union_name(&self, union_id: UnionId) -> Option<&str> {
        for (name, id) in &self.union_map {
            if *id == union_id {
                return Some(name.as_str());
            }
        }
        None
    }

    /// 查询变体的 payload 类型。
    ///
    /// # 参数
    /// - `union_name`: 联合体名称
    /// - `variant_index`: 变体索引
    ///
    /// # 返回
    /// - `Some(HirType)`: 变体的 payload 类型
    /// - `None`: 未找到或无 payload
    pub fn get_variant_payload_type(&self, union_name: &str, variant_index: usize) -> Option<HirType> {
        let union_def = self.union_defs.get(union_name)?;
        let variant = union_def.variants.get(variant_index)?;

        if variant.payload.is_empty() {
            return None;
        }

        // 转换前端类型表达式为 HIR 类型
        let ty_expr = &variant.payload[0];
        let mut diag_sink = crate::diag::DiagSink::new();
        let frontend_ty = self.type_ctx.resolve_type_expr(ty_expr, &mut diag_sink);
        let mut type_conv = TypeConverter::new(&self.struct_map, &self.union_map, &mut diag_sink);
        Some(type_conv.convert(&frontend_ty, variant.span))
    }
}

/// 降级整个模块。
pub fn lower_module(
    ast: &AstModule,
    symbols: &SymbolTable,
    type_ctx: &TypeContext,
    diag: &mut DiagSink,
) -> HirModule {
    let mut ctx = LoweringContext::new(diag, symbols, type_ctx);

    // 第一遍：收集所有函数、结构体和联合体名称，建立 ID 映射
    let mut func_id_counter = 0usize;
    let mut struct_id_counter = 0usize;
    let mut union_id_counter = 0usize;

    // 预注册内置函数
    let print_id = FuncId(func_id_counter);
    func_id_counter += 1;
    ctx.func_map.insert("print".to_string(), print_id);

    let println_id = FuncId(func_id_counter);
    func_id_counter += 1;
    ctx.func_map.insert("println".to_string(), println_id);

    let read_file_id = FuncId(func_id_counter);
    func_id_counter += 1;
    ctx.func_map.insert("read_file".to_string(), read_file_id);

    let write_file_id = FuncId(func_id_counter);
    func_id_counter += 1;
    ctx.func_map.insert("write_file".to_string(), write_file_id);

    let eprint_id = FuncId(func_id_counter);
    func_id_counter += 1;
    ctx.func_map.insert("eprint".to_string(), eprint_id);

    let eprintln_id = FuncId(func_id_counter);
    func_id_counter += 1;
    ctx.func_map.insert("eprintln".to_string(), eprintln_id);

    for item in &ast.items {
        match item {
            Item::Func(f) => {
                let id = FuncId(func_id_counter);
                func_id_counter += 1;
                ctx.func_map.insert(f.name.clone(), id);
            }
            Item::Struct(s) => {
                let id = StructId(struct_id_counter);
                struct_id_counter += 1;
                ctx.struct_map.insert(s.name.clone(), id);
            }
            Item::Union(u) => {
                let id = UnionId(union_id_counter);
                union_id_counter += 1;
                ctx.union_map.insert(u.name.clone(), id);
                ctx.union_defs.insert(u.name.clone(), u);
            }
            _ => {}
        }
    }

    // 第二遍：降级所有项
    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut unions = Vec::new();
    let _globals: Vec<crate::middleend::hir::HirGlobal> = Vec::new();  // Kore0 暂不支持全局变量

    // 先添加内置函数声明（占位，无函数体）
    use crate::diag::Span;
    let dummy_span = Span::new(crate::diag::FileId(0), 0, 0);

    functions.push(HirFunction {
        name: "print".to_string(),
        params: vec![HirParam { name: "s".to_string(), ty: HirType::Str, span: dummy_span }],
        ret_type: HirType::Void,
        body: None,  // 内置函数无函数体
        span: dummy_span,
    });

    functions.push(HirFunction {
        name: "println".to_string(),
        params: vec![HirParam { name: "s".to_string(), ty: HirType::Str, span: dummy_span }],
        ret_type: HirType::Void,
        body: None,
        span: dummy_span,
    });

    functions.push(HirFunction {
        name: "read_file".to_string(),
        params: vec![HirParam { name: "path".to_string(), ty: HirType::Str, span: dummy_span }],
        ret_type: HirType::Str,
        body: None,
        span: dummy_span,
    });

    functions.push(HirFunction {
        name: "write_file".to_string(),
        params: vec![
            HirParam { name: "path".to_string(), ty: HirType::Str, span: dummy_span },
            HirParam { name: "content".to_string(), ty: HirType::Str, span: dummy_span },
        ],
        ret_type: HirType::i32(),
        body: None,
        span: dummy_span,
    });

    functions.push(HirFunction {
        name: "eprint".to_string(),
        params: vec![HirParam { name: "s".to_string(), ty: HirType::Str, span: dummy_span }],
        ret_type: HirType::Void,
        body: None,
        span: dummy_span,
    });

    functions.push(HirFunction {
        name: "eprintln".to_string(),
        params: vec![HirParam { name: "s".to_string(), ty: HirType::Str, span: dummy_span }],
        ret_type: HirType::Void,
        body: None,
        span: dummy_span,
    });

    for item in &ast.items {
        match item {
            Item::Func(f) => {
                let hir_func = ctx.lower_func(f);
                functions.push(hir_func);
            }
            Item::Struct(s) => {
                let id = *ctx.struct_map.get(&s.name).unwrap();
                let hir_struct = lower_struct(s, id, &mut ctx);
                structs.push(hir_struct);
            }
            Item::Union(u) => {
                let id = *ctx.union_map.get(&u.name).unwrap();
                let hir_union = lower_union(u, id, &mut ctx);
                unions.push(hir_union);
            }
            Item::Use(_) => {
                // Use 声明在 stage0 中不生成 HIR（模块系统是 stage1 特性）
            }
        }
    }

    HirModule {
        functions,
        structs,
        unions,
        globals: vec![],
    }
}

/// 降级结构体定义。
fn lower_struct(
    ast: &crate::frontend::ast::StructDef,
    _id: StructId,
    ctx: &mut LoweringContext,
) -> HirStruct {
    let mut type_conv = TypeConverter::new(&ctx.struct_map, &ctx.union_map, ctx.diag);

    let fields = ast.fields.iter().map(|f| {
        // 从类型上下文获取字段类型
        let frontend_ty = ctx.type_ctx.get_struct_field(&ast.name, &f.name);
        let hir_ty = match frontend_ty {
            Some(ty) => type_conv.convert(&ty, f.span),
            None => HirType::Void,
        };

        crate::middleend::hir::HirField {
            name: f.name.clone(),
            ty: hir_ty,
            span: f.span,
        }
    }).collect();

    HirStruct {
        name: ast.name.clone(),
        fields,
        span: ast.span,
    }
}

/// 降级联合体定义。
fn lower_union(
    ast: &crate::frontend::ast::UnionDef,
    _id: UnionId,
    ctx: &mut LoweringContext,
) -> HirUnion {
    let mut variants = Vec::new();

    for v in &ast.variants {
        let payload = if v.payload.is_empty() {
            None
        } else {
            // 对于多个类型，创建匿名结构体来包装
            // 简化处理：只取第一个类型
            let ty_expr = &v.payload[0];
            let frontend_ty = ctx.type_ctx.resolve_type_expr(ty_expr, ctx.diag);
            let mut type_conv = TypeConverter::new(&ctx.struct_map, &ctx.union_map, ctx.diag);
            Some(type_conv.convert(&frontend_ty, v.span))
        };

        variants.push(crate::middleend::hir::HirVariant {
            name: v.name.clone(),
            payload,
            span: v.span,
        });
    }

    HirUnion {
        name: ast.name.clone(),
        variants,
        span: ast.span,
    }
}
