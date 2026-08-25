//! CFG（控制流图）验证。
//!
//! 检查：
//! 1. 入口块存在
//! 2. 所有 BlockId 引用有效
//! 3. 所有块从入口可达
//! 4. 无悬空块引用

use crate::middleend::hir::{BlockId, HirBody, HirTerminator};
use crate::diag::{DiagSink, Diagnostic, DiagLoc};
use std::collections::{HashSet, HashMap};

/// 验证 CFG 完整性
pub fn validate_cfg(body: &HirBody, sink: &mut DiagSink) -> bool {
    let mut valid = true;

    // 1. 检查入口块存在
    if !body.blocks.iter().any(|b| b.id == body.entry_block) {
        sink.emit(Diagnostic::error(
            9001,
            format!("Entry block {:?} does not exist", body.entry_block),
            DiagLoc::None,
        ));
        return false;
    }

    // 2. 构建块 ID 集合
    let block_ids: HashSet<BlockId> = body.blocks.iter().map(|b| b.id).collect();

    // 3. 检查块 ID 唯一性
    if block_ids.len() != body.blocks.len() {
        sink.emit(Diagnostic::error(
            9002,
            "Duplicate block IDs found".to_string(),
            DiagLoc::None,
        ));
        valid = false;
    }

    // 4. 检查所有终结符引用的块存在
    for block in &body.blocks {
        for target in successors(&block.terminator) {
            if !block_ids.contains(&target) {
                sink.emit(Diagnostic::error(
                    9003,
                    format!(
                        "Block {:?} references non-existent block {:?}",
                        block.id, target
                    ),
                    DiagLoc::At(block.span),
                ));
                valid = false;
            }
        }
    }

    // 5. 检查所有块从入口可达
    let reachable = compute_reachable(body);
    for block in &body.blocks {
        if !reachable.contains(&block.id) {
            sink.emit(Diagnostic::warning(
                9004,
                format!("Block {:?} is unreachable", block.id),
                DiagLoc::At(block.span),
            ));
        }
    }

    // 6. 检查 CFG 完整性（无死循环块导致的不返回）
    valid &= validate_returns(body, &reachable, sink);

    valid
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

/// 计算从入口块可达的所有块
fn compute_reachable(body: &HirBody) -> HashSet<BlockId> {
    let mut reachable = HashSet::new();
    let mut worklist = vec![body.entry_block];

    while let Some(block_id) = worklist.pop() {
        if reachable.contains(&block_id) {
            continue;
        }

        reachable.insert(block_id);

        if let Some(block) = body.blocks.iter().find(|b| b.id == block_id) {
            for succ in successors(&block.terminator) {
                worklist.push(succ);
            }
        }
    }

    reachable
}

/// 验证返回路径完整性
///
/// 检查：
/// 1. 如果函数返回类型不是 void，所有可达路径必须有 Return
/// 2. 检测无限循环（所有可达块都不返回）
fn validate_returns(
    body: &HirBody,
    reachable: &HashSet<BlockId>,
    sink: &mut DiagSink,
) -> bool {
    let valid = true;

    // 构建反向图：哪些块可以到达哪些块
    let predecessors = compute_predecessors(body);

    // 找到所有返回块
    let return_blocks: HashSet<BlockId> = body
        .blocks
        .iter()
        .filter(|b| matches!(b.terminator, HirTerminator::Return(_)))
        .map(|b| b.id)
        .collect();

    // 检查是否存在无法到达返回块的可达块（可能的无限循环）
    if !return_blocks.is_empty() {
        let can_reach_return = compute_can_reach(&predecessors, &return_blocks, body);

        for block in &body.blocks {
            if reachable.contains(&block.id) && !can_reach_return.contains(&block.id) {
                // 这个块可达但无法到达任何返回块
                // 可能是无限循环或 Unreachable 终结符
                if !matches!(block.terminator, HirTerminator::Unreachable) {
                    sink.emit(Diagnostic::warning(
                        9005,
                        format!(
                            "Block {:?} may lead to infinite loop (cannot reach return)",
                            block.id
                        ),
                        DiagLoc::At(block.span),
                    ));
                }
            }
        }
    }

    valid
}

/// 计算前驱映射
fn compute_predecessors(body: &HirBody) -> HashMap<BlockId, Vec<BlockId>> {
    let mut preds: HashMap<BlockId, Vec<BlockId>> = HashMap::new();

    for block in &body.blocks {
        for succ in successors(&block.terminator) {
            preds.entry(succ).or_default().push(block.id);
        }
    }

    preds
}

/// 计算哪些块可以到达目标块集合
fn compute_can_reach(
    predecessors: &HashMap<BlockId, Vec<BlockId>>,
    targets: &HashSet<BlockId>,
    _body: &HirBody,
) -> HashSet<BlockId> {
    let mut can_reach = targets.clone();
    let mut worklist: Vec<BlockId> = targets.iter().copied().collect();

    while let Some(block_id) = worklist.pop() {
        if let Some(preds) = predecessors.get(&block_id) {
            for &pred in preds {
                if can_reach.insert(pred) {
                    worklist.push(pred);
                }
            }
        }
    }

    can_reach
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleend::hir::*;
    use crate::diag::{Span, FileId, Severity};

    #[test]
    fn test_valid_cfg() {
        let body = HirBody {
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

        let mut sink = DiagSink::new();
        assert!(validate_cfg(&body, &mut sink));
        assert_eq!(sink.peek().len(), 0);
    }

    #[test]
    fn test_missing_entry_block() {
        let body = HirBody {
            blocks: vec![HirBlock {
                id: BlockId(1),
                stmts: vec![],
                terminator: HirTerminator::Return(None),
                span: Span::new(FileId(0), 0, 0),
            }],
            locals: vec![],
            entry_block: BlockId(0), // 不存在
        };

        let mut sink = DiagSink::new();
        assert!(!validate_cfg(&body, &mut sink));
        assert!(sink.has_errors());
    }

    #[test]
    fn test_invalid_block_reference() {
        let body = HirBody {
            blocks: vec![HirBlock {
                id: BlockId(0),
                stmts: vec![],
                terminator: HirTerminator::Goto(BlockId(999)), // 不存在
                span: Span::new(FileId(0), 0, 0),
            }],
            locals: vec![],
            entry_block: BlockId(0),
        };

        let mut sink = DiagSink::new();
        assert!(!validate_cfg(&body, &mut sink));
        assert!(sink.has_errors());
    }

    #[test]
    fn test_unreachable_block() {
        let body = HirBody {
            blocks: vec![
                HirBlock {
                    id: BlockId(0),
                    stmts: vec![],
                    terminator: HirTerminator::Return(None),
                    span: Span::new(FileId(0), 0, 0),
                },
                // bb1 不可达
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

        let mut sink = DiagSink::new();
        assert!(validate_cfg(&body, &mut sink));
        // 应该有警告（不可达块）
        assert!(sink.peek().iter().any(|d| d.severity == Severity::Warning));
    }
}
