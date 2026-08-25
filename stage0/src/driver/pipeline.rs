//! 前端 pass 序列的协调。ADR 007 Q13：driver 负责串起 frontend 的各个 pass，
//! pass 本身互不引用。
//!
//! ADR 009 的闸门规则：某个 pass 报错后不进下一个 pass。所以这里不是简单地
//! 顺序调用，而是每步之后检查「本次调用新增的错误数」。看新增数而不是
//! `sink.has_errors()`，因为多文件编译共用一个 sink：前一个文件的错误不该
//! 让后一个文件在词法阶段就被拦掉。

use crate::diag::{DiagSink, FileId};
use crate::frontend::ast::Module;
use crate::frontend::escape::EscapeChecker;
use crate::frontend::lexer::{Token, tokenize};
use crate::frontend::parser::parse;
use crate::frontend::resolve::{ModuleId, ModuleRegistry, Resolver, SymbolTable};
use crate::frontend::typecheck::{TypeChecker, TypeContext};

/// 前端最后跑完的 pass。闸门在哪一步拦下的，就停在哪一档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 词法分析报错，未进入语法分析。
    Lex,
    /// 语法分析报错，未进入名字消解。
    Parse,
    /// 名字消解报错，未进入不逃逸检查。
    Resolve,
    /// 名字消解报错后不逃逸检查，未进入类型检查。
    Escape,
    /// 跑完类型检查（前端当前的最后一档）。
    TypeCheck,
}

/// 前端产物。各字段是否为 `Some` 取决于闸门在哪一步拦下。
pub struct FrontendOutput {
    /// 词法产物。始终存在——闸门再早也是在词法之后。
    pub tokens: Vec<Token>,
    /// 语法产物。词法闸门拦下时为 `None`。
    pub module: Option<Module>,
    /// 符号表。语法闸门拦下时为 `None`。
    pub symbols: Option<SymbolTable>,
    /// 类型上下文。类型检查完成时为 `Some`。
    pub type_ctx: Option<TypeContext>,
    /// 最后跑完的 pass。
    pub stage: Stage,
    /// 编译期求值步数（用于 --stats 报告）。
    pub comptime_eval_steps: usize,
}

/// 跑完前端的全部 pass：词法 → 语法 → 名字消解 → 不逃逸检查 → 类型检查。
///
/// 诊断累积进 `sink`，由调用方决定何时渲染。
pub fn run_frontend(file: FileId, source: &str, sink: &mut DiagSink) -> FrontendOutput {
    let before = sink.err_count();

    let tokens = tokenize(file, source, sink);
    if sink.err_count() > before {
        return FrontendOutput {
            tokens,
            module: None,
            symbols: None,
            type_ctx: None,
            stage: Stage::Lex,
            comptime_eval_steps: 0,
        };
    }

    // parse 消耗 tokens，但 tokens 还要交给测试注解校验器（注解本身是
    // comment token），所以这里克隆一份。
    let module = parse(file, tokens.clone(), sink);
    if sink.err_count() > before {
        return FrontendOutput {
            tokens,
            module: Some(module),
            symbols: None,
            type_ctx: None,
            stage: Stage::Parse,
            comptime_eval_steps: 0,
        };
    }

    let symbols = Resolver::new(sink).resolve(&module);
    if sink.err_count() > before {
        return FrontendOutput {
            tokens,
            module: Some(module),
            symbols: Some(symbols),
            type_ctx: None,
            stage: Stage::Resolve,
            comptime_eval_steps: 0,
        };
    }

    EscapeChecker::new(sink).check_module(&module);
    if sink.err_count() > before {
        return FrontendOutput {
            tokens,
            module: Some(module),
            symbols: Some(symbols),
            type_ctx: None,
            stage: Stage::Escape,
            comptime_eval_steps: 0,
        };
    }

    let mut type_checker = TypeChecker::new(&symbols, sink);
    type_checker.check_module(&module);
    let type_ctx = type_checker.type_context().clone();

    FrontendOutput {
        tokens,
        module: Some(module),
        symbols: Some(symbols),
        type_ctx: Some(type_ctx),
        stage: Stage::TypeCheck,
        comptime_eval_steps: 0,
    }
}

/// 跑完前端的全部 pass（多文件模式）：词法 → 语法 → 名字消解 → 不逃逸检查 → 类型检查。
///
/// 与 `run_frontend` 类似，但使用 ModuleRegistry 支持跨模块符号解析。
pub fn run_frontend_with_registry(
    file: FileId,
    source: &str,
    sink: &mut DiagSink,
    registry: &mut ModuleRegistry,
    current_module: ModuleId,
) -> FrontendOutput {
    let before = sink.err_count();

    let tokens = tokenize(file, source, sink);
    if sink.err_count() > before {
        return FrontendOutput {
            tokens,
            module: None,
            symbols: None,
            type_ctx: None,
            stage: Stage::Lex,
            comptime_eval_steps: 0,
        };
    }

    let module = parse(file, tokens.clone(), sink);
    if sink.err_count() > before {
        return FrontendOutput {
            tokens,
            module: Some(module),
            symbols: None,
            type_ctx: None,
            stage: Stage::Parse,
            comptime_eval_steps: 0,
        };
    }

    let symbols = Resolver::with_registry(sink, registry, current_module).resolve(&module);
    if sink.err_count() > before {
        return FrontendOutput {
            tokens,
            module: Some(module),
            symbols: Some(symbols),
            type_ctx: None,
            stage: Stage::Resolve,
            comptime_eval_steps: 0,
        };
    }

    EscapeChecker::new(sink).check_module(&module);
    if sink.err_count() > before {
        return FrontendOutput {
            tokens,
            module: Some(module),
            symbols: Some(symbols),
            type_ctx: None,
            stage: Stage::Escape,
            comptime_eval_steps: 0,
        };
    }

    let mut type_checker = TypeChecker::new(&symbols, sink);
    type_checker.check_module(&module);
    let type_ctx = type_checker.type_context().clone();

    FrontendOutput {
        tokens,
        module: Some(module),
        symbols: Some(symbols),
        type_ctx: Some(type_ctx),
        stage: Stage::TypeCheck,
        comptime_eval_steps: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::ErrorCode;

    fn run(source: &str) -> (FrontendOutput, Vec<u16>) {
        let mut sink = DiagSink::new();
        let out = run_frontend(FileId(0), source, &mut sink);
        let codes = sink.finish().iter().map(|d| d.code).collect();
        (out, codes)
    }

    #[test]
    fn clean_source_runs_through_typecheck() {
        let (out, codes) = run("f :: (x own ^T) void => { y := x }");
        assert_eq!(out.stage, Stage::TypeCheck);
        assert!(out.module.is_some());
        assert!(out.symbols.is_some());
        assert!(codes.is_empty(), "干净源码不应有诊断，实际：{codes:?}");
    }

    #[test]
    fn empty_source_runs_through_typecheck() {
        let (out, codes) = run("");
        assert_eq!(out.stage, Stage::TypeCheck);
        assert!(codes.is_empty(), "空文件不应有诊断，实际：{codes:?}");
    }

    #[test]
    fn escape_violation_is_reported_by_pipeline() {
        // 走完整管线也要能报出 E5001，证明 pass 确实被接上了。
        let (out, codes) = run(
            "f :: (x own ^T) void => {
  y := x
  z := x
}",
        );
        assert_eq!(out.stage, Stage::Escape);
        assert!(
            codes.contains(&ErrorCode::UseAfterMove.as_u16()),
            "应报 E5001，实际：{codes:?}"
        );
    }

    #[test]
    fn lex_error_stops_before_parse() {
        // 未闭合字符串是词法错误，闸门应拦在 Lex 档。
        let (out, _codes) = run("f :: () str => \"unclosed");
        assert_eq!(out.stage, Stage::Lex);
        assert!(out.module.is_none(), "词法失败后不应有 AST");
        assert!(out.symbols.is_none());
    }

    #[test]
    fn tokens_survive_for_annotation_verification() {
        // 校验器要读 comment token，管线必须把 tokens 交回来。
        let (out, _codes) = run("f :: () i32 => 42  --~ E9999");
        assert!(
            out.tokens.iter().any(|t| matches!(
                &t.kind,
                crate::frontend::lexer::TokenKind::Comment(
                    crate::frontend::lexer::CommentKind::TestAnnot,
                    _
                )
            )),
            "测试注解应作为 comment token 保留在词法产物中"
        );
    }
}
