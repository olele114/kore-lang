//! 死代码消除 Pass。
//!
//! 移除不可达的基本块，减少 HIR 体积。
//! 算法：从入口块开始，标记所有可达块，删除未标记的块。

use crate::middleend::hir::{BlockId, HirBody, HirTerminator};
use std::collections::HashSet;

use super::Pass;

/// 死代码消除 Pass
pub struct DeadCodeElimination;

impl Pass for DeadCodeElimination {
    fn name(&self) -> &str {
        "dead-code-elimination"
    }

    fn run_on_body(&mut self, body: &mut HirBody) -> bool {
        // 1. 标记可达块
        let reachable = compute_reachable_blocks(body);

        // 2. 删除不可达块
        let original_count = body.blocks.len();
        body.blocks.retain(|block| reachable.contains(&block.id));

        // 3. 返回是否做了修改
        body.blocks.len() < original_count
    }
}

/// 计算从入口块可达的所有块
fn compute_reachable_blocks(body: &HirBody) -> HashSet<BlockId> {
    let mut reachable = HashSet::new();
    let mut worklist = vec![body.entry_block];

    while let Some(block_id) = worklist.pop() {
        // 已访问过，跳过
        if reachable.contains(&block_id) {
            continue;
        }

        reachable.insert(block_id);

        // 查找该块
        if let Some(block) = body.blocks.iter().find(|b| b.id == block_id) {
            // 将后继块加入工作列表
            for succ in successors(&block.terminator) {
                worklist.push(succ);
            }
        }
    }

    reachable
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
    fn test_remove_unreachable_blocks() {
        let mut body = HirBody {
            blocks: vec![
                HirBlock {
                    id: BlockId(0),
                    stmts: vec![],
                    terminator: HirTerminator::Goto(BlockId(1)),
                    span: Span::new(FileId(0), 0, 0),
                },
                HirBlock {
                    id: BlockId(1),
                    stmts: vec![],
                    terminator: HirTerminator::Return(None),
                    span: Span::new(FileId(0), 0, 0),
                },
                // bb2 不可达
                HirBlock {
                    id: BlockId(2),
                    stmts: vec![],
                    terminator: HirTerminator::Unreachable,
                    span: Span::new(FileId(0), 0, 0),
                },
            ],
            locals: vec![],
            entry_block: BlockId(0),
        };

        let mut pass = DeadCodeElimination;
        let changed = pass.run_on_body(&mut body);

        assert!(changed);
        assert_eq!(body.blocks.len(), 2);  // bb2 被删除
        assert!(body.blocks.iter().any(|b| b.id == BlockId(0)));
        assert!(body.blocks.iter().any(|b| b.id == BlockId(1)));
        assert!(!body.blocks.iter().any(|b| b.id == BlockId(2)));
    }

    #[test]
    fn test_all_blocks_reachable() {
        let mut body = HirBody {
            blocks: vec![
                HirBlock {
                    id: BlockId(0),
                    stmts: vec![],
                    terminator: HirTerminator::Goto(BlockId(1)),
                    span: Span::new(FileId(0), 0, 0),
                },
                HirBlock {
                    id: BlockId(1),
                    stmts: vec![],
                    terminator: HirTerminator::Return(None),
                    span: Span::new(FileId(0), 0, 0),
                },
            ],
            locals: vec![],
            entry_block: BlockId(0),
        };

        let mut pass = DeadCodeElimination;
        let changed = pass.run_on_body(&mut body);

        assert!(!changed);  // 没有改变
        assert_eq!(body.blocks.len(), 2);
    }
}
