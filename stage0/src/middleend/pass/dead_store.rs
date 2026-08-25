//! 无用赋值消除 Pass。
//!
//! 删除从未被读取的局部变量赋值，减少 HIR 体积。
//! 算法：活跃变量分析 + 删除无用赋值语句。

use crate::middleend::hir::{BlockId, HirBlock, HirBody, HirOperand, HirPlace, HirRvalue, HirStmt, HirTerminator, LocalId};
use std::collections::{HashMap, HashSet};

use super::Pass;

/// 无用赋值消除 Pass
pub struct DeadStoreElimination;

impl Pass for DeadStoreElimination {
    fn name(&self) -> &str {
        "dead-store-elimination"
    }

    fn run_on_body(&mut self, body: &mut HirBody) -> bool {
        // 1. 计算活跃变量集合
        let live_sets = compute_liveness(body);

        // 2. 删除无用赋值
        let mut changed = false;

        for block in &mut body.blocks {
            let info = live_sets.get(&block.id).cloned().unwrap_or_else(|| LivenessInfo {
                live_in: HashSet::new(),
                live_out: HashSet::new(),
            });

            let original_len = block.stmts.len();
            let mut new_stmts = Vec::new();
            let mut live = info.live_out.clone();

            // 首先将 terminator 中使用的变量加入活跃集
            collect_uses_from_terminator(&block.terminator, &mut live);

            // 反向遍历语句，计算每条语句后的活跃集
            for stmt in block.stmts.iter().rev() {
                let mut keep = true;

                match stmt {
                    HirStmt::Assign { lhs, rhs, .. } => {
                        if let HirPlace::Local(local_id) = lhs {
                            // 如果赋值的变量在之后不活跃，删除该赋值
                            if !live.contains(local_id) {
                                keep = false;
                            }

                            // 无论是否保留，都需要更新活跃集
                            // 移除被定义的变量
                            live.remove(local_id);
                            // 添加右侧使用的变量
                            collect_uses_from_rvalue(rhs, &mut live);
                        }
                    }
                    HirStmt::Call { .. } | HirStmt::Drop { .. } => {
                        // 函数调用和 drop 必须保留（有副作用）
                        keep = true;
                    }
                }

                if keep {
                    new_stmts.push(stmt.clone());
                }
            }

            new_stmts.reverse();
            block.stmts = new_stmts;

            if block.stmts.len() < original_len {
                changed = true;
            }
        }

        changed
    }
}

#[derive(Debug, Clone)]
struct LivenessInfo {
    live_in: HashSet<LocalId>,
    live_out: HashSet<LocalId>,
}

/// 计算每个基本块的活跃变量集合
fn compute_liveness(body: &HirBody) -> HashMap<BlockId, LivenessInfo> {
    let mut live_sets: HashMap<BlockId, LivenessInfo> = HashMap::new();

    // 初始化所有块
    for block in &body.blocks {
        live_sets.insert(block.id, LivenessInfo {
            live_in: HashSet::new(),
            live_out: HashSet::new(),
        });
    }

    // 不动点迭代
    let mut changed = true;
    while changed {
        changed = false;

        for block in &body.blocks {
            let mut new_live_out = HashSet::new();

            // live_out[B] = ∪ live_in[S] for all successors S
            let succs = successors(&block.terminator);
            for succ_id in succs {
                if let Some(succ_info) = live_sets.get(&succ_id) {
                    new_live_out.extend(&succ_info.live_in);
                }
            }

            // 计算 gen 和 kill 集合
            let (uses, defs) = compute_gen_kill(block);

            // live_in[B] = gen[B] ∪ (live_out[B] - kill[B])
            let mut new_live_in = new_live_out.clone();
            for killed in &defs {
                new_live_in.remove(killed);
            }
            new_live_in.extend(&uses);

            // 检查是否改变
            let info = live_sets.get_mut(&block.id).unwrap();
            if info.live_in != new_live_in || info.live_out != new_live_out {
                info.live_in = new_live_in;
                info.live_out = new_live_out;
                changed = true;
            }
        }
    }

    live_sets
}

/// 计算基本块的 gen 和 kill 集合
///
/// gen 集合：块内向上暴露的使用（在被定义前使用的变量）
/// kill 集合：块内被定义的变量
///
/// 关键：terminator 中使用的变量必须在块出口处活着，
/// 因此这些变量需要在整个块内保持活性，即使它们在块内被定义。
fn compute_gen_kill(block: &HirBlock) -> (HashSet<LocalId>, HashSet<LocalId>) {
    let mut uses = HashSet::new();  // gen 集合：向上暴露的使用
    let mut defs = HashSet::new();  // kill 集合：被定义的变量

    // 正向遍历语句
    for stmt in block.stmts.iter() {
        match stmt {
            HirStmt::Assign { lhs, rhs, .. } => {
                // 先处理 rhs（使用），收集在此之前未定义的变量
                let mut rhs_uses = HashSet::new();
                collect_uses_from_rvalue(rhs, &mut rhs_uses);
                for var in rhs_uses {
                    if !defs.contains(&var) {
                        uses.insert(var);
                    }
                }

                // 再处理 lhs（定义）
                if let HirPlace::Local(local_id) = lhs {
                    defs.insert(*local_id);
                }
            }
            HirStmt::Call { .. } | HirStmt::Drop { .. } => {}
        }
    }

    // terminator 中的使用：这些变量在块出口处必须活着
    // 因此需要无条件加入 gen 集合（即使它们在块内被定义）
    let mut term_uses = HashSet::new();
    collect_uses_from_terminator(&block.terminator, &mut term_uses);
    uses.extend(term_uses);

    (uses, defs)
}

/// 从 Rvalue 收集使用的局部变量
fn collect_uses_from_rvalue(rvalue: &HirRvalue, uses: &mut HashSet<LocalId>) {
    match rvalue {
        HirRvalue::Use(operand) => collect_uses_from_operand(operand, uses),
        HirRvalue::BinaryOp { lhs, rhs, .. } => {
            collect_uses_from_operand(lhs, uses);
            collect_uses_from_operand(rhs, uses);
        }
        HirRvalue::UnaryOp { operand, .. } => {
            collect_uses_from_operand(operand, uses);
        }
        HirRvalue::Ref { place, .. } => {
            collect_uses_from_place(place, uses);
        }
        HirRvalue::Deref(operand) => {
            collect_uses_from_operand(operand, uses);
        }
        HirRvalue::Aggregate { fields, .. } => {
            for operand in fields {
                collect_uses_from_operand(operand, uses);
            }
        }
        HirRvalue::Discriminant(place) => {
            collect_uses_from_place(place, uses);
        }
        HirRvalue::ExtractPayload { place, .. } => {
            collect_uses_from_place(place, uses);
        }
        HirRvalue::ArrayToSlice { array, .. } => {
            collect_uses_from_operand(array, uses);
        }
    }
}

/// 从 Place 收集使用的局部变量
fn collect_uses_from_place(place: &HirPlace, uses: &mut HashSet<LocalId>) {
    if let HirPlace::Local(local_id) = place {
        uses.insert(*local_id);
    }
}

/// 从 Operand 收集使用的局部变量
fn collect_uses_from_operand(operand: &HirOperand, uses: &mut HashSet<LocalId>) {
    if let HirOperand::Place(place) = operand {
        collect_uses_from_place(place.as_ref(), uses);
    }
}

/// 从 Terminator 收集使用的局部变量
fn collect_uses_from_terminator(term: &HirTerminator, uses: &mut HashSet<LocalId>) {
    match term {
        HirTerminator::Return(Some(operand)) => {
            collect_uses_from_operand(operand, uses);
        }
        HirTerminator::Switch { discr, .. } => {
            collect_uses_from_operand(discr, uses);
        }
        _ => {}
    }
}

/// 提取终结符的后继块
fn successors(term: &HirTerminator) -> Vec<BlockId> {
    match term {
        HirTerminator::Goto(target) => vec![*target],
        HirTerminator::Return(_) => vec![],
        HirTerminator::Switch { targets, otherwise, .. } => {
            let mut succs = vec![*otherwise];
            for (_, target) in targets {
                succs.push(*target);
            }
            succs
        }
        HirTerminator::Unreachable => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleend::hir::*;
    use crate::diag::{Span, FileId};

    #[test]
    fn test_remove_dead_store() {
        use crate::diag::FileId;

        let mut body = HirBody {
            blocks: vec![HirBlock {
                id: BlockId(0),
                stmts: vec![
                    // x = 10; (dead)
                    HirStmt::Assign {
                        lhs: HirPlace::Local(LocalId(0)),
                        rhs: HirRvalue::Use(HirOperand::Const(Const::Int(10))),
                        span: Span::new(FileId(0), 0, 0),
                    },
                    // y = 20; (live)
                    HirStmt::Assign {
                        lhs: HirPlace::Local(LocalId(1)),
                        rhs: HirRvalue::Use(HirOperand::Const(Const::Int(20))),
                        span: Span::new(FileId(0), 0, 0),
                    },
                ],
                terminator: HirTerminator::Return(Some(HirOperand::Place(Box::new(HirPlace::Local(LocalId(1)))))),
                span: Span::new(FileId(0), 0, 0),
            }],
            locals: vec![],
            entry_block: BlockId(0),
        };

        let mut pass = DeadStoreElimination;
        let changed = pass.run_on_body(&mut body);

        eprintln!("After DSE: {} stmts, changed: {}", body.blocks[0].stmts.len(), changed);
        assert!(changed, "Expected DSE to make changes");
        assert_eq!(body.blocks[0].stmts.len(), 1, "Expected 1 stmt after DSE, got {}", body.blocks[0].stmts.len());
    }

    #[test]
    fn test_all_stores_live() {
        let mut body = HirBody {
            blocks: vec![HirBlock {
                id: BlockId(0),
                stmts: vec![
                    HirStmt::Assign {
                        lhs: HirPlace::Local(LocalId(0)),
                        rhs: HirRvalue::Use(HirOperand::Const(Const::Int(10))),
                        span: Span::new(FileId(0), 0, 0),
                    },
                    HirStmt::Assign {
                        lhs: HirPlace::Local(LocalId(1)),
                        rhs: HirRvalue::BinaryOp {
                            op: BinOp::Add,
                            lhs: HirOperand::Place(Box::new(HirPlace::Local(LocalId(0)))),
                            rhs: HirOperand::Const(Const::Int(5)),
                        },
                        span: Span::new(FileId(0), 0, 0),
                    },
                ],
                terminator: HirTerminator::Return(Some(HirOperand::Place(Box::new(HirPlace::Local(LocalId(1)))))),
                span: Span::new(FileId(0), 0, 0),
            }],
            locals: vec![],
            entry_block: BlockId(0),
        };

        let mut pass = DeadStoreElimination;
        let changed = pass.run_on_body(&mut body);

        assert!(!changed);  // 没有改变
        assert_eq!(body.blocks[0].stmts.len(), 2);
    }
}
