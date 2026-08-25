//! 表达式降级：AST Expr → HIR Place/Operand/Rvalue。
//!
//! 表达式降级的核心任务：
//! 1. 区分左值（Place）和右值（Operand/Rvalue）
//! 2. 插入临时变量存储中间结果
//! 3. 将复杂表达式分解为语句序列

use super::LoweringContext;
use crate::diag::Span;
use crate::frontend::ast::{Expr, Stmt};
use crate::middleend::hir::{
    HirPlace, HirOperand, HirRvalue, HirStmt, HirTerminator, Const, HirBlock,
    LocalId, BinOp, UnOp, AggregateKind,
    ty::HirType,
};

/// 表达式降级结果：可能是 Place（左值）或 Operand（右值）。
#[derive(Clone, Debug)]
pub enum ExprResult {
    Place(HirPlace),
    Operand(HirOperand),
}

impl ExprResult {
    /// 转换为 Operand（如果是 Place，包装为 Operand::Place）
    pub fn to_operand(self, _ctx: &mut LoweringContext) -> Option<HirOperand> {
        match self {
            ExprResult::Operand(op) => Some(op),
            ExprResult::Place(place) => Some(HirOperand::Place(Box::new(place))),
        }
    }

    /// 获取表达式结果的类型
    pub fn ty(&self, ctx: &mut LoweringContext) -> HirType {
        match self {
            ExprResult::Operand(op) => op.ty(ctx),
            ExprResult::Place(place) => {
                use crate::diag::FileId;
                let span = Span::new(FileId(0), 0, 0);
                ctx.infer_place_type(place, span)
            }
        }
    }
}

impl HirOperand {
    /// 获取操作数的类型
    pub fn ty(&self, ctx: &mut LoweringContext) -> HirType {
        match self {
            HirOperand::Place(place) => {
                // 使用简单的 Span 占位符
                use crate::diag::FileId;
                let span = Span::new(FileId(0), 0, 0);
                ctx.infer_place_type(place, span)
            },
            HirOperand::Const(c) => match c {
                Const::Int(_) => HirType::i32(),
                Const::Float(_) => HirType::f64(),
                Const::Bool(_) => HirType::Bool,
                Const::Str(_) => HirType::Str,
                Const::Void => HirType::Void,
                Const::Nil => HirType::Void,
            },
            HirOperand::FuncRef(_func_id) => {
                // 函数引用的类型推断需要函数签名表
                // 当前 HIR 阶段暂不支持完整的函数指针类型
                // 后续需要在 LoweringContext 中维护 func_id → 签名的映射
                HirType::Void
            }
        }
    }
}

impl<'a> LoweringContext<'a> {
    /// 降级表达式为 Operand（右值）。
    /// 如果表达式产生 Place，会生成临时变量加载它。
    pub fn lower_expr_to_operand(&mut self, expr: &Expr) -> Option<HirOperand> {
        match self.lower_expr(expr)? {
            ExprResult::Operand(op) => Some(op),
            ExprResult::Place(place) => {
                // 生成临时变量加载 Place
                let temp = self.fresh_local();
                let ty = self.infer_place_type(&place, expr.span());

                self.locals.push(crate::middleend::hir::HirLocal {
                    name: None,
                    ty,
                    span: expr.span(),
                });

                // 添加赋值语句：temp = place
                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Assign {
                        lhs: HirPlace::Local(temp),
                        rhs: HirRvalue::Use(HirOperand::Place(Box::new(place))),
                        span: expr.span(),
                    });
                }

                Some(HirOperand::Place(Box::new(HirPlace::Local(temp))))
            }
        }
    }

    /// 降级表达式（可能返回 Place 或 Operand）。
    pub fn lower_expr(&mut self, expr: &Expr) -> Option<ExprResult> {
        match expr {
            // 字面量直接转为常量 Operand
            Expr::Int(value, _span) => {
                let parsed = value.parse::<i64>().unwrap_or(0);
                Some(ExprResult::Operand(HirOperand::Const(
                    Const::Int(parsed as i128)
                )))
            }

            Expr::Float(value, _span) => {
                let parsed = value.parse::<f64>().unwrap_or(0.0);
                Some(ExprResult::Operand(HirOperand::Const(
                    Const::Float(parsed)
                )))
            }

            Expr::Bool(value, _span) => {
                Some(ExprResult::Operand(HirOperand::Const(
                    Const::Bool(*value)
                )))
            }

            Expr::Nil(_span) => {
                Some(ExprResult::Operand(HirOperand::Const(
                    Const::Void
                )))
            }

            // 路径表达式（变量引用或函数引用）→ Place
            Expr::Path(segments, span) => {
                if segments.len() == 1 {
                    let name = &segments[0];
                    // 先尝试解析为局部变量
                    if let Some(local_id) = self.lookup_local(name) {
                        Some(ExprResult::Place(HirPlace::Local(local_id)))
                    } else if let Some(func_id) = self.lookup_func(name) {
                        // 解析为函数引用，转换为 FuncRef operand
                        Some(ExprResult::Operand(HirOperand::FuncRef(func_id)))
                    } else {
                        self.diag.emit(crate::diag::Diagnostic::error(
                            3001,
                            format!("未定义的变量: {}", name),
                            crate::diag::DiagLoc::At(*span),
                        ));
                        None
                    }
                } else {
                    self.diag.emit(crate::diag::Diagnostic::error(
                        2009,
                        "多段路径在 stage0 中尚未实现",
                        crate::diag::DiagLoc::At(*span),
                    ));
                    None
                }
            }

            // 字段访问：base.field
            Expr::Field { base, name, span } => {
                let base_place = match self.lower_expr(base)? {
                    ExprResult::Place(p) => p,
                    ExprResult::Operand(op) => {
                        // Operand 不能直接投影，需要先存到临时变量
                        let temp = self.create_temp_for_operand(op, *span);
                        HirPlace::Local(temp)
                    }
                };

                let field_idx = self.resolve_field_index(&base_place, name, *span)?;

                Some(ExprResult::Place(HirPlace::Field {
                    base: Box::new(base_place),
                    field: field_idx,
                }))
            }

            // 数组索引：base[index]
            Expr::Index { base, index, span } => {
                let base_place = match self.lower_expr(base)? {
                    ExprResult::Place(p) => p,
                    ExprResult::Operand(op) => {
                        let temp = self.create_temp_for_operand(op, *span);
                        HirPlace::Local(temp)
                    }
                };

                let index_op = self.lower_expr_to_operand(index)?;

                Some(ExprResult::Place(HirPlace::Index {
                    base: Box::new(base_place),
                    index: Box::new(index_op),
                }))
            }

            // 解引用：expr^
            Expr::Deref(inner, span) => {
                let inner_place = match self.lower_expr(inner)? {
                    ExprResult::Place(p) => p,
                    ExprResult::Operand(op) => {
                        let temp = self.create_temp_for_operand(op, *span);
                        HirPlace::Local(temp)
                    }
                };

                Some(ExprResult::Place(HirPlace::Deref(Box::new(inner_place))))
            }

            // 二元运算
            Expr::Binary { op, lhs, rhs, span } => {
                let lhs_op = self.lower_expr_to_operand(lhs)?;
                let rhs_op = self.lower_expr_to_operand(rhs)?;

                let hir_op = self.convert_binop(op, *span)?;
                let rvalue = HirRvalue::BinaryOp {
                    op: hir_op,
                    lhs: lhs_op,
                    rhs: rhs_op,
                };

                // 生成临时变量存储结果
                let temp = self.fresh_local();
                // 比较与逻辑运算产出 bool，其余算术/位运算沿用左操作数类型
                let result_ty = match hir_op {
                    BinOp::Eq
                    | BinOp::Ne
                    | BinOp::Lt
                    | BinOp::Le
                    | BinOp::Gt
                    | BinOp::Ge
                    | BinOp::LogicAnd
                    | BinOp::LogicOr => crate::middleend::hir::ty::HirType::Bool,
                    _ => self.infer_expr_type(lhs),
                };

                self.locals.push(crate::middleend::hir::HirLocal {
                    name: None,
                    ty: result_ty,
                    span: *span,
                });

                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Assign {
                        lhs: HirPlace::Local(temp),
                        rhs: rvalue,
                        span: *span,
                    });
                }

                Some(ExprResult::Operand(HirOperand::Place(Box::new(HirPlace::Local(temp)))))
            }

            // 一元运算
            Expr::Unary { op, operand, span } => {
                let operand_hir = self.lower_expr_to_operand(operand)?;
                let hir_op = self.convert_unop(op, *span)?;

                let rvalue = HirRvalue::UnaryOp {
                    op: hir_op,
                    operand: operand_hir,
                };

                let temp = self.fresh_local();
                // 逻辑非产出 bool，取负/按位取反沿用操作数类型
                let result_ty = match hir_op {
                    UnOp::Not => crate::middleend::hir::ty::HirType::Bool,
                    _ => self.infer_expr_type(operand),
                };

                self.locals.push(crate::middleend::hir::HirLocal {
                    name: None,
                    ty: result_ty,
                    span: *span,
                });

                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Assign {
                        lhs: HirPlace::Local(temp),
                        rhs: rvalue,
                        span: *span,
                    });
                }

                Some(ExprResult::Operand(HirOperand::Place(Box::new(HirPlace::Local(temp)))))
            }

            // 函数调用（简化版，完整版在 func.rs）
            Expr::Call { callee, args, span } => {
                let func_op = self.lower_expr_to_operand(callee)?;
                let arg_ops: Vec<_> = args.iter()
                    .filter_map(|a| self.lower_expr_to_operand(a))
                    .collect();

                if arg_ops.len() != args.len() {
                    return None;  // 有参数降级失败
                }

                // 生成临时变量存储返回值
                let temp = self.fresh_local();
                let ret_ty = self.infer_call_return_type(callee);

                self.locals.push(crate::middleend::hir::HirLocal {
                    name: None,
                    ty: ret_ty.clone(),
                    span: *span,
                });

                let dest = if ret_ty == HirType::Void {
                    None
                } else {
                    Some(HirPlace::Local(temp))
                };

                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Call {
                        dest,
                        func: func_op,
                        args: arg_ops,
                        span: *span,
                    });
                }

                if ret_ty == HirType::Void {
                    Some(ExprResult::Operand(HirOperand::Const(Const::Void)))
                } else {
                    Some(ExprResult::Operand(HirOperand::Place(Box::new(HirPlace::Local(temp)))))
                }
            }

            // 块表达式：降级为语句序列，末尾表达式语句作为块的值
            Expr::Block { stmts, span } => {
                // 进入新作用域
                self.enter_scope();

                // 末尾的表达式语句是块的值，需单独降级以取回结果
                let (body, tail) = match stmts.split_last() {
                    Some((Stmt::Expr(tail_expr), body)) => (body, Some(tail_expr)),
                    _ => (&stmts[..], None),
                };

                // 降级块内语句
                self.lower_block_stmts(body)?;

                // 在 drop/defer 之前求出尾值
                let tail_value = match tail {
                    Some(tail_expr) => self.lower_expr_to_operand(tail_expr)?,
                    None => HirOperand::Const(Const::Void),
                };

                // 退出作用域，插入 defer 和 Drop 语句
                let (owned_locals, defers) = self.exit_scope();

                // 先插入 Drop（先析构对象）
                for local_id in owned_locals.iter().rev() {
                    // 跳过已移动的变量（避免 double drop）
                    if !self.is_moved(*local_id) {
                        if let Some(block) = self.current_block_mut() {
                            block.stmts.push(HirStmt::Drop {
                                place: HirPlace::Local(*local_id),
                                span: *span,
                            });
                        }
                    }
                }

                // 后插入 defer（后清理，此时对象已析构）
                for (defer_expr, _span) in defers.iter().rev() {
                    let _ = self.lower_expr(defer_expr);
                }

                Some(ExprResult::Operand(tail_value))
            }

            // 分支表达式：降级为 Switch terminator + arm blocks
            Expr::Branch { scrutinee, arms, span } => {
                if let Some(discr) = scrutinee {
                    self.lower_branch(discr, arms, *span)
                } else {
                    let true_expr = Expr::Bool(true, *span);
                    self.lower_branch(&true_expr, arms, *span)
                }
            }

            // 循环表达式：降级为 loop blocks
            Expr::Loop { label, subject, body, span } => {
                // subject 为循环条件
                let cond = subject.as_ref().map(|s| s.as_ref());
                let label_ref = label.as_ref().map(|s| s.as_str());
                self.lower_loop(label_ref, cond, body, *span)
            }

            // 控制流语句
            Expr::Ret(value, span) => {
                self.lower_ret(value.as_ref().map(|b| b.as_ref()), *span)
            }

            Expr::Stop { label, span } => {
                self.lower_stop(label.as_ref().map(|s| s.as_str()), *span)
            }

            Expr::Skip { label, span } => {
                self.lower_skip(label.as_deref(), *span)
            }

            Expr::Jmp { target, label, span } => {
                let _ = target;
                self.lower_jmp(label.as_deref(), *span)
            }

            Expr::Str(s, _span) => {
                // 字符串字面量降级为常量操作数
                Some(ExprResult::Operand(HirOperand::Const(Const::Str(s.clone()))))
            }

            Expr::Propagate(inner, span) => {
                // 错误传播 `expr!` 降级为判别式检查 + 条件分支
                // 展开模式：
                //   let tmp = inner;
                //   if discriminant(tmp) == ERROR_VARIANT {
                //       return ExtractPayload(tmp, ERROR_VARIANT);
                //   }
                //   ExtractPayload(tmp, SUCCESS_VARIANT)

                self.lower_propagate(inner, *span)
            }

            // 聚合类型构造
            Expr::StructLit { name, fields, span } => {
                self.lower_struct_lit(name, fields, *span)
            }

            Expr::ArrayLit { elements, span } => {
                self.lower_array_lit(elements, *span)
            }

            Expr::VariantConstructor { name, payload, span } => {
                // `.Ok` / `.Err` 只有在没有任何用户定义联合声明该变体时，
                // 才视为内建错误联合；否则按普通联合处理，避免用户定义的
                // `Result :: .Ok(T) | .Err(E)` 被错误地按错误联合布局构造。
                let is_user_union_variant = self
                    .union_defs
                    .values()
                    .any(|def| def.variants.iter().any(|v| &v.name == name));

                if (name == "Ok" || name == "Err") && !is_user_union_variant {
                    // 对于错误联合类型，直接生成聚合构造
                    // variant_index: Ok=0, Err=1
                    let variant_index = if name == "Ok" { 0 } else { 1 };

                    // 降级 payload（如果有）
                    let fields = if let Some(p) = payload {
                        vec![self.lower_expr_to_operand(p)?]
                    } else {
                        vec![]
                    };

                    // 分配临时变量存储结果
                    let temp = self.fresh_local();

                    // 错误联合的类型来自当前函数的返回类型
                    let ty = self
                        .current_function_return_type
                        .clone()
                        .unwrap_or(HirType::Void);

                    // 使用 ErrorUnion aggregate kind，携带声明类型以便后端
                    // 按 ok/err 两侧的最大尺寸分配 payload 槽位
                    let aggregate = HirRvalue::Aggregate {
                        kind: AggregateKind::ErrorUnion(variant_index, ty.clone()),
                        fields,
                    };

                    self.locals.push(crate::middleend::hir::HirLocal {
                        name: None,
                        ty,
                        span: *span,
                    });

                    if let Some(block) = self.current_block_mut() {
                        block.stmts.push(HirStmt::Assign {
                            lhs: HirPlace::Local(temp),
                            rhs: aggregate,
                            span: *span,
                        });
                    }

                    return Some(ExprResult::Operand(HirOperand::Place(Box::new(HirPlace::Local(temp)))));
                }

                // 查找变体所属的联合类型
                let variant_index = self.find_variant_index(name)?;

                // 获取联合类型 ID（假设 find_variant_index 返回的是全局唯一的）
                // 这里需要从上下文推断联合类型，暂时通过遍历 union_defs 查找
                let mut union_id = None;
                for (union_name, def) in &self.union_defs {
                    if def.variants.iter().any(|v| &v.name == name) {
                        union_id = self.union_map.get(union_name).copied();
                        crate::trace!("DEBUG lower_expr: found union_name={}, union_id={:?}", union_name, union_id);
                        break;
                    }
                }

                let union_id = match union_id {
                    Some(id) => id,
                    None => {
                        crate::trace!("DEBUG lower_expr: ERROR - no union found for variant '{}'", name);
                        self.diag.emit(crate::diag::Diagnostic::error(
                            crate::diag::ErrorCode::InternalCompilerError.as_u16(),
                            format!("无法找到变体 '.{}' 所属的联合类型", name),
                            crate::diag::DiagLoc::At(*span),
                        ));
                        return None;
                    }
                };

                // 降级 payload（如果有）
                let fields = if let Some(p) = payload {
                    vec![self.lower_expr_to_operand(p)?]
                } else {
                    vec![]
                };

                // 生成联合体聚合构造
                let aggregate = HirRvalue::Aggregate {
                    kind: AggregateKind::Union(union_id, variant_index),
                    fields,
                };

                // 分配临时变量存储结果
                let temp = self.fresh_local();
                let ty = HirType::Union(union_id);

                self.locals.push(crate::middleend::hir::HirLocal {
                    name: None,
                    ty,
                    span: *span,
                });

                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Assign {
                        lhs: HirPlace::Local(temp),
                        rhs: aggregate,
                        span: *span,
                    });
                }

                Some(ExprResult::Operand(HirOperand::Place(Box::new(HirPlace::Local(temp)))))
            }
        }
    }

    // ────── 辅助方法 ──────

    /// 为 Operand 创建临时变量。
    fn create_temp_for_operand(&mut self, op: HirOperand, span: Span) -> LocalId {
        let temp = self.fresh_local();
        let ty = self.infer_operand_type(&op, span);

        self.locals.push(crate::middleend::hir::HirLocal {
            name: None,
            ty,
            span,
        });

        if let Some(block) = self.current_block_mut() {
            block.stmts.push(HirStmt::Assign {
                lhs: HirPlace::Local(temp),
                rhs: HirRvalue::Use(op),
                span,
            });
        }

        temp
    }

    /// 解析字段索引。
    /// 从 place 的类型中查询结构体定义，返回字段在字段列表中的索引。
    fn resolve_field_index(&mut self, place: &HirPlace, field_name: &str, span: Span) -> Option<usize> {
        // 1. 获取 place 的类型
        let place_ty = self.infer_place_type(place, span);

        // 2. 从类型中提取结构体 ID，然后查询原始名称
        let struct_id = match &place_ty {
            HirType::Struct(id) => *id,
            _ => {
                self.diag.emit(crate::diag::Diagnostic::error(
                    2009,
                    format!("字段访问的基础类型必须是结构体，实际: {:?}", place_ty),
                    crate::diag::DiagLoc::At(span),
                ));
                return None;
            }
        };

        // 3. 从 struct_map 反查结构体名称
        let struct_name = self.struct_map.iter()
            .find(|(_name, id)| **id == struct_id)
            .map(|(name, _)| name.clone());

        let struct_name = match struct_name {
            Some(name) => name,
            None => {
                self.diag.emit(crate::diag::Diagnostic::error(
                    2010,
                    format!("未找到结构体 ID: {:?}", struct_id),
                    crate::diag::DiagLoc::At(span),
                ));
                return None;
            }
        };

        // 4. 查询结构体字段列表
        let fields = self.type_ctx.get_struct(&struct_name);
        if fields.is_none() {
            self.diag.emit(crate::diag::Diagnostic::error(
                2011,
                format!("未找到结构体定义: {}", struct_name),
                crate::diag::DiagLoc::At(span),
            ));
            return None;
        }

        // 5. 查找字段索引
        let field_list = fields.unwrap();
        for (idx, (name, _ty)) in field_list.iter().enumerate() {
            if name == field_name {
                return Some(idx);
            }
        }

        self.diag.emit(crate::diag::Diagnostic::error(
            2012,
            format!("结构体 {} 没有字段 {}", struct_name, field_name),
            crate::diag::DiagLoc::At(span),
        ));
        None
    }

    /// 推断 Place 的类型。
    fn infer_place_type(&mut self, place: &HirPlace, span: Span) -> HirType {
        match place {
            HirPlace::Local(local_id) => {
                // 从 locals 列表查询变量类型
                if let Some(local) = self.locals.get(local_id.0) {
                    local.ty.clone()
                } else {
                    HirType::Void
                }
            }
            HirPlace::Field { base, field } => {
                // 递归获取 base 的类型，然后查询字段类型
                let base_ty = self.infer_place_type(base, span);
                match base_ty {
                    HirType::Struct(struct_id) => {
                        // 从 struct_map 反查结构体名称
                        if let Some((struct_name, _)) = self.struct_map.iter().find(|(_, id)| **id == struct_id) {
                            if let Some(fields) = self.type_ctx.get_struct(struct_name) {
                                if let Some((_name, ty)) = fields.get(*field) {
                                    // 将 frontend Type 转换为 HirType
                                    use super::ty::TypeConverter;
                                    let mut conv = TypeConverter::new(&self.struct_map, &self.union_map, self.diag);
                                    return conv.convert(ty, span);
                                }
                            }
                        }
                        HirType::Void
                    }
                    _ => HirType::Void,
                }
            }
            HirPlace::Index { base, .. } => {
                // 数组/切片索引：获取元素类型
                let base_ty = self.infer_place_type(base, span);
                match base_ty {
                    HirType::Array { elem, .. } => *elem,
                    HirType::Slice { elem } => *elem,
                    _ => HirType::Void,
                }
            }
            HirPlace::Deref(inner) => {
                // 解引用：获取指针指向的类型
                let inner_ty = self.infer_place_type(inner, span);
                match inner_ty {
                    HirType::Ptr { pointee, .. } => *pointee,
                    _ => HirType::Void,
                }
            }
        }
    }

    /// 推断 Operand 的类型。
    fn infer_operand_type(&mut self, op: &HirOperand, span: Span) -> HirType {
        match op {
            HirOperand::Const(c) => match c {
                Const::Int(_) => HirType::i32(),
                Const::Float(_) => HirType::f64(),
                Const::Bool(_) => HirType::Bool,
                Const::Void => HirType::Void,
                Const::Str(_) => HirType::Str,
                Const::Nil => HirType::Void,
            },
            HirOperand::Place(place) => {
                self.infer_place_type(place, span)
            }
            HirOperand::FuncRef(func_id) => {
                // 从 TypeContext 查询函数签名
                if let Some((func_name, _)) = self.func_map.iter().find(|(_, id)| **id == *func_id) {
                    if let Some(func_ty) = self.type_ctx.get_func(func_name) {
                        use crate::frontend::typecheck::Type;
                        if let Type::Func { params, ret, .. } = func_ty {
                            use super::ty::TypeConverter;
                            let mut conv = TypeConverter::new(&self.struct_map, &self.union_map, self.diag);
                            let hir_params = params.iter()
                                .map(|p| conv.convert(p, span))
                                .collect();
                            let hir_ret = conv.convert(&ret, span);
                            return HirType::FnPtr {
                                params: hir_params,
                                ret: Box::new(hir_ret),
                            };
                        }
                    }
                }
                HirType::Void
            }
        }
    }

    /// 推断表达式类型（从 AST 表达式推断 HIR 类型）。
    ///
    /// 注意：这是简化的类型推断实现，仅基于语法结构推断。
    /// 完整的类型推断需要：
    /// - 统一的类型变量求解
    /// - 多态函数实例化
    /// - 上下文相关的类型推导
    pub(super) fn infer_expr_type(&mut self, expr: &Expr) -> HirType {
        match expr {
            Expr::Int(_, _) => HirType::i32(),
            Expr::Float(_, _) => HirType::f64(),
            Expr::Bool(_, _) => HirType::Bool,
            Expr::Str(_, _) => HirType::Str,
            Expr::Nil(_) => HirType::Void,

            Expr::Path(segments, _) if segments.len() == 1 => {
                let name = &segments[0];
                if let Some(local_id) = self.lookup_local(name) {
                    if let Some(local) = self.locals.get(local_id.0) {
                        return local.ty.clone();
                    }
                }
                HirType::Void
            }

            Expr::Binary { op, lhs, .. } => {
                match *op {
                    "==" | "!=" | "<" | "<=" | ">" | ">=" => HirType::Bool,
                    _ => self.infer_expr_type(lhs),
                }
            }

            Expr::Unary { operand, .. } => {
                // 简化：使用操作数类型
                self.infer_expr_type(operand)
            }

            Expr::Index { base, .. } => {
                let base_ty = self.infer_expr_type(base);
                match base_ty {
                    HirType::Array { elem, .. } => *elem,
                    HirType::Slice { elem } => *elem,
                    _ => HirType::Void,
                }
            }

            Expr::Deref(inner, _) => {
                let inner_ty = self.infer_expr_type(inner);
                match inner_ty {
                    HirType::Ptr { pointee, .. } => *pointee,
                    _ => HirType::Void,
                }
            }

            Expr::Field { base, name, span } => {
                let base_ty = self.infer_expr_type(base);
                match base_ty {
                    HirType::Struct(struct_id) => {
                        // 从 struct_map 反查结构体名称
                        if let Some(struct_name) = self.struct_map.iter()
                            .find(|(_, id)| **id == struct_id)
                            .map(|(name, _)| name.clone())
                        {
                            // 使用 type_ctx.get_struct 查询字段列表
                            if let Some(fields) = self.type_ctx.get_struct(&struct_name) {
                                if let Some((_field_name, field_ty)) = fields.iter()
                                    .find(|(fname, _)| fname == name)
                                {
                                    // field_ty 是 frontend Type，需要转换为 HirType
                                    let mut conv = super::ty::TypeConverter::new(
                                        &self.struct_map,
                                        &self.union_map,
                                        self.diag,
                                    );
                                    return conv.convert(field_ty, *span);
                                }
                            }
                        }
                        HirType::Void
                    }
                    _ => HirType::Void,
                }
            }

            Expr::Call { callee, .. } => {
                // 使用专门的函数调用返回类型推断
                self.infer_call_return_type(callee)
            }

            Expr::ArrayLit { elements, .. } => {
                if elements.is_empty() {
                    // 空数组默认为 [Void; 0]
                    HirType::Array {
                        elem: Box::new(HirType::Void),
                        len: 0,
                    }
                } else {
                    // 从首个元素推断元素类型
                    let elem_ty = self.infer_expr_type(&elements[0]);
                    HirType::Array {
                        elem: Box::new(elem_ty),
                        len: elements.len(),
                    }
                }
            }

            Expr::VariantConstructor { name, .. } => {
                // 查找变体所属的联合类型
                for (union_name, def) in &self.union_defs {
                    if def.variants.iter().any(|v| &v.name == name) {
                        if let Some(union_id) = self.union_map.get(union_name).copied() {
                            return HirType::Union(union_id);
                        }
                    }
                }
                HirType::Void
            }

            Expr::Propagate(inner, _) => {
                // ? 表达式的类型是内部表达式错误联合的 ok 类型
                let inner_ty = self.infer_expr_type(inner);
                match inner_ty {
                    HirType::ErrUnion { ok, .. } => *ok,
                    _ => HirType::Void,
                }
            }

            _ => HirType::Void,
        }
    }

    /// 降级块中的语句序列。
    fn lower_block_stmts(&mut self, stmts: &[Stmt]) -> Option<()> {
        for stmt in stmts {
            self.lower_stmt(stmt);
        }
        Some(())
    }


    /// 推断函数调用返回类型。
    fn infer_call_return_type(&mut self, callee: &Expr) -> HirType {
        // 从 callee 提取函数名
        let func_name = match callee {
            Expr::Path(segments, _) if segments.len() == 1 => &segments[0],
            _ => return HirType::Void,  // 复杂表达式或多段路径暂不支持
        };

        // 从 TypeContext 查询函数签名
        if let Some(func_ty) = self.type_ctx.get_func(func_name) {
            use crate::frontend::typecheck::Type;
            if let Type::Func { ret, .. } = func_ty {
                // 将 frontend Type 转换为 HIR Type
                use super::ty::TypeConverter;
                let mut conv = TypeConverter::new(&self.struct_map, &self.union_map, self.diag);
                return conv.convert(&ret, callee.span());
            }
        }

        HirType::Void
    }

    /// 转换二元运算符。
    fn convert_binop(&mut self, op: &str, span: Span) -> Option<BinOp> {
        match op {
            "+" => Some(BinOp::Add),
            "-" => Some(BinOp::Sub),
            "*" => Some(BinOp::Mul),
            "/" => Some(BinOp::Div),
            "%" => Some(BinOp::Rem),
            "==" => Some(BinOp::Eq),
            "!=" => Some(BinOp::Ne),
            "<" => Some(BinOp::Lt),
            "<=" => Some(BinOp::Le),
            ">" => Some(BinOp::Gt),
            ">=" => Some(BinOp::Ge),
            "&" => Some(BinOp::BitAnd),
            "|" => Some(BinOp::BitOr),
            "^" => Some(BinOp::BitXor),
            "<<" => Some(BinOp::Shl),
            ">>" => Some(BinOp::Shr),
            _ => {
                self.diag.emit(crate::diag::Diagnostic::error(
                    2009,
                    format!("未知二元运算符: {}", op),
                    crate::diag::DiagLoc::At(span),
                ));
                None
            }
        }
    }

    /// 转换一元运算符。
    fn convert_unop(&mut self, op: &str, span: Span) -> Option<UnOp> {
        match op {
            "-" => Some(UnOp::Neg),
            "!" => Some(UnOp::Not),
            "~" => Some(UnOp::BitNot),
            _ => {
                self.diag.emit(crate::diag::Diagnostic::error(
                    2009,
                    format!("未知一元运算符: {}", op),
                    crate::diag::DiagLoc::At(span),
                ));
                None
            }
        }
    }

    /// 降级错误传播表达式 `expr!`。
    ///
    /// 展开为：
    /// ```text
    /// let tmp = expr;
    /// if discriminant(tmp) == ERROR_VARIANT {
    ///     return ExtractPayload(tmp, ERROR_VARIANT);
    /// }
    /// ExtractPayload(tmp, SUCCESS_VARIANT)
    /// ```
    fn lower_propagate(&mut self, inner: &Expr, span: Span) -> Option<ExprResult> {
        // 1. 降级内部表达式到临时变量
        let inner_op = self.lower_expr_to_operand(inner)?;
        let tmp = self.fresh_local();
        let inner_ty = inner_op.ty(self);

        self.locals.push(crate::middleend::hir::HirLocal {
            name: None,
            ty: inner_ty.clone(),
            span,
        });

        // 赋值：tmp = inner
        if let Some(block) = self.current_block_mut() {
            block.stmts.push(HirStmt::Assign {
                lhs: HirPlace::Local(tmp),
                rhs: HirRvalue::Use(inner_op),
                span,
            });
        }

        // 2. 检查类型是否为错误联合
        match &inner_ty {
            HirType::ErrUnion { ok, err } => {
                // 错误联合类型 T ! E
                // 展开为：
                //   if tag == 0 (ok) { return ok_value }
                //   else { return err_value }

                // 创建判别式检查
                let discr_place = HirPlace::Local(tmp);
                let tag_place = HirPlace::Field {
                    base: Box::new(discr_place.clone()),
                    field: 0,
                };

                let discr_tmp = self.fresh_local();
                self.locals.push(crate::middleend::hir::HirLocal {
                    name: None,
                    ty: HirType::Int { width: 8, signed: false },
                    span,
                });

                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Assign {
                        lhs: HirPlace::Local(discr_tmp),
                        rhs: HirRvalue::Use(HirOperand::Place(Box::new(tag_place))),
                        span,
                    });
                }

                // 创建条件分支块
                let error_block = self.fresh_block();
                let success_block = self.fresh_block();
                let merge_block = self.fresh_block();

                // 设置当前块的终结器：if tag == 1 (error)
                if let Some(block) = self.current_block_mut() {
                    block.terminator = HirTerminator::Switch {
                        discr: HirOperand::Place(Box::new(HirPlace::Local(discr_tmp))),
                        targets: vec![(1u64, error_block)],
                        otherwise: success_block,
                    };
                }

                // error_block: 提取错误并返回
                self.current_block = Some(error_block);
                self.blocks.push(HirBlock {
                    id: error_block,
                    stmts: Vec::new(),
                    terminator: HirTerminator::Unreachable,
                    span,
                });

                let err_payload_tmp = self.fresh_local();
                let payload_place = HirPlace::Field {
                    base: Box::new(discr_place.clone()),
                    field: 1,
                };

                self.locals.push(crate::middleend::hir::HirLocal {
                    name: None,
                    ty: (**err).clone(),
                    span,
                });

                // 构造完整的错误联合返回值
                let err_union_tmp = self.fresh_local();
                self.locals.push(crate::middleend::hir::HirLocal {
                    name: None,
                    ty: inner_ty.clone(),
                    span,
                });

                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Assign {
                        lhs: HirPlace::Local(err_payload_tmp),
                        rhs: HirRvalue::Use(HirOperand::Place(Box::new(payload_place.clone()))),
                        span,
                    });
                    // 重新打包为错误联合（Err 变体）
                    block.stmts.push(HirStmt::Assign {
                        lhs: HirPlace::Local(err_union_tmp),
                        rhs: HirRvalue::Aggregate {
                            // Err variant；携带声明类型，保证重新打包时
                            // payload 槽位与被传播的联合布局一致
                            kind: crate::middleend::hir::AggregateKind::ErrorUnion(
                                1,
                                inner_ty.clone(),
                            ),
                            fields: vec![HirOperand::Place(Box::new(HirPlace::Local(err_payload_tmp)))],
                        },
                        span,
                    });
                    block.terminator = HirTerminator::Return(
                        Some(HirOperand::Place(Box::new(HirPlace::Local(err_union_tmp))))
                    );
                }

                // success_block: 提取成功值
                self.current_block = Some(success_block);
                self.blocks.push(HirBlock {
                    id: success_block,
                    stmts: Vec::new(),
                    terminator: HirTerminator::Unreachable,
                    span,
                });

                let ok_payload_tmp = self.fresh_local();

                self.locals.push(crate::middleend::hir::HirLocal {
                    name: None,
                    ty: (**ok).clone(),
                    span,
                });

                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Assign {
                        lhs: HirPlace::Local(ok_payload_tmp),
                        rhs: HirRvalue::Use(HirOperand::Place(Box::new(payload_place))),
                        span,
                    });
                    block.terminator = HirTerminator::Goto(merge_block);
                }

                // 继续在 merge_block
                self.current_block = Some(merge_block);
                self.blocks.push(HirBlock {
                    id: merge_block,
                    stmts: Vec::new(),
                    terminator: HirTerminator::Unreachable,
                    span,
                });

                return Some(ExprResult::Place(HirPlace::Local(ok_payload_tmp)));
            }
            HirType::Union(union_id) => {
                // 旧的联合体类型处理（保持向后兼容）
                let _union_id = *union_id;

                // 获取联合体变体信息
                // 查找联合体名称（通过 union_id 反查）
                let union_name = self.union_map.iter()
                    .find(|(_, id)| **id == _union_id)
                    .map(|(name, _)| name.clone());

                let (success_variant, error_variant) = if let Some(ref _u_name) = union_name {
                    // 查询 Ok 和 Err 变体的实际索引
                    let ok_idx = self.find_variant_index("Ok").unwrap_or(0);
                    let err_idx = self.find_variant_index("Err").unwrap_or(1);
                    (ok_idx, err_idx)
                } else {
                    // 回退到假设索引
                    (0, 1)
                };

                // 4. 创建判别式检查
                let discr_place = HirPlace::Local(tmp);
                let discr_rvalue = HirRvalue::Discriminant(discr_place.clone());

                let discr_tmp = self.fresh_local();
                self.locals.push(crate::middleend::hir::HirLocal {
                    name: None,
                    ty: HirType::Int { width: 32, signed: false },
                    span,
                });

                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Assign {
                        lhs: HirPlace::Local(discr_tmp),
                        rhs: discr_rvalue,
                        span,
                    });
                }

                // 5. 创建条件分支块
                let then_block = self.fresh_block();
                let else_block = self.fresh_block();
                let merge_block = self.fresh_block();

                // 设置当前块的终结器：if discriminant == ERROR_VARIANT
                if let Some(block) = self.current_block_mut() {
                    block.terminator = HirTerminator::Switch {
                        discr: HirOperand::Place(Box::new(HirPlace::Local(discr_tmp))),
                        targets: vec![(error_variant as u64, then_block)],
                        otherwise: else_block,
                    };
                }

                // 6. then_block: 提取错误并返回
                self.current_block = Some(then_block);
                let err_payload_tmp = self.fresh_local();

                // 查询错误变体的实际类型
                let err_ty = if let Some(ref u_name) = union_name {
                    self.get_variant_payload_type(u_name, error_variant)
                        .unwrap_or_else(|| HirType::Void)
                } else {
                    HirType::Void
                };

                self.locals.push(crate::middleend::hir::HirLocal {
                    name: None,
                    ty: err_ty,
                    span,
                });

                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Assign {
                        lhs: HirPlace::Local(err_payload_tmp),
                        rhs: HirRvalue::ExtractPayload {
                            place: discr_place.clone(),
                            variant_index: error_variant,
                        },
                        span,
                    });
                    block.terminator = HirTerminator::Return(
                        Some(HirOperand::Place(Box::new(HirPlace::Local(err_payload_tmp))))
                    );
                }

                // 7. else_block: 提取成功值
                self.current_block = Some(else_block);
                let ok_payload_tmp = self.fresh_local();

                // 查询成功变体的实际类型
                let ok_ty = if let Some(ref u_name) = union_name {
                    self.get_variant_payload_type(u_name, success_variant)
                        .unwrap_or_else(|| HirType::Void)
                } else {
                    HirType::Void
                };

                self.locals.push(crate::middleend::hir::HirLocal {
                    name: None,
                    ty: ok_ty,
                    span,
                });

                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Assign {
                        lhs: HirPlace::Local(ok_payload_tmp),
                        rhs: HirRvalue::ExtractPayload {
                            place: discr_place,
                            variant_index: success_variant,
                        },
                        span,
                    });
                    block.terminator = HirTerminator::Goto(merge_block);
                }

                // 8. 继续在 merge_block
                self.current_block = Some(merge_block);

                Some(ExprResult::Place(HirPlace::Local(ok_payload_tmp)))
            }
            _ => {
                // 不是错误联合或联合体，降级为简单求值（向后兼容）
                self.diag.emit(crate::diag::Diagnostic::warning(
                    3001,
                    "错误传播 '!' 应用于非错误联合类型，降级为直接求值",
                    crate::diag::DiagLoc::At(span),
                ));
                Some(ExprResult::Place(HirPlace::Local(tmp)))
            }
        }
    }

    /// 降级结构体字面量。
    fn lower_struct_lit(
        &mut self,
        name: &str,
        fields: &[(String, Expr)],
        span: Span,
    ) -> Option<ExprResult> {
        // 查找结构体 ID
        let struct_id = match self.struct_map.get(name) {
            Some(&id) => id,
            None => {
                self.diag.emit(crate::diag::Diagnostic::error(
                    2011,
                    format!("未知结构体: {}", name),
                    crate::diag::DiagLoc::At(span),
                ));
                return None;
            }
        };

        // 降级所有字段表达式
        let mut field_operands = Vec::new();
        for (_field_name, field_expr) in fields {
            let operand = self.lower_expr_to_operand(field_expr)?;
            field_operands.push(operand);
        }

        // 创建临时变量存储聚合结果
        let temp = self.fresh_local();
        let ty = HirType::Struct(struct_id);

        self.locals.push(crate::middleend::hir::HirLocal {
            name: None,
            ty,
            span,
        });

        // 添加赋值语句：temp = Struct { fields }
        if let Some(block) = self.current_block_mut() {
            block.stmts.push(HirStmt::Assign {
                lhs: HirPlace::Local(temp),
                rhs: HirRvalue::Aggregate {
                    kind: crate::middleend::hir::AggregateKind::Struct(struct_id),
                    fields: field_operands,
                },
                span,
            });
        }

        Some(ExprResult::Place(HirPlace::Local(temp)))
    }

    /// 降级数组字面量。
    fn lower_array_lit(&mut self, elements: &[Expr], span: Span) -> Option<ExprResult> {
        if elements.is_empty() {
            self.diag.emit(crate::diag::Diagnostic::error(
                2012,
                "空数组字面量需要显式类型标注",
                crate::diag::DiagLoc::At(span),
            ));
            return None;
        }

        // 降级所有元素表达式
        let mut element_operands = Vec::new();
        for elem in elements {
            let operand = self.lower_expr_to_operand(elem)?;
            element_operands.push(operand);
        }

        // 推断元素类型（使用第一个元素的类型）
        let elem_ty = self.infer_operand_type(&element_operands[0], span);
        let len = elements.len();

        // 创建临时变量存储数组结果
        let temp = self.fresh_local();
        let ty = HirType::Array {
            elem: Box::new(elem_ty.clone()),
            len,
        };

        self.locals.push(crate::middleend::hir::HirLocal {
            name: None,
            ty,
            span,
        });

        // 添加赋值语句：temp = [elem1, elem2, ...]
        if let Some(block) = self.current_block_mut() {
            block.stmts.push(HirStmt::Assign {
                lhs: HirPlace::Local(temp),
                rhs: HirRvalue::Aggregate {
                    kind: crate::middleend::hir::AggregateKind::Array(elem_ty, len),
                    fields: element_operands,
                },
                span,
            });
        }

        Some(ExprResult::Place(HirPlace::Local(temp)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{DiagSink, FileId, Span};
    use crate::frontend::ast::node::Expr;
    use crate::frontend::resolve::SymbolTable;
    use crate::frontend::typecheck::TypeContext;

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
            locals: Vec::new(),
            blocks: Vec::new(),
            current_block: None,
            local_map: std::collections::HashMap::new(),
            next_local: 0,
            next_block: 0,
            loop_stack: Vec::new(),
            label_map: std::collections::HashMap::new(),
            defer_scopes: Vec::new(),
            scope_stack: Vec::new(),
            moved_locals: std::collections::HashSet::new(),
            current_function_return_type: None,
        }
    }

    #[test]
    fn test_lower_int_literal() {
        let mut ctx = make_ctx();
        let span = Span::new(FileId(0), 0, 1);
        let expr = Expr::Int("42".to_string(), span);

        let result = ctx.lower_expr(&expr).unwrap();
        match result {
            ExprResult::Operand(HirOperand::Const(Const::Int(42))) => {}
            _ => panic!("Expected Int constant 42"),
        }
    }

    #[test]
    fn test_lower_bool_literal() {
        let mut ctx = make_ctx();
        let span = Span::new(FileId(0), 0, 1);
        let expr = Expr::Bool(true, span);

        let result = ctx.lower_expr(&expr).unwrap();
        match result {
            ExprResult::Operand(HirOperand::Const(Const::Bool(true))) => {}
            _ => panic!("Expected Bool constant true"),
        }
    }

    #[test]
    fn test_lower_binary_op() {
        let mut ctx = make_ctx();
        let span = Span::new(FileId(0), 0, 1);
        ctx.start_block(span);

        let lhs = Expr::Int("1".to_string(), span);
        let rhs = Expr::Int("2".to_string(), span);
        let expr = Expr::Binary {
            op: "+",
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        };

        let result = ctx.lower_expr(&expr).unwrap();

        // 应该生成一个临时变量
        assert_eq!(ctx.locals.len(), 1);
        assert_eq!(ctx.blocks[0].stmts.len(), 1);

        match result {
            ExprResult::Operand(HirOperand::Place(_)) => {}
            _ => panic!("Expected Place operand for binary op result"),
        }
    }

    #[test]
    fn test_convert_binop() {
        let mut ctx = make_ctx();
        let span = Span::new(FileId(0), 0, 1);

        assert!(matches!(ctx.convert_binop("+", span), Some(BinOp::Add)));
        assert!(matches!(ctx.convert_binop("-", span), Some(BinOp::Sub)));
        assert!(matches!(ctx.convert_binop("*", span), Some(BinOp::Mul)));
        assert!(matches!(ctx.convert_binop("/", span), Some(BinOp::Div)));
        assert!(matches!(ctx.convert_binop("==", span), Some(BinOp::Eq)));
        assert!(matches!(ctx.convert_binop("!=", span), Some(BinOp::Ne)));
        assert!(matches!(ctx.convert_binop("<", span), Some(BinOp::Lt)));
    }

    #[test]
    fn test_convert_unop() {
        let mut ctx = make_ctx();
        let span = Span::new(FileId(0), 0, 1);

        assert!(matches!(ctx.convert_unop("-", span), Some(UnOp::Neg)));
        assert!(matches!(ctx.convert_unop("!", span), Some(UnOp::Not)));
        assert!(matches!(ctx.convert_unop("~", span), Some(UnOp::BitNot)));
    }
}
