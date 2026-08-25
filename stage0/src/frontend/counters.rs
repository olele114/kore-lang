//! 确定性前端计数器。ADR 010：性能基准使用计数器而非墙钟时间，
//! 避免环境噪声干扰结果。计数器从 pass 输出派生，不侵入生产代码。

use crate::frontend::ast::{Module, Item, Expr, Stmt};

/// 前端 pass 的确定性计数器。
///
/// 每个字段代表一类工作单元，可用于跨 commit 对比性能退化。
#[derive(Debug, Default, Clone)]
pub struct FrontendCounters {
    /// 词法分析产生的记号数（含 Eof）。
    pub tokens_produced: usize,
    /// 语法分析产生的 AST 顶层节点数。
    pub items_parsed: usize,
    /// AST 中所有表达式节点的总数（递归计数）。
    pub expr_nodes: usize,
    /// 诊断发射总数（错误 + 警告）。
    pub diags_emitted: usize,
}

impl FrontendCounters {
    /// 从词法输出和语法输出构建计数器。
    pub fn from_outputs(
        tokens: &[crate::frontend::lexer::Token],
        module: &Module,
        diag_err: u32,
        diag_warn: u32,
    ) -> Self {
        FrontendCounters {
            tokens_produced: tokens.len(),
            items_parsed: module.items.len(),
            expr_nodes: count_exprs_in_module(module),
            diags_emitted: (diag_err + diag_warn) as usize,
        }
    }
}

fn count_exprs_in_module(module: &Module) -> usize {
    module.items.iter().map(count_exprs_in_item).sum()
}

fn count_exprs_in_item(item: &Item) -> usize {
    match item {
        Item::Func(f) => 1 + count_exprs(&f.body),
        Item::Struct(_) | Item::Union(_) | Item::Use(_) => 0,
    }
}

fn count_exprs(expr: &Expr) -> usize {
    1 + match expr {
        Expr::Binary { lhs, rhs, .. } => count_exprs(lhs) + count_exprs(rhs),
        Expr::Unary { operand, .. } => count_exprs(operand),
        Expr::Call { callee, args, .. } => {
            count_exprs(callee) + args.iter().map(count_exprs).sum::<usize>()
        }
        Expr::Field { base, .. } | Expr::Deref(base, _) | Expr::Propagate(base, _) => {
            count_exprs(base)
        }
        Expr::Index { base, index, .. } => count_exprs(base) + count_exprs(index),
        Expr::Loop { subject, body, .. } => {
            subject.as_deref().map_or(0, count_exprs) + count_exprs(body)
        }
        Expr::Branch { scrutinee, arms, .. } => {
            scrutinee.as_deref().map_or(0, count_exprs)
                + arms.iter().map(|a| count_exprs(&a.body)).sum::<usize>()
        }
        Expr::Block { stmts, .. } => stmts.iter().map(count_exprs_in_stmt).sum::<usize>(),
        Expr::Ret(inner, _) => inner.as_deref().map_or(0, count_exprs),
        Expr::Stop { .. } => 0,
        Expr::Jmp { target, .. } => target.as_deref().map_or(0, count_exprs),
        _ => 0,
    }
}

fn count_exprs_in_stmt(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Let { init, .. } => count_exprs(init),
        Stmt::Expr(e) => count_exprs(e),
        Stmt::Assign { target, value, .. } => count_exprs(target) + count_exprs(value),
        Stmt::Defer(e, _) => count_exprs(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{DiagSink, FileId};
    use crate::frontend::lexer::tokenize;
    use crate::frontend::parser::parse;

    #[test]
    fn empty_module_counts_zero_items_and_exprs() {
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), "", &mut sink);
        let module = parse(FileId(0), tokens.clone(), &mut sink);
        let counters = FrontendCounters::from_outputs(&tokens, &module, 0, 0);

        assert_eq!(counters.items_parsed, 0);
        assert_eq!(counters.expr_nodes, 0);
    }

    #[test]
    fn simple_function_counts_one_item() {
        let mut sink = DiagSink::new();
        let source = "f :: () i32 => 42";
        let tokens = tokenize(FileId(0), source, &mut sink);
        let module = parse(FileId(0), tokens.clone(), &mut sink);
        let counters = FrontendCounters::from_outputs(&tokens, &module, 0, 0);

        assert_eq!(counters.items_parsed, 1);
    }

    #[test]
    fn function_body_counts_as_one_expr() {
        let mut sink = DiagSink::new();
        let source = "f :: () i32 => 42";
        let tokens = tokenize(FileId(0), source, &mut sink);
        let module = parse(FileId(0), tokens.clone(), &mut sink);
        let counters = FrontendCounters::from_outputs(&tokens, &module, 0, 0);

        assert_eq!(counters.expr_nodes, 2);
    }

    #[test]
    fn binary_expr_counts_three_nodes() {
        let mut sink = DiagSink::new();
        let source = "f :: () i32 => 1 + 2";
        let tokens = tokenize(FileId(0), source, &mut sink);
        let module = parse(FileId(0), tokens.clone(), &mut sink);
        let counters = FrontendCounters::from_outputs(&tokens, &module, 0, 0);

        assert_eq!(counters.expr_nodes, 4);
    }

    #[test]
    fn block_with_multiple_stmts_counts_each_expr() {
        let mut sink = DiagSink::new();
        let source = "f :: () void => { x := 1; y := 2 }";
        let tokens = tokenize(FileId(0), source, &mut sink);
        let module = parse(FileId(0), tokens.clone(), &mut sink);
        let counters = FrontendCounters::from_outputs(&tokens, &module, 0, 0);

        assert!(counters.expr_nodes >= 3);
    }

    #[test]
    fn struct_and_union_count_zero_exprs() {
        let mut sink = DiagSink::new();
        let source = "S :: { x i32 }\nU :: .{ A | B }";
        let tokens = tokenize(FileId(0), source, &mut sink);
        let module = parse(FileId(0), tokens.clone(), &mut sink);
        let counters = FrontendCounters::from_outputs(&tokens, &module, 0, 0);

        assert_eq!(counters.items_parsed, 2);
        assert_eq!(counters.expr_nodes, 0);
    }

    #[test]
    fn diags_counted_correctly() {
        let mut sink = DiagSink::new();
        let tokens = tokenize(FileId(0), "", &mut sink);
        let module = parse(FileId(0), tokens.clone(), &mut sink);
        let counters = FrontendCounters::from_outputs(&tokens, &module, 3, 2);

        assert_eq!(counters.diags_emitted, 5);
    }

    #[test]
    fn tokens_include_eof() {
        let mut sink = DiagSink::new();
        let source = "x";
        let tokens = tokenize(FileId(0), source, &mut sink);
        let module = parse(FileId(0), tokens.clone(), &mut sink);
        let counters = FrontendCounters::from_outputs(&tokens, &module, 0, 0);

        assert!(counters.tokens_produced >= 2);
    }

    #[test]
    fn nested_binary_counts_all_levels() {
        let mut sink = DiagSink::new();
        let source = "f :: () i32 => 1 + 2 * 3";
        let tokens = tokenize(FileId(0), source, &mut sink);
        let module = parse(FileId(0), tokens.clone(), &mut sink);
        let counters = FrontendCounters::from_outputs(&tokens, &module, 0, 0);

        assert_eq!(counters.expr_nodes, 6);
    }

    #[test]
    fn call_with_args_counts_callee_and_args() {
        let mut sink = DiagSink::new();
        let source = "f :: () i32 => g(1, 2)";
        let tokens = tokenize(FileId(0), source, &mut sink);
        let module = parse(FileId(0), tokens.clone(), &mut sink);
        let counters = FrontendCounters::from_outputs(&tokens, &module, 0, 0);

        assert!(counters.expr_nodes >= 4);
    }

    #[test]
    fn loop_with_subject_counts_both() {
        let mut sink = DiagSink::new();
        let source = "f :: () void => @ cond { skip }";
        let tokens = tokenize(FileId(0), source, &mut sink);
        let module = parse(FileId(0), tokens.clone(), &mut sink);
        let counters = FrontendCounters::from_outputs(&tokens, &module, 0, 0);

        assert!(counters.expr_nodes >= 3);
    }

    #[test]
    fn branch_arms_all_counted() {
        let mut sink = DiagSink::new();
        let source = "f :: (x i32) i32 => ? { x => 10 | y => 20 | _ => 0 }";
        let tokens = tokenize(FileId(0), source, &mut sink);
        let module = parse(FileId(0), tokens.clone(), &mut sink);
        let counters = FrontendCounters::from_outputs(&tokens, &module, 0, 0);

        assert!(counters.expr_nodes >= 5);
    }
}
