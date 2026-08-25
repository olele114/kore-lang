//! 控制流降级：? 分支、@ 循环、ret/stop/skip/jmp。
//!
//! 核心策略：
//! - `? expr { arms }` → Switch terminator + basic blocks per arm
//! - `@ label? { body }` → Loop with entry/body/exit blocks
//! - `ret expr` → Return terminator
//! - `stop` → Unreachable terminator (panic)
//! - `skip`/`jmp label` → Goto terminator

use crate::frontend::ast::{Arm, Expr, Pattern};
use crate::middleend::hir::*;
use crate::middleend::hir::ty::HirType;
use crate::diag::{Span, Diagnostic, ErrorCode, DiagLoc, FileId};
use super::{LoweringContext, ExprResult};

impl<'a> LoweringContext<'a> {
    /// 降级 `?` 分支表达式
    ///
    /// 结构：
    /// ```text
    /// ? discriminant {
    ///     pattern1 => expr1
    ///     pattern2 => expr2
    ///     ...
    /// }
    /// ```
    ///
    /// 降级为：
    /// 1. 计算 discriminant → temp
    /// 2. 创建 arm blocks
    /// 3. 创建 after block（分支汇合点）
    /// 4. 在当前块插入 Switch terminator
    /// 5. 降级每个 arm body，结尾跳转到 after
    /// 6. 如果有返回值，在 after block 创建 phi temp
    pub fn lower_branch(
        &mut self,
        discriminant: &Expr,
        arms: &[Arm],
        span: Span,
    ) -> Option<ExprResult> {
        // 1. 计算判别值
        let discr_op = self.lower_expr_to_operand(discriminant)?;

        // 1.5. 将判别值保存到临时变量，用于后续绑定提取
        let discr_local = self.fresh_local();
        let discr_ty = discr_op.ty(self);
        self.locals.push(HirLocal {
            name: None,
            ty: discr_ty.clone(),
            span: discriminant.span(),
        });

        if let Some(block) = self.current_block_mut() {
            block.stmts.push(HirStmt::Assign {
                lhs: HirPlace::Local(discr_local),
                rhs: HirRvalue::Use(discr_op.clone()),
                span: discriminant.span(),
            });
        }

        let discr_place = HirPlace::Local(discr_local);

        // 2. 创建 arm blocks 和 after block
        let after_block = self.fresh_block();
        let mut arm_blocks = Vec::new();

        for _ in arms {
            // 只分配 BlockId，实际块在降级 arm body 时创建
            let block_id = self.fresh_block();
            arm_blocks.push(block_id);
        }

        // unreachable_block 将在需要时创建

        // 3. 提取联合类型的判别值（如果 discr_ty 是联合类型）
        let switch_operand = if matches!(
            discr_ty,
            ty::HirType::Union(_) | ty::HirType::ErrUnion { .. }
        ) {
            let discr_val_local = self.fresh_local();
            self.locals.push(HirLocal {
                name: None,
                ty: ty::HirType::i32(),
                span: Span::new(FileId(0), 0, 0),
            });

            if let Some(block) = self.current_block_mut() {
                block.stmts.push(HirStmt::Assign {
                    lhs: HirPlace::Local(discr_val_local),
                    rhs: HirRvalue::Discriminant(discr_place.clone()),
                    span: Span::new(FileId(0), 0, 0),
                });
            }
            HirOperand::Place(Box::new(HirPlace::Local(discr_val_local)))
        } else {
            // 非联合类型：直接使用原始判别值
            HirOperand::Place(Box::new(discr_place.clone()))
        };

        // 4. 先检测是否所有 arm 都终止（预扫描）
        let all_arms_terminate = arms.iter().all(|arm| {
            self.expr_always_terminates(&arm.body)
        });

        // 检测是否存在条件守卫（Pattern::Cond）
        let has_cond_guards = arms.iter().any(|arm| matches!(arm.pattern, Pattern::Cond(_)));

        // 5. 根据情况决定是否创建 unreachable_block
        let unreachable_block = if all_arms_terminate && !has_cond_guards {
            // 所有 arm 都终止且没有条件守卫：不需要 unreachable block
            arm_blocks.last().copied().unwrap_or(after_block)
        } else {
            // 有 arm 不终止或存在条件守卫：创建 unreachable block
            self.fresh_block()
        };

        // 4. 编译模式匹配为决策树
        // 关键修复：条件守卫需要 fallback 到 after_block，因为所有条件可能都为 false
        let fallback = if all_arms_terminate && !has_cond_guards {
            unreachable_block
        } else {
            after_block
        };
        let decision_tree = self.compile_patterns(arms, &arm_blocks, fallback, unreachable_block)?;

        // 5. 保存入口块 ID（在 materialize 覆盖 current_block 之前）
        let entry_point_block = self.current_block.expect("branch must have entry block");

        // 6. 展开决策树为基本块（处理嵌套的 Guard 节点）
        let decision_entry_block = decision_tree.materialize(self, switch_operand, span);

        // 7. 设置入口块跳转到决策树入口（使用保存的 ID）
        if let Some(block) = self.blocks.iter_mut().find(|b| b.id == entry_point_block) {
            block.terminator = HirTerminator::Goto(decision_entry_block);
        }

        // 6. 预先降级第一个 arm 来检查是否有返回值
        // 注意：即使有条件守卫，也需要降级 body 来确定返回类型
        let first_arm_result = if !arms.is_empty() {
            // 临时切换到第一个 arm block 并降级
            let first_block_id = arm_blocks[0];
            self.current_block = Some(first_block_id);
            self.blocks.push(HirBlock {
                id: first_block_id,
                stmts: Vec::new(),
                terminator: HirTerminator::Unreachable,
                span: arms[0].span,
            });

            // 提取第一个 arm 的绑定变量
            self.extract_pattern_bindings(&arms[0].pattern, &discr_place, arms[0].span);

            self.lower_expr(&arms[0].body)
        } else {
            None
        };

        // 6. 根据第一个 arm 的返回值决定是否需要 phi temp
        let result_local = if let Some(result) = first_arm_result.clone() {
            let result_op = result.to_operand(self);
            if let Some(op) = result_op {
                let hir_ty = op.ty(self);

                // void 臂不需要 phi temp：后端不为 void 局部分配栈空间
                // （backend/llvm/function.rs 跳过 is_void 的 local），若仍生成
                // `Assign { lhs: Local(n), rhs: Use(Const(Void)) }`，codegen 会
                // 报 "Symbol not found: local"。此时仅需修好 arm 的 terminator。
                let temp_id = if hir_ty.is_void() {
                    None
                } else {
                    let id = self.fresh_local();
                    self.locals.push(HirLocal {
                        name: None,
                        ty: hir_ty,
                        span,
                    });
                    Some(id)
                };

                // 检查第一个 arm 是否已经有非 Unreachable 的 terminator
                let first_arm_terminator = self.blocks.iter()
                    .find(|b| b.id == arm_blocks[0])
                    .map(|b| b.terminator.clone())
                    .unwrap_or(HirTerminator::Unreachable);

                match first_arm_terminator {
                    HirTerminator::Unreachable => {
                        // 第一个 arm 没有 terminator，直接插入赋值和跳转
                        if let Some(block) = self.blocks.iter_mut().find(|b| b.id == arm_blocks[0]) {
                            if let Some(temp_id) = temp_id {
                                block.stmts.push(HirStmt::Assign {
                                    lhs: HirPlace::Local(temp_id),
                                    rhs: HirRvalue::Use(op),
                                    span: arms[0].span,
                                });
                            }
                            block.terminator = HirTerminator::Goto(after_block);
                        }
                    }
                    HirTerminator::Goto(_inner_block) => {
                        // 第一个 arm 跳转到另一个块（如嵌套 match 的入口）
                        // 需要在 current_block（嵌套表达式的 after_block）中插入赋值
                        if let Some(current) = self.current_block {
                            // current_block 是嵌套表达式的 after_block
                            if let Some(block) = self.blocks.iter_mut().find(|b| b.id == current) {
                                if let Some(temp_id) = temp_id {
                                    block.stmts.push(HirStmt::Assign {
                                        lhs: HirPlace::Local(temp_id),
                                        rhs: HirRvalue::Use(op),
                                        span: arms[0].span,
                                    });
                                }
                                // 将嵌套表达式的 after_block 跳转到外层 after_block
                                block.terminator = HirTerminator::Goto(after_block);
                            }
                        }
                    }
                    _ => {
                        // 其他情况（Return, Switch 等），arm 已经终止
                        // 不需要插入赋值
                    }
                }

                temp_id
            } else {
                None
            }
        } else {
            None
        };

        // 7. 降级所有 arms，跳过第一个 arm（已在预降级阶段处理）
        let start_idx = 1;
        for (i, arm) in arms.iter().enumerate().skip(start_idx) {
            let arm_block_id = arm_blocks[i];
            self.current_block = Some(arm_block_id);

            // 只有当块不存在时才创建（避免重复创建第一个 arm 的块）
            if !self.blocks.iter().any(|b| b.id == arm_block_id) {
                self.blocks.push(HirBlock {
                    id: arm_block_id,
                    stmts: Vec::new(),
                    terminator: HirTerminator::Unreachable,
                    span: arm.span,
                });
            }

            // 提取绑定变量
            self.extract_pattern_bindings(&arm.pattern, &discr_place, arm.span);

            // 降级 arm body
            if let Some(result) = self.lower_expr(&arm.body) {
                if let Some(result_op) = result.to_operand(self) {
                    // 如果有 phi temp，写入结果。
                    // void 臂没有值可写：语句位置的分支允许各臂类型不统一
                    // （见 typecheck/checker.rs 的 branch_in_stmt_pos），此时
                    // 产值臂在前会分配非 void 的 phi temp，而后续 void 臂的
                    // 结果不能写入其中，否则 codegen 报 "void has no value"。
                    // phi temp 的值在语句位置本就无人读取，跳过赋值即可。
                    if let Some(result_local) = result_local
                        && !result_op.ty(self).is_void()
                    {
                        if let Some(block) = self.current_block_mut() {
                            block.stmts.push(HirStmt::Assign {
                                lhs: HirPlace::Local(result_local),
                                rhs: HirRvalue::Use(result_op),
                                span: arm.span,
                            });
                        }
                    }
                }
            }
        }

        // 8. 检查是否所有 arm 都已终止（不是 Unreachable）
        let all_arms_terminated = arm_blocks.iter().all(|&block_id| {
            self.blocks.iter()
                .find(|b| b.id == block_id)
                .map(|b| !matches!(b.terminator, HirTerminator::Unreachable))
                .unwrap_or(false)
        });

        // 检查模式是否完全覆盖（是否有 wildcard 或 bind 模式）
        let _has_wildcard = arms.iter().any(|arm| {
            matches!(arm.pattern, Pattern::Wildcard(_) | Pattern::Bind(_, _))
        });

        // 如果所有 arm 都已终止，后续代码不可达（无论是否有通配符）
        // 这是因为即使模式不完全，未匹配的情况也会由其他机制处理（如运行时 panic）
        if all_arms_terminated {
            // 所有 arm 都已终止，不需要 after_block
            // 注意：unreachable_block 在前面已经被设置为复用现有块，因此这里不需要创建
            // 如果 unreachable_block 是新分配的（还未被创建），才创建它
            let unreachable_exists = self.blocks.iter().any(|b| b.id == unreachable_block);
            if !unreachable_exists {
                self.blocks.push(HirBlock {
                    id: unreachable_block,
                    stmts: Vec::new(),
                    terminator: HirTerminator::Unreachable,
                    span,
                });
            }

            // 将 current_block 设为 None，仅当没有条件守卫时
            // 条件守卫需要保持 after_block 作为 fallback
            if !has_cond_guards {
                self.current_block = None;
            } else {
                // 创建 after_block 用于条件守卫的 fallback
                self.blocks.push(HirBlock {
                    id: after_block,
                    stmts: Vec::new(),
                    terminator: HirTerminator::Unreachable,
                    span,
                });
                self.current_block = Some(after_block);
            }
        } else {
            // 至少有一个 arm 需要继续执行（terminator 仍为 Unreachable）
            // 为这些 arm 添加跳转到 after_block

            if let Some(block) = self.blocks.iter_mut().find(|b| b.id == arm_blocks[0]) {
                if matches!(block.terminator, HirTerminator::Unreachable) {
                    block.terminator = HirTerminator::Goto(after_block);
                }
            }

            for &arm_block_id in arm_blocks.iter().skip(1) {
                if let Some(block) = self.blocks.iter_mut().find(|b| b.id == arm_block_id) {
                    if matches!(block.terminator, HirTerminator::Unreachable) {
                        block.terminator = HirTerminator::Goto(after_block);
                    }
                }
            }

            // 创建 after_block
            // 注意：如果所有 arms 都 terminate（例如都包含 ret），
            // after_block 永远不会被执行，但我们仍然创建它以保持 HIR 结构完整
            self.current_block = Some(after_block);
            self.blocks.push(HirBlock {
                id: after_block,
                stmts: Vec::new(),
                terminator: HirTerminator::Unreachable,
                span,
            });

            // 创建 unreachable_block（用于非穷尽匹配的 otherwise）
            self.blocks.push(HirBlock {
                id: unreachable_block,
                stmts: Vec::new(),
                terminator: HirTerminator::Unreachable,
                span,
            });
        }

        // 9. 返回 phi temp（如果有）
        result_local.map(|local| ExprResult::Place(HirPlace::Local(local)))
    }

    /// 从模式中提取绑定变量并在当前块中生成提取语句
    ///
    /// 对于 `.Some(v)` 模式，创建局部变量 `v` 并从判别值中提取字段值
    fn extract_pattern_bindings(
        &mut self,
        pattern: &Pattern,
        discr_place: &HirPlace,
        span: Span,
    ) {
        match pattern {
            Pattern::Variant { name: variant_name, bindings, .. } => {
                // 获取变体索引
                let variant_idx = self.find_variant_index(variant_name).unwrap_or(0);

                // 查找联合体名称（通过遍历所有联合体定义）
                let union_name = self.union_defs.iter()
                    .find(|(_, def)| def.variants.iter().any(|v| &v.name == variant_name))
                    .map(|(name, _)| name.clone());

                // 错误联合（`T ! E`）不在 union_defs 中登记，payload 类型直接
                // 来自判别值的 HirType::ErrUnion：变体 0 = .Ok(T)，1 = .Err(E)。
                // 缺了这一步会退化成 i32，把 str 的胖指针按整数读，输出乱码。
                let errunion_payload = match discr_place {
                    HirPlace::Local(lid) => match self.locals.get(lid.0).map(|l| l.ty.clone()) {
                        Some(HirType::ErrUnion { ok, err }) => Some(if variant_idx == 0 {
                            (*ok).clone()
                        } else {
                            (*err).clone()
                        }),
                        _ => None,
                    },
                    _ => None,
                };

                // 对每个绑定变量创建 HirLocal 并生成提取语句
                for (_field_idx, binding_name) in bindings.iter().enumerate() {
                    // 创建局部变量
                    let local_id = self.fresh_local();

                    // 推断绑定变量的类型（先错误联合，再具名联合定义）
                    let binding_ty = if let Some(ref ty) = errunion_payload {
                        ty.clone()
                    } else if let Some(ref u_name) = union_name {
                        self.get_variant_payload_type(u_name, variant_idx)
                            .unwrap_or_else(|| HirType::i32())
                    } else {
                        HirType::i32()
                    };

                    self.locals.push(HirLocal {
                        name: Some(binding_name.clone()),
                        ty: binding_ty,
                        span,
                    });

                    // 注册到作用域
                    self.local_map.insert(binding_name.clone(), local_id);

                    // 生成 payload 提取语句：binding = extract_payload(discr_place, variant_idx)
                    if let Some(block) = self.current_block_mut() {
                        block.stmts.push(HirStmt::Assign {
                            lhs: HirPlace::Local(local_id),
                            rhs: HirRvalue::ExtractPayload {
                                place: discr_place.clone(),
                                variant_index: variant_idx,
                            },
                            span,
                        });
                    }
                }
            }
            Pattern::Bind(name, _) => {
                // 通配符 "_" 不绑定，直接跳过
                if name == "_" {
                    return;
                }

                // 简单绑定：直接将判别值赋给绑定变量
                let local_id = self.fresh_local();

                // 从判别值的类型推断绑定变量类型
                let binding_ty = match discr_place {
                    HirPlace::Local(lid) => {
                        self.locals.get(lid.0)
                            .map(|local| local.ty.clone())
                            .unwrap_or_else(|| HirType::i32())
                    }
                    _ => HirType::i32(),
                };

                self.locals.push(HirLocal {
                    name: Some(name.clone()),
                    ty: binding_ty,
                    span,
                });

                self.local_map.insert(name.clone(), local_id);

                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Assign {
                        lhs: HirPlace::Local(local_id),
                        rhs: HirRvalue::Use(HirOperand::Place(Box::new(discr_place.clone()))),
                        span,
                    });
                }
            }
            Pattern::Wildcard(_) | Pattern::Lit(_) | Pattern::Cond(_) => {
                // 这些模式不需要绑定提取
            }
        }
    }

    /// 降级 `@` 循环表达式
    ///
    /// 结构：
    /// ```text
    /// @ label? { body }
    /// ```
    ///
    /// 降级为：
    /// - header block（循环入口）
    /// - body block（循环体）
    /// - exit block（循环出口）
    ///
    /// `skip` → Goto(header), `stop` → Goto(exit)
    pub fn lower_loop(
        &mut self,
        label: Option<&str>,
        cond: Option<&Expr>,
        body: &Expr,
        span: Span,
    ) -> Option<ExprResult> {
        // 创建 loop blocks
        let header = self.fresh_block();
        let body_block = self.fresh_block();
        let exit_block = self.fresh_block();

        // 当前块跳转到 header
        let current = self.current_block?;
        if let Some(block) = self.blocks.iter_mut().find(|b| b.id == current) {
            block.terminator = HirTerminator::Goto(header);
        }

        // Header block：每次迭代都在此重新求值循环条件
        self.current_block = Some(header);
        self.blocks.push(HirBlock {
            id: header,
            stmts: Vec::new(),
            terminator: HirTerminator::Unreachable,
            span,
        });

        // 条件求值可能自身产生新块，终结符要落在条件求值的尾块上
        let (cond_tail, cond_op) = match cond {
            Some(expr) => {
                let op = self.lower_expr_to_operand(expr)?;
                (self.current_block?, Some(op))
            }
            None => (header, None),
        };

        if let Some(block) = self.blocks.iter_mut().find(|b| b.id == cond_tail) {
            block.terminator = match cond_op {
                // `@ cond { .. }`：真值进入 body，假值跳出循环
                Some(discr) => HirTerminator::Switch {
                    discr,
                    targets: vec![(1, body_block)],
                    otherwise: exit_block,
                },
                // `@ { .. }`：无条件无限循环
                None => HirTerminator::Goto(body_block),
            };
        }

        // Body block
        self.current_block = Some(body_block);
        self.blocks.push(HirBlock {
            id: body_block,
            stmts: Vec::new(),
            terminator: HirTerminator::Unreachable,
            span,
        });

        // 进入循环上下文，记录当前作用域深度
        let scope_depth = self.scope_stack.len();
        self.loop_stack.push((header, exit_block, scope_depth));

        // 注册标签到 label_map（如果有）
        if let Some(label_str) = label {
            self.label_map.insert(label_str.to_string(), (header, exit_block, scope_depth));
        }

        // 降级 body（body 为 void 时返回 None，不能用 `?` 提前退出）
        let _ = self.lower_expr(body);

        // 退出循环上下文
        self.loop_stack.pop();

        // 从 label_map 中移除标签
        if let Some(label_str) = label {
            self.label_map.remove(label_str);
        }

        // Body 尾块默认跳回 header（除非有显式 stop/ret）
        if let Some(block) = self.current_block_mut() {
            if matches!(block.terminator, HirTerminator::Unreachable) {
                block.terminator = HirTerminator::Goto(header);
            }
        }

        // Exit block
        self.current_block = Some(exit_block);
        self.blocks.push(HirBlock {
            id: exit_block,
            stmts: Vec::new(),
            terminator: HirTerminator::Unreachable,
            span,
        });

        None
    }

    /// 降级 `ret` 语句
    pub fn lower_ret(&mut self, value: Option<&Expr>, span: Span) -> Option<ExprResult> {
        // 如果 current_block 为 None，表示当前代码不可达（例如在 branch 所有分支都已终止后）
        // 此时不需要生成任何代码，直接返回
        if self.current_block.is_none() {
            crate::trace!("DEBUG lower_ret: current_block is None, returning early");
            return None;
        }

        let initial_block = self.current_block;
        crate::trace!("DEBUG lower_ret: initial current_block = {:?}", initial_block);

        let ret_op = if let Some(expr) = value {
            Some(self.lower_expr_to_operand(expr)?)
        } else {
            None
        };

        crate::trace!("DEBUG lower_ret: after lower_expr_to_operand, current_block = {:?}", self.current_block);

        // 在 return 前退出当前作用域，插入 defer 和 Drop 语句
        let (owned_locals, defers) = self.exit_scope();

        // 先插入 Drop（先析构对象）
        for local_id in owned_locals.iter().rev() {
            // 跳过已移动的变量（避免 double drop）
            if !self.is_moved(*local_id) {
                if let Some(block) = self.current_block_mut() {
                    block.stmts.push(HirStmt::Drop {
                        place: HirPlace::Local(*local_id),
                        span,
                    });
                }
            }
        }

        // 后插入 defer（后清理，此时对象已析构）
        for (defer_expr, _span) in defers.iter().rev() {
            let _ = self.lower_expr(defer_expr);
        }

        // 恢复作用域（因为函数体作用域需要在函数结束时正式退出）
        self.scope_stack.push(owned_locals);
        self.defer_scopes.push(defers);

        // 重新获取 current_block，因为 lower_expr_to_operand 可能改变了它
        // 例如，分支表达式会创建 after_block 并将其设置为 current_block
        let block_id = self.current_block?;

        crate::trace!("DEBUG lower_ret: setting Return terminator on block {:?}", block_id);

        if let Some(block) = self.blocks.iter_mut().find(|b| b.id == block_id) {
            crate::trace!("DEBUG lower_ret: found block {:?}, setting terminator", block.id);
            block.terminator = HirTerminator::Return(ret_op);
        } else {
            crate::trace!("DEBUG lower_ret: ERROR - block {:?} not found!", block_id);
        }

        // 关键修复：在设置 Return terminator 后，将 current_block 设为 None
        // 这样后续代码就不会继续向这个已终止的 block 添加内容
        self.current_block = None;
        crate::trace!("DEBUG lower_ret: set current_block to None");

        None
    }

    /// 降级 `stop` 语句（break）
    pub fn lower_stop(&mut self, label: Option<&str>, span: Span) -> Option<ExprResult> {
        let (exit, target_scope_depth) = if let Some(label_str) = label {
            // 带标签的 stop：从 label_map 查找目标循环的 exit 和作用域深度
            if let Some((_, exit, depth)) = self.label_map.get(label_str) {
                (*exit, *depth)
            } else {
                self.diag.emit(Diagnostic::error(
                    ErrorCode::Unimplemented.as_u16(),
                    &format!("label '{}' not found in current scope", label_str),
                    DiagLoc::At(span),
                ));
                return None;
            }
        } else {
            // 无标签：获取最内层循环的 exit block 和作用域深度
            if let Some((_, exit, depth)) = self.loop_stack.last() {
                (*exit, *depth)
            } else {
                self.diag.emit(Diagnostic::error(
                    ErrorCode::Unimplemented.as_u16(),
                    "stop outside of loop",
                    DiagLoc::At(span),
                ));
                return None;
            }
        };

        // 展开从当前作用域到目标循环作用域之间的所有 defer 和 drop
        self.unwind_scopes_to_depth(target_scope_depth, span);

        // 设置跳转到 exit block
        if let Some(block) = self.current_block_mut() {
            block.terminator = HirTerminator::Goto(exit);
        }
        self.current_block = None;

        None
    }

    /// 降级 `skip` 语句（continue）
    pub fn lower_skip(&mut self, label: Option<&str>, span: Span) -> Option<ExprResult> {
        let (header, target_scope_depth) = if let Some(label_str) = label {
            // 带标签的 skip：从 label_map 查找目标循环的 header 和作用域深度
            if let Some((h, _, depth)) = self.label_map.get(label_str) {
                (*h, *depth)
            } else {
                self.diag.emit(Diagnostic::error(
                    ErrorCode::Unimplemented.as_u16(),
                    &format!("label '{}' not found in current scope", label_str),
                    DiagLoc::At(span),
                ));
                return None;
            }
        } else {
            // 无标签：获取最内层循环的 header block 和作用域深度
            if let Some((h, _, depth)) = self.loop_stack.last() {
                (*h, *depth)
            } else {
                self.diag.emit(Diagnostic::error(
                    ErrorCode::Unimplemented.as_u16(),
                    "skip outside of loop",
                    DiagLoc::At(span),
                ));
                return None;
            }
        };

        // 展开从当前作用域到目标循环作用域之间的所有 defer 和 drop
        self.unwind_scopes_to_depth(target_scope_depth, span);

        // 设置跳转到 header block
        if let Some(block) = self.current_block_mut() {
            block.terminator = HirTerminator::Goto(header);
        }
        self.current_block = None;

        None
    }

    /// 降级 `jmp` 语句（break）
    pub fn lower_jmp(&mut self, label: Option<&str>, span: Span) -> Option<ExprResult> {
        let (exit, target_scope_depth) = if let Some(label_str) = label {
            // 带标签的 jmp：从 label_map 查找目标循环的 exit 和作用域深度
            if let Some((_header, e, depth)) = self.label_map.get(label_str) {
                (*e, *depth)
            } else {
                self.diag.emit(Diagnostic::error(
                    ErrorCode::Unimplemented.as_u16(),
                    &format!("label '{}' not found in current scope", label_str),
                    DiagLoc::At(span),
                ));
                return None;
            }
        } else {
            // 无标签：获取最内层循环的 exit block 和作用域深度
            if let Some((_, e, depth)) = self.loop_stack.last() {
                (*e, *depth)
            } else {
                self.diag.emit(Diagnostic::error(
                    ErrorCode::Unimplemented.as_u16(),
                    "jmp outside of loop",
                    DiagLoc::At(span),
                ));
                return None;
            }
        };

        // 展开从当前作用域到目标循环作用域之间的所有 defer 和 drop
        self.unwind_scopes_to_depth(target_scope_depth, span);

        if let Some(block) = self.current_block_mut() {
            block.terminator = HirTerminator::Goto(exit);
        }
        self.current_block = None;

        None
    }

    /// 降级 `defer` 语句
    ///
    /// 策略：收集 defer 语句到函数级列表，在 Return terminator 前插入
    pub fn lower_defer(&mut self, expr: &Expr, span: Span) -> Option<ExprResult> {
        // 将 defer 表达式存储到当前作用域（在 func.rs 的 lower_stmt 中已处理）
        // 这里不需要额外操作，因为 Stmt::Defer 分支已经处理了
        let _ = (expr, span);
        None
    }

    /// 展开从当前作用域到目标深度之间的所有作用域，插入 drop 和 defer 语句。
    ///
    /// 用于 break/continue 时正确清理中间作用域。
    ///
    /// # 参数
    /// - `target_depth`: 目标作用域深度（不展开该深度的作用域）
    /// - `span`: 用于生成 drop 语句的源位置
    fn unwind_scopes_to_depth(&mut self, target_depth: usize, span: Span) {
        let current_depth = self.scope_stack.len();

        if current_depth <= target_depth {
            // 已经在目标深度或更浅，无需展开
            return;
        }

        // 临时保存需要展开的作用域
        let mut temp_owned = Vec::new();
        let mut temp_defers = Vec::new();

        // 从内到外展开作用域
        for _ in target_depth..current_depth {
            let (owned, defers) = self.exit_scope();
            temp_owned.push(owned);
            temp_defers.push(defers);
        }

        // 插入 drops 和 defers（从外到内，即逆序）
        for (owned, defers) in temp_owned.iter().zip(temp_defers.iter()).rev() {
            // 先插入 Drop（先析构对象）
            for local_id in owned.iter().rev() {
                if !self.is_moved(*local_id) {
                    if let Some(block) = self.current_block_mut() {
                        block.stmts.push(HirStmt::Drop {
                            place: HirPlace::Local(*local_id),
                            span,
                        });
                    }
                }
            }

            // 后插入 defer（后清理，此时对象已析构）
            for (defer_expr, _span) in defers.iter().rev() {
                let _ = self.lower_expr(defer_expr);
            }
        }

        // 恢复作用域（从外到内，即逆序）
        for (owned, defers) in temp_owned.into_iter().zip(temp_defers.into_iter()).rev() {
            self.scope_stack.push(owned);
            self.defer_scopes.push(defers);
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────────
// 单元测试
// ────────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::DiagSink;
    use crate::frontend::resolve::SymbolTable;
    use crate::frontend::typecheck::TypeContext;

    fn make_ctx() -> LoweringContext<'static> {
        let diag = Box::leak(Box::new(DiagSink::new()));
        let symbols = Box::leak(Box::new(SymbolTable::new()));
        let type_ctx = Box::leak(Box::new(TypeContext::new()));

        LoweringContext::new(diag, symbols, type_ctx)
    }

    #[test]
    fn test_lower_ret_void() {
        let mut ctx = make_ctx();
        let span = Span::new(crate::diag::FileId(0), 0, 1);
        ctx.start_block(span);

        ctx.lower_ret(None, span);

        assert_eq!(ctx.blocks.len(), 1);
        assert!(matches!(
            ctx.blocks[0].terminator,
            HirTerminator::Return(None)
        ));
    }

    #[test]
    fn test_lower_ret_with_value() {
        let mut ctx = make_ctx();
        let span = Span::new(crate::diag::FileId(0), 0, 1);
        ctx.start_block(span);

        let expr = Expr::Int("42".to_string(), span);
        ctx.lower_ret(Some(&expr), span);

        assert_eq!(ctx.blocks.len(), 1);
        assert!(matches!(
            ctx.blocks[0].terminator,
            HirTerminator::Return(Some(_))
        ));
    }

    #[test]
    fn test_lower_stop() {
        let mut ctx = make_ctx();
        let span = Span::new(crate::diag::FileId(0), 0, 1);
        ctx.start_block(span);

        ctx.lower_stop(None, span);

        assert_eq!(ctx.blocks.len(), 1);
        assert!(matches!(
            ctx.blocks[0].terminator,
            HirTerminator::Unreachable { .. }
        ));
    }

    #[test]
    fn test_branch_with_all_arms_returning() {
        let mut ctx = make_ctx();
        let span = Span::new(crate::diag::FileId(0), 0, 1);

        // 创建一个分支表达式，所有分支都显式 ret
        use crate::frontend::ast::{Arm, Expr, Pattern};

        let scrutinee = Expr::Bool(true, span);
        let arms = vec![
            Arm {
                pattern: Pattern::Lit(Box::new(Expr::Bool(true, span))),
                body: Expr::Ret(Some(Box::new(Expr::Int("1".to_string(), span))), span),
                span,
            },
            Arm {
                pattern: Pattern::Wildcard(span),
                body: Expr::Ret(Some(Box::new(Expr::Int("2".to_string(), span))), span),
                span,
            },
        ];

        ctx.start_block(span);

        let result = ctx.lower_branch(&scrutinee, &arms, span);

        // 验证：
        // 1. 应该返回 None（所有分支都终止，表达式不可达）
        assert!(result.is_none(), "Branch with all arms returning should produce None");

        // 2. current_block 应该为 None（后续代码不可达）
        assert!(ctx.current_block.is_none(), "current_block should be None after branch with all arms terminating");

        // 3. 应该至少有一些 blocks 包含 Return 终止符
        let return_blocks = ctx.blocks.iter()
            .filter(|b| matches!(b.terminator, HirTerminator::Return(_)))
            .count();
        assert!(return_blocks >= 2, "Should have at least 2 blocks with Return terminators");

        // 4. 不应该有 unreachable after_block（可以通过检查没有空终止符来验证）
        let unreachable_blocks = ctx.blocks.iter()
            .filter(|b| matches!(b.terminator, HirTerminator::Unreachable { .. }))
            .count();
        assert_eq!(unreachable_blocks, 0, "Should not have unreachable blocks when all arms terminate");
    }
}
