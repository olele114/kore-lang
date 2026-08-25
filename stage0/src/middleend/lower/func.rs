//! 函数降级：参数、局部变量、函数体。
//!
//! 核心流程：
//! 1. 转换函数签名（参数类型、返回类型）
//! 2. 为每个参数创建 HirLocal
//! 3. 创建入口基本块
//! 4. 降级函数体（语句序列）
//! 5. 确保所有控制流路径终止（Return/Unreachable）

use crate::frontend::ast::{Func, Expr, Stmt};
use crate::frontend::ast::node::TypeExpr;
use crate::middleend::hir::*;
use crate::middleend::hir::ty::HirType;
use crate::diag::{Span, Diagnostic, DiagLoc, ErrorCode};
use super::{LoweringContext, TypeConverter};

impl<'a> LoweringContext<'a> {
    /// 降级函数定义
    pub fn lower_func(&mut self, ast: &Func) -> HirFunction {
        // 重置函数级状态
        self.reset_function_state();

        // 1. 先解析所有语义类型（统一借用 diag）
        let param_semantic_types: Vec<_> = ast.params.iter()
            .map(|param| self.type_ctx.resolve_type_expr(&param.ty, self.diag))
            .collect();

        let ret_semantic_ty = ast.ret.as_ref()
            .map(|ret| self.type_ctx.resolve_type_expr(ret, self.diag));

        // 2. 转换参数和返回类型（在一个作用域内完成，之后释放 type_converter）
        let (hir_param_types, ret_ty) = {
            let mut type_converter = TypeConverter::new(
                &self.struct_map,
                &self.union_map,
                self.diag,
            );

            let hir_param_types: Vec<_> = param_semantic_types.iter()
                .zip(ast.params.iter())
                .map(|(semantic_ty, param)| {
                    type_converter.convert(semantic_ty, param.span)
                })
                .collect();

            let ret_ty = if let Some(semantic_ret) = ret_semantic_ty {
                type_converter.convert(&semantic_ret, ast.span)
            } else {
                HirType::Void
            };

            (hir_param_types, ret_ty)
        };

        // 保存当前函数的返回类型
        self.current_function_return_type = Some(ret_ty.clone());

        // 3. 创建 HirParam 和局部变量（type_converter 已释放）
        let mut hir_params = Vec::new();
        let mut owned_params = Vec::new();  // 收集 owned 参数的 LocalId

        for ((param, hir_ty), _) in ast.params.iter().zip(hir_param_types.iter()).zip(param_semantic_types.iter()) {
            let local_id = self.fresh_local();

            // 创建局部变量
            self.locals.push(HirLocal {
                name: Some(param.name.clone()),
                ty: hir_ty.clone(),
                span: param.span,
            });

            // 创建参数
            hir_params.push(HirParam {
                name: param.name.clone(),
                ty: hir_ty.clone(),
                span: param.span,
            });

            // 注册参数名到 LocalId 映射
            self.register_local(param.name.clone(), local_id);

            // 如果是 owned 指针参数，记录下来
            if matches!(hir_ty, HirType::Ptr { owned: true, .. }) {
                owned_params.push(local_id);
            }
        }

        // 4. 创建入口基本块
        let entry = self.start_block(ast.span);

        // 5. 进入函数体作用域（为函数体建立作用域）
        self.enter_scope();

        // 6. 将 owned 参数注册到当前作用域（函数体作用域）
        for local_id in owned_params {
            self.track_owned_local(local_id);
        }

        // 7. 降级函数体
        self.lower_block_body(&ast.body, ast.span);

        // 8. 退出函数体作用域，插入 defer 和 Drop
        let (owned_locals, defers) = self.exit_scope();

        // 先插入 Drop（先析构）
        for local_id in owned_locals.iter().rev() {
            // 跳过已移动的变量（避免 double drop）
            if !self.is_moved(*local_id) {
                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Drop {
                        place: HirPlace::Local(*local_id),
                        span: ast.span,
                    });
                }
            }
        }

        // 后插入 defer（后清理，此时对象已析构）
        for (defer_expr, _span) in defers.iter().rev() {
            let _ = self.lower_expr(defer_expr);
        }

        // 9. 确保函数终止
        self.ensure_termination(matches!(ret_ty, HirType::Void), ast.span);

        // DEBUG: 打印 HIR 结构
        crate::trace!("\n=== HIR for function {} ===", ast.name);
        for block in &self.blocks {
            crate::trace!("Block {}:", block.id.0);
            for stmt in &block.stmts {
                crate::trace!("  {:?}", stmt);
            }
            crate::trace!("  terminator: {:?}", block.terminator);
        }
        crate::trace!("=== END HIR ===\n");

        HirFunction {
            name: ast.name.clone(),
            params: hir_params,
            ret_type: ret_ty,
            body: Some(HirBody {
                locals: std::mem::take(&mut self.locals),
                blocks: std::mem::take(&mut self.blocks),
                entry_block: entry,
            }),
            span: ast.span,
        }
    }

    /// 降级块表达式的主体（语句序列）
    fn lower_block_body(&mut self, body: &Expr, _span: Span) {
        match body {
            Expr::Block { stmts, .. } => {
                for stmt in stmts {
                    self.lower_stmt(stmt);
                }
            }
            // 如果不是块，当作单个表达式，并将其作为返回值
            _ => {
                if let Some(operand) = self.lower_expr_to_operand(body) {
                    // 将表达式的值作为函数返回值
                    if let Some(block) = self.current_block_mut() {
                        block.terminator = HirTerminator::Return(Some(operand));
                    }
                }
            }
        }
    }

    /// 降级语句
    pub(super) fn lower_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                ty,
                init,
                is_mut,
                span,
            } => {
                self.lower_let(name, ty.as_ref(), Some(init), *is_mut, *span);
            }

            Stmt::Assign { target, value, span } => {
                self.lower_assign(target, value, *span);
            }

            Stmt::Defer(expr, span) => {
                // 收集 defer 语句到当前作用域，在作用域退出时逆序执行
                if let Some(defer_scope) = self.defer_scopes.last_mut() {
                    defer_scope.push((expr.clone(), *span));
                }
            }

            Stmt::Expr(expr) => {
                self.lower_expr(expr);
            }
        }
    }

    /// 降级 let 绑定
    fn lower_let(
        &mut self,
        name: &str,
        ty: Option<&TypeExpr>,
        init: Option<&Expr>,
        _is_mut: bool,
        span: Span,
    ) {
        // 1. 推断或转换类型
        let hir_ty = if let Some(ty) = ty {
            let mut converter = TypeConverter::new(
                &self.struct_map,
                &self.union_map,
                self.diag,
            );
            // TypeExpr 的各变体都在最后一个字段携带 Span
            let ty_span = match ty {
                TypeExpr::Named(_, s) => *s,
                TypeExpr::Borrow(_, s) => *s,
                TypeExpr::Own(_, s) => *s,
                TypeExpr::Array(_, _, s) => *s,
                TypeExpr::Slice(_, s) => *s,
                TypeExpr::ErrUnion(_, _, s) => *s,
            };
            converter.convert_type_expr(ty, ty_span)
        } else if let Some(init_expr) = init {
            self.infer_expr_type(init_expr)
        } else {
            // 无类型无初始化，报错
            self.diag.emit(Diagnostic::error(
                ErrorCode::InternalCompilerError.as_u16(),
                format!("variable `{}` needs type annotation", name),
                DiagLoc::At(span),
            ));
            HirType::Void
        };

        // 2. 创建 local
        let local_id = self.fresh_local();
        self.locals.push(HirLocal {
            name: Some(name.to_string()),
            ty: hir_ty.clone(),
            span,
        });

        // 注册变量名到 LocalId 映射
        self.register_local(name.to_string(), local_id);

        // 2.5. 如果是 owned 指针，记录到当前作用域
        if matches!(hir_ty, HirType::Ptr { owned: true, .. }) {
            self.track_owned_local(local_id);
        }

        // 3. 降级初始化表达式
        if let Some(init_expr) = init {
            if let Some(init_op) = self.lower_expr_to_operand(init_expr) {
                // 如果 init_op 是 owned 指针的局部变量，标记为已移动
                if let HirOperand::Place(place_box) = &init_op {
                    if let HirPlace::Local(src_local) = **place_box {
                        if self.is_owned_local(src_local) {
                            self.mark_moved(src_local);
                        }
                    }
                }

                // 检查是否需要数组到切片的隐式转换
                let init_ty = init_op.ty(self);

                // 无显式类型标注时，以实际降级结果的类型为准。分支/块表达式的
                // 类型依赖模式绑定，无法在降级前从语法推断，只能事后回填。
                if ty.is_none() && hir_ty == HirType::Void && init_ty != HirType::Void {
                    self.locals[local_id.0].ty = init_ty.clone();
                }

                let rvalue = if let (HirType::Slice { elem: slice_elem }, HirType::Array { elem: arr_elem, len }) = (&hir_ty, &init_ty) {
                    // 数组到切片的隐式转换
                    if slice_elem == arr_elem {
                        HirRvalue::ArrayToSlice {
                            array: init_op,
                            elem_ty: (**slice_elem).clone(),
                            len: *len,
                        }
                    } else {
                        HirRvalue::Use(init_op)
                    }
                } else {
                    HirRvalue::Use(init_op)
                };

                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Assign {
                        lhs: HirPlace::Local(local_id),
                        rhs: rvalue,
                        span,
                    });
                }
            }
        }
    }

    /// 降级赋值语句
    fn lower_assign(&mut self, lhs: &Expr, rhs: &Expr, span: Span) {
        // 1. 降级左值为 Place
        let place = match self.lower_expr(lhs) {
            Some(super::ExprResult::Place(p)) => p,
            Some(super::ExprResult::Operand(_)) => {
                self.diag.emit(Diagnostic::error(
                    ErrorCode::InternalCompilerError.as_u16(),
                    "cannot assign to non-place expression".to_string(),
                    DiagLoc::At(span),
                ));
                return;
            }
            None => return,
        };

        // 2. 降级右值为 Operand
        let rhs_op = match self.lower_expr_to_operand(rhs) {
            Some(op) => op,
            None => return,
        };

        // 如果 rhs_op 是 owned 指针的局部变量，标记为已移动
        if let HirOperand::Place(p) = &rhs_op {
            if let HirPlace::Local(src_local) = **p {
                if self.is_owned_local(src_local) {
                    self.mark_moved(src_local);
                }
            }
        }

        // 3. 插入赋值语句
        if let Some(block) = self.current_block_mut() {
            block.stmts.push(HirStmt::Assign {
                lhs: place,
                rhs: HirRvalue::Use(rhs_op),
                span,
            });
        }
    }

    /// 确保函数所有路径都有终止符
    fn ensure_termination(&mut self, is_void: bool, span: Span) {
        // 先检查是否需要报错
        let needs_error = if let Some(block) = self.current_block_mut() {
            if matches!(block.terminator, HirTerminator::Unreachable) {
                !is_void
            } else {
                false
            }
        } else {
            false
        };

        // 报错（此时不借用 self.blocks）
        if needs_error {
            self.diag.emit(Diagnostic::error(
                ErrorCode::InternalCompilerError.as_u16(),
                "function must return a value".to_string(),
                DiagLoc::At(span),
            ));
        }

        // 设置终止符
        if let Some(block) = self.current_block_mut() {
            if matches!(block.terminator, HirTerminator::Unreachable) {
                if is_void {
                    block.terminator = HirTerminator::Return(None);
                } else {
                    block.terminator = HirTerminator::Unreachable;
                }
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────────
// 单元测试
// ────────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{DiagSink, FileId, Span};
    use crate::frontend::ast::node::{Expr, Func, TypeExpr};
    use crate::frontend::resolve::SymbolTable;
    use crate::frontend::typecheck::TypeContext;
    use crate::middleend::hir::ty::HirType;

    fn make_ctx() -> LoweringContext<'static> {
        let diag = Box::leak(Box::new(DiagSink::new()));
        let symbols = Box::leak(Box::new(SymbolTable::new()));
        let type_ctx = Box::leak(Box::new(TypeContext::new()));

        LoweringContext {
            diag,
            symbols,
            type_ctx,
            struct_map: std::collections::HashMap::new(),
            union_map: std::collections::HashMap::new(),
            union_defs: std::collections::HashMap::new(),
            func_map: std::collections::HashMap::new(),
            loop_stack: Vec::new(),
            label_map: std::collections::HashMap::new(),
            locals: Vec::new(),
            blocks: Vec::new(),
            current_block: None,
            local_map: std::collections::HashMap::new(),
            next_local: 0,
            next_block: 0,
            defer_scopes: Vec::new(),
            scope_stack: Vec::new(),
            moved_locals: std::collections::HashSet::new(),
            current_function_return_type: None,
        }
    }

    #[test]
    fn test_lower_void_func() {
        let mut ctx = make_ctx();
        let span = Span::new(FileId(0), 0, 1);

        let ast = Func {
            is_public: false,
            name: "test".to_string(),
            params: vec![],
            ret: None,
            err: None,
            body: Expr::Block {
                stmts: vec![],
                span,
            },
            span,
        };

        let hir = ctx.lower_func(&ast);

        assert_eq!(hir.params.len(), 0);
        assert!(matches!(hir.ret_type, HirType::Void));
    }

    #[test]
    fn test_lower_let_with_init() {
        let mut ctx = make_ctx();
        let span = Span::new(FileId(0), 0, 1);
        ctx.start_block(span);

        let init_expr = Expr::Int("42".to_string(), span);
        let type_expr = TypeExpr::Named("i32".to_string(), span);

        ctx.lower_let(
            "x",
            Some(&type_expr),
            Some(&init_expr),
            true,
            span,
        );

        assert_eq!(ctx.locals.len(), 1);
        assert_eq!(ctx.locals[0].name.as_deref(), Some("x"));
        assert_eq!(ctx.blocks[0].stmts.len(), 1);
        assert!(matches!(ctx.blocks[0].stmts[0], HirStmt::Assign { .. }));
    }
}
