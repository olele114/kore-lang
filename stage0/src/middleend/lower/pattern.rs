//! 模式匹配编译为决策树
//!
//! 将 `?` 分支表达式的模式匹配编译为高效的决策树，
//! 生成基本块跳转逻辑。

use crate::diag::Span;
use crate::frontend::ast::{Arm, Expr, Pattern};
use crate::middleend::hir::{
    BinOp, BlockId, Const, HirOperand, HirPlace, HirRvalue, HirStmt, HirTerminator,
};
use super::LoweringContext;

/// 决策树节点
#[derive(Debug)]
pub enum DecisionNode {
    /// 叶节点：跳转到指定 arm 块
    Leaf(BlockId),

    /// Switch 节点：根据判别式值分支
    Switch {
        discriminant: HirOperand,
        targets: Vec<(u64, Box<DecisionNode>)>,
        otherwise: Box<DecisionNode>,
    },

    /// Guard 节点：条件表达式守卫
    Guard {
        condition: HirOperand,
        then_branch: Box<DecisionNode>,
        else_branch: Box<DecisionNode>,
    },
}

impl DecisionNode {
    /// 展平决策树为线性的块序列
    ///
    /// 递归展开嵌套的决策节点，为每个非 Leaf 节点创建辅助块。
    /// 返回决策树的入口 BlockId。
    pub fn materialize(self, ctx: &mut LoweringContext, discr: HirOperand, span: Span) -> BlockId {
        match self {
            DecisionNode::Leaf(block) => block,
            DecisionNode::Switch { discriminant: _, targets, otherwise } => {
                // 创建辅助块来存放 Switch 终结符
                let switch_block = ctx.start_block(span);

                // 递归展开所有目标分支
                let hir_targets: Vec<(u64, BlockId)> = targets
                    .into_iter()
                    .map(|(val, node)| {
                        let target_block = node.materialize(ctx, discr.clone(), span);
                        (val, target_block)
                    })
                    .collect();

                let otherwise_block = otherwise.materialize(ctx, discr.clone(), span);

                // 设置 Switch 终结符（直接修改块的 terminator）
                if let Some(idx) = ctx.blocks.iter().position(|b| b.id == switch_block) {
                    ctx.blocks[idx].terminator = HirTerminator::Switch {
                        discr,
                        targets: hir_targets,
                        otherwise: otherwise_block,
                    };
                }

                switch_block
            }
            DecisionNode::Guard { condition, then_branch, else_branch } => {
                // 创建辅助块来存放条件分支
                let guard_block = ctx.start_block(span);

                // 递归展开 then 和 else 分支
                let then_block = then_branch.materialize(ctx, discr.clone(), span);
                let else_block = else_branch.materialize(ctx, discr, span);

                // 使用 Switch 模拟条件分支（直接修改块的 terminator）
                if let Some(idx) = ctx.blocks.iter().position(|b| b.id == guard_block) {
                    ctx.blocks[idx].terminator = HirTerminator::Switch {
                        discr: condition,
                        targets: vec![(1, then_block)],
                        otherwise: else_block,
                    };
                }

                guard_block
            }
        }
    }
}

impl LoweringContext<'_> {
    /// 编译模式匹配为决策树
    pub fn compile_patterns(
        &mut self,
        arms: &[Arm],
        arm_blocks: &[BlockId],
        fallback: BlockId,
        unreachable: BlockId,
    ) -> Option<DecisionNode> {
        crate::trace!("DEBUG compile_patterns: arms.len()={}, arm_blocks={:?}, fallback=BlockId({}), unreachable=BlockId({})",
            arms.len(), arm_blocks, fallback.0, unreachable.0);

        if arms.is_empty() {
            return Some(DecisionNode::Leaf(fallback));
        }

        // 简化实现：逐个模式线性匹配
        let mut targets = Vec::new();
        let mut wildcard_block = None;

        for (i, arm) in arms.iter().enumerate() {
            let block = arm_blocks[i];
            match &arm.pattern {
                Pattern::Lit(expr) => {
                    // 字面量模式：提取常量值
                    if let Some(val) = self.extract_const_u64(expr) {
                        targets.push((val, Box::new(DecisionNode::Leaf(block))));
                    }
                }
                Pattern::Variant { name, bindings, .. } => {
                    // 变体模式：需要查询联合体类型和变体索引
                    // 1. 从变体名推断联合体类型（简化：假设变体名唯一）
                    if let Some(variant_index) = self.find_variant_index(name) {
                        targets.push((variant_index as u64, Box::new(DecisionNode::Leaf(block))));
                    }

                    // 绑定变量提取：
                    // 完整实现需要在目标块开头插入语句，从判别体中提取字段值
                    // 例如：let binding_var = discriminant.field_N
                    // 当前简化：忽略绑定变量，假设模式匹配仅用于控制流
                    let _ = bindings;
                }
                Pattern::Wildcard(_) | Pattern::Bind(_, _) => {
                    // 通配符或绑定：匹配所有值
                    wildcard_block = Some(block);
                    break;
                }
                Pattern::Cond(expr) => {
                    // 条件守卫模式：生成 Guard 节点
                    crate::trace!("DEBUG compile_patterns: 处理条件守卫 arm[{}], block=BlockId({})", i, block.0);
                    // 1. 降级守卫表达式为 HirOperand
                    if let Some(condition) = self.lower_expr_to_operand(expr) {
                        // 2. then_branch 指向当前 arm 的块
                        let then_branch = Box::new(DecisionNode::Leaf(block));
                        crate::trace!("DEBUG compile_patterns: then_branch = BlockId({})", block.0);

                        // 3. else_branch：如果还有后续 arm，递归处理；否则跳转到 fallback
                        let else_branch = if i + 1 < arms.len() {
                            crate::trace!("DEBUG compile_patterns: 递归处理剩余 {} 个 arms", arms.len() - i - 1);
                            // 递归处理剩余的 arms
                            Box::new(
                                self.compile_patterns(
                                    &arms[i + 1..],
                                    &arm_blocks[i + 1..],
                                    fallback,
                                    unreachable,
                                )
                                .unwrap_or(DecisionNode::Leaf(fallback)),
                            )
                        } else {
                            crate::trace!("DEBUG compile_patterns: 无剩余 arms，else_branch = fallback BlockId({})", fallback.0);
                            Box::new(DecisionNode::Leaf(fallback))
                        };

                        crate::trace!("DEBUG compile_patterns: 返回 Guard 节点");
                        // 返回 Guard 节点（对于条件守卫，直接返回而不继续循环）
                        return Some(DecisionNode::Guard {
                            condition,
                            then_branch,
                            else_branch,
                        });
                    }
                }
            }
        }

        // 使用 unreachable block 作为 otherwise（穷尽匹配时）
        let otherwise = Box::new(DecisionNode::Leaf(
            wildcard_block.unwrap_or(unreachable)
        ));

        if targets.is_empty() {
            Some(*otherwise)
        } else {
            Some(DecisionNode::Switch {
                discriminant: HirOperand::Const(Const::Int(0)), // 占位
                targets,
                otherwise,
            })
        }
    }

    /// 从 AST 表达式提取常量 u64 值
    pub fn extract_const_u64(&self, expr: &Expr) -> Option<u64> {
        match expr {
            Expr::Int(s, _) => s.parse::<u64>().ok(),
            Expr::Bool(b, _) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    }

    /// 查找变体在联合体中的索引
    pub fn find_variant_index(&self, variant_name: &str) -> Option<usize> {
        // 特殊处理内置 ErrUnion 变体
        match variant_name {
            "Ok" => return Some(0),
            "Err" => return Some(1),
            _ => {}
        }

        // 遍历所有联合体定义，查找变体名
        for union_def in self.union_defs.values() {
            for (index, variant) in union_def.variants.iter().enumerate() {
                if variant.name == variant_name {
                    return Some(index);
                }
            }
        }
        None
    }

    /// 编译变体模式为 Discriminant 检查
    pub fn compile_variant_pattern(
        &mut self,
        place: HirPlace,
        variant_index: usize,
        then_block: BlockId,
        else_block: BlockId,
        span: Span,
    ) {
        // 1. 生成 Discriminant rvalue
        let discr_temp = self.fresh_local();
        self.push_assign(
            HirPlace::Local(discr_temp),
            HirRvalue::Discriminant(place),
            span,
        );

        // 2. 生成比较指令
        let cmp_temp = self.fresh_local();
        self.push_assign(
            HirPlace::Local(cmp_temp),
            HirRvalue::BinaryOp {
                op: BinOp::Eq,
                lhs: HirOperand::Place(Box::new(HirPlace::Local(discr_temp))),
                rhs: HirOperand::Const(Const::Int(variant_index as i128)),
            },
            span,
        );

        // 3. 插入条件跳转（需要扩展 HirTerminator）
        // 当前 Switch 仅支持整数，需要 SwitchInt(bool)
        if let Some(current_id) = self.current_block {
            if let Some(block) = self.blocks.get_mut(current_id.0) {
                block.terminator = HirTerminator::Switch {
                    discr: HirOperand::Place(Box::new(HirPlace::Local(cmp_temp))),
                    targets: vec![(1, then_block)],
                    otherwise: else_block,
                };
            }
        }
    }

    /// 辅助：添加赋值语句到当前块
    fn push_assign(&mut self, lhs: HirPlace, rhs: HirRvalue, span: Span) {
        if let Some(block) = self.current_block.and_then(|id| self.blocks.get_mut(id.0)) {
            block.stmts.push(HirStmt::Assign { lhs, rhs, span });
        }
    }
}
