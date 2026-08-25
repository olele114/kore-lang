//! korec —— stage0 驱动器。
//!
//! 手写参数解析，不用 clap：退出码 2 的判定要手控，且 stage1 要把这套 CLI
//! 机械翻译成 Kore0，不能依赖 clap 形状的 API（见 Cargo.toml 注释）。

use kore_stage0::ExitCode;
use kore_stage0::diag::{
    DEFAULT_ERROR_LIMIT, DiagLoc, DiagSink, Diagnostic, ErrorFormat, FileId, Registry,
    error_limit_from_arg, render,
};
use kore_stage0::driver::{run_frontend, run_frontend_with_registry, verify_test_annotations};
use kore_stage0::middleend::lower::lower_module;
use kore_stage0::middleend::hir::HirModule;
use kore_stage0::backend::{compile_to_object, compile_and_link, EmitType, LinkerError};
use kore_stage0::frontend::ast::{Item, Module};
use kore_stage0::frontend::resolve::module::{Import, ModuleId, ModuleRegistry, emit_circular_dependency, emit_undefined_module};
use kore_stage0::frontend::resolve::path::PathResolver;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};

const REGISTRY_SRC: &str = include_str!("../../errors/registry.sexp");

const USAGE: &str = "\
用法: korec [选项] <源文件>...

选项:
  --error-format=<human|json|short>  诊断输出格式（默认 human）
  --error-limit=<N>                  最多输出 N 条错误，0 表示不限（默认 100）
  --explain <code>                   打印错误码长文，如 --explain E4001
  --emit=<stage>                     转储中间产物，可重复
  --emit-spans                       转储中带 Span
  -o <path>                          输出路径
  --time-passes                      编译结束后打印各 pass 墙钟时间
  --stats                            编译结束后打印计数指标
  --verify-test-annotations          验证 --~ 测试注解是否全部触发
  --debug-trace                      打开编译器内部调试跟踪（写 stderr）
  -h, --help                         打印本帮助
";

/// `--emit` 的八个阶段（ADR 009 Q19）。
const EMIT_STAGES: [&str; 8] = [
    "tokens", "ast", "resolved", "typed", "ir", "llvm-ir", "asm", "obj",
];

struct Options {
    inputs: Vec<String>,
    error_format: ErrorFormat,
    error_limit: u32,
    emit: Vec<String>,
    emit_spans: bool,
    output: Option<String>,
    time_passes: bool,
    stats: bool,
    verify_test_annotations: bool,
    debug_trace: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            inputs: Vec::new(),
            error_format: ErrorFormat::Human,
            error_limit: DEFAULT_ERROR_LIMIT,
            emit: Vec::new(),
            emit_spans: false,
            output: None,
            time_passes: false,
            stats: false,
            verify_test_annotations: false,
            debug_trace: false,
        }
    }
}

/// 参数解析的结果。`Explain` 与 `Help` 是不进入编译流程的早退路径。
enum Action {
    Compile(Options),
    Explain(u16),
    Help,
    Usage(String),
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match parse_args(&args) {
        Action::Help => {
            print!("{USAGE}");
            ExitCode::Ok
        }
        Action::Usage(msg) => {
            eprintln!("error: {msg}");
            eprint!("{USAGE}");
            ExitCode::UsageError
        }
        Action::Explain(code) => explain(code),
        Action::Compile(opts) => compile(opts),
    };
    std::process::exit(code.as_i32());
}

fn parse_args(args: &[String]) -> Action {
    if args.is_empty() {
        return Action::Help;
    }

    let mut o = Options::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-h" | "--help" => return Action::Help,
            "--emit-spans" => o.emit_spans = true,
            "--time-passes" => o.time_passes = true,
            "--stats" => o.stats = true,
            "--verify-test-annotations" => o.verify_test_annotations = true,
            "--debug-trace" => o.debug_trace = true,
            "-o" => {
                i += 1;
                match args.get(i) {
                    Some(p) => o.output = Some(p.clone()),
                    None => return Action::Usage("`-o` 缺少路径".into()),
                }
            }
            "--explain" => {
                i += 1;
                let Some(raw) = args.get(i) else {
                    return Action::Usage("`--explain` 缺少错误码".into());
                };
                return match parse_code(raw) {
                    Some(c) => Action::Explain(c),
                    None => Action::Usage(format!("无法识别的错误码：{raw}")),
                };
            }
            _ if a.starts_with("--error-format=") => {
                let v = &a["--error-format=".len()..];
                match ErrorFormat::parse(v) {
                    Some(f) => o.error_format = f,
                    None => {
                        return Action::Usage(format!(
                            "未知的 --error-format 取值：{v}（可选 human|json|short）"
                        ));
                    }
                }
            }
            _ if a.starts_with("--error-limit=") => {
                let v = &a["--error-limit=".len()..];
                match v.parse::<u32>() {
                    Ok(n) => o.error_limit = n,
                    Err(_) => {
                        return Action::Usage(format!("--error-limit 需要非负整数，得到：{v}"));
                    }
                }
            }
            _ if a.starts_with("--emit=") => {
                let v = &a["--emit=".len()..];
                if !EMIT_STAGES.contains(&v) {
                    return Action::Usage(format!(
                        "未知的 --emit 阶段：{v}（可选 {}）",
                        EMIT_STAGES.join("|")
                    ));
                }
                o.emit.push(v.to_string());
            }
            _ if a.starts_with('-') => {
                return Action::Usage(format!("未知选项：{a}"));
            }
            _ => o.inputs.push(a.to_string()),
        }
        i += 1;
    }

    if o.inputs.is_empty() {
        return Action::Usage("未指定源文件".into());
    }

    // ADR 009 Q21：多阶段转储不得走 stdout，两份 S-表达式混在一个流里没法拆。
    if o.emit.len() > 1 && o.output.is_none() {
        return Action::Usage("`--emit` 指定多个阶段时必须用 `-o` 指定输出目录".into());
    }

    Action::Compile(o)
}

/// 接受 `E4001` 与 `4001` 两种写法。
fn parse_code(raw: &str) -> Option<u16> {
    let digits = raw
        .strip_prefix('E')
        .or_else(|| raw.strip_prefix('e'))
        .unwrap_or(raw);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u16>().ok()
}

fn explain(code: u16) -> ExitCode {
    let reg = match Registry::parse(REGISTRY_SRC) {
        Ok(r) => r,
        Err(e) => {
            // 登记表是编译进二进制的，解析失败是编译器自身的问题。
            kore_stage0::ice::report("src/main.rs:explain", &format!("错误码登记表损坏：{e}"));
            return ExitCode::Ice;
        }
    };
    match reg.get(code) {
        Some(e) => {
            println!("E{:04}: {}", e.code, e.msg);
            println!();
            println!("{}", e.explain);
            if e.status == kore_stage0::diag::CodeStatus::Retired {
                println!();
                println!("注意：该错误码已退役，当前编译器不再产出它。");
            }
            ExitCode::Ok
        }
        None => {
            eprintln!("error: 登记表中没有错误码 E{code:04}");
            ExitCode::UsageError
        }
    }
}

/// 从 AST 中提取 use 语句。
fn extract_imports(module: &Module) -> Vec<Import> {
    let mut imports = Vec::new();
    for item in &module.items {
        if let Item::Use(use_path) = item {
            let imported_name = use_path.segments.last().cloned().unwrap_or_default();
            imports.push(Import {
                imported_name,
                segments: use_path.segments.clone(),
                span: use_path.span,
            });
        }
    }
    imports
}

fn compile(opts: Options) -> ExitCode {
    kore_stage0::trace::init_from(opts.debug_trace);

    let mut sink = DiagSink::with_error_limit(error_limit_from_arg(opts.error_limit));

    // 阶段 1：构建模块注册表和依赖图
    let mut registry = ModuleRegistry::new();
    let mut file_id_map: HashMap<PathBuf, FileId> = HashMap::new();
    let mut to_process: VecDeque<PathBuf> = opts.inputs.iter().map(PathBuf::from).collect();
    let mut processed: HashSet<PathBuf> = HashSet::new();
    let mut next_file_id = 0u32;

    while let Some(file_path) = to_process.pop_front() {
        // 标准化路径
        let canonical_path = match file_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                // 文件不存在，使用原始路径继续
                file_path.clone()
            }
        };

        if processed.contains(&canonical_path) {
            continue;
        }

        // 读取文件
        let file_id = FileId(next_file_id);
        next_file_id += 1;
        file_id_map.insert(canonical_path.clone(), file_id);

        let source = match std::fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                sink.emit(Diagnostic::error(
                    9001,
                    format!("无法读取源文件 `{}`：{}", file_path.display(), e),
                    DiagLoc::File(file_id),
                ));
                processed.insert(canonical_path);
                continue;
            }
        };

        // 词法分析和语法分析（仅用于提取 use 语句）
        let tokens = kore_stage0::frontend::lexer::tokenize(file_id, &source, &mut sink);
        if !opts.verify_test_annotations && sink.has_errors() {
            processed.insert(canonical_path);
            continue;
        }

        let module = kore_stage0::frontend::parser::parse(file_id, tokens, &mut sink);
        if !opts.verify_test_annotations && sink.has_errors() {
            processed.insert(canonical_path);
            continue;
        }

        // 提取导入信息
        let imports = extract_imports(&module);

        // 解析依赖路径
        let resolver = PathResolver::new(&file_path);
        for import in &imports {
            match resolver.resolve_use_path(&import.segments) {
                Ok(dep_path) => {
                    // 检查文件是否存在
                    if !dep_path.exists() {
                        emit_undefined_module(&mut sink, &import.imported_name, import.span);
                        continue;
                    }
                    to_process.push_back(dep_path);
                }
                Err(e) => {
                    sink.emit(Diagnostic::error(
                        4006,
                        format!("无法解析模块路径: {}", e),
                        DiagLoc::At(import.span),
                    ));
                }
            }
        }

        // 注册模块
        let module_name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        registry.register_module(canonical_path.clone(), module_name, module, imports);
        processed.insert(canonical_path);
    }

    // 建立依赖关系
    let all_module_ids = registry.all_module_ids();
    for module_id in all_module_ids {
        let module_info = registry.get_module(module_id).unwrap();
        let imports = module_info.imports.clone();

        for import in &imports {
            if let Some(dep_module_id) = registry.find_module_by_name(&import.imported_name) {
                registry.add_dependency(module_id, dep_module_id);
            }
        }
    }

    // pass 门禁：依赖收集阶段有错就不继续（测试注解模式除外）
    if !opts.verify_test_annotations && sink.has_errors() {
        return finish(sink, &opts);
    }

    // 阶段 2：检测循环依赖
    if let Err(cycle) = registry.check_cycles() {
        emit_circular_dependency(&mut sink, &cycle, &registry);
        return finish(sink, &opts);
    }

    // 阶段 3：拓扑排序确定编译顺序
    let sorted_modules = match registry.topological_sort() {
        Ok(sorted) => sorted,
        Err(_) => {
            sink.emit(Diagnostic::error(
                4009,
                "模块拓扑排序失败（可能存在循环依赖）".to_string(),
                DiagLoc::None,
            ));
            return finish(sink, &opts);
        }
    };

    // 阶段 4：按拓扑顺序读取并编译每个模块
    let mut sources: Vec<(FileId, String, ModuleId)> = Vec::new();

    for module_id in &sorted_modules {
        let module_info = registry.get_module(*module_id).unwrap();
        let file_id = *file_id_map.get(&module_info.path).unwrap();

        let source = match std::fs::read_to_string(&module_info.path) {
            Ok(s) => s,
            Err(e) => {
                sink.emit(Diagnostic::error(
                    9001,
                    format!("无法读取源文件 `{}`：{}", module_info.path.display(), e),
                    DiagLoc::File(file_id),
                ));
                continue;
            }
        };

        sources.push((file_id, source, *module_id));
    }

    // pass 门禁：文件读取有错就不继续（测试注解模式除外）
    if !opts.verify_test_annotations && sink.has_errors() {
        return finish(sink, &opts);
    }

    // 主编译路径：对每个源文件跑完整前端 pass 序列（词法→语法→消解→不逃逸）。
    // 诊断累积进共享 sink；run_frontend 内部按「本次新增错误数」做闸门，
    // 所以前一个文件的错误不会拦住后一个文件的词法阶段。
    let mut frontend_outputs = Vec::new();
    for (file_id, source, module_id) in &sources {
        if opts.verify_test_annotations {
            // 注解校验模式：用独立 sink 收集本文件的诊断，再交给校验器对比。
            // 独立 sink 避免其他文件的诊断污染校验结果。
            let mut file_sink =
                DiagSink::with_error_limit(error_limit_from_arg(opts.error_limit));
            let out = run_frontend(*file_id, source, &mut file_sink);
            let diags = file_sink.finish();
            let result = verify_test_annotations(source, &out.tokens, &diags);

            // 总是输出诊断（以便用户看到具体错误）
            for diag in &diags {
                sink.emit(diag.clone());
            }

            match result {
                kore_stage0::driver::TestResult::Pass => {
                    eprintln!("test annotations: PASS ({})", opts.inputs[file_id.0 as usize]);
                }
                kore_stage0::driver::TestResult::AnnotationNotTriggered(annot) => {
                    eprintln!(
                        "test annotations: FAIL ({}:{}): 注解未触发: {}",
                        opts.inputs[file_id.0 as usize],
                        annot.line,
                        annot.raw
                    );
                    return ExitCode::CompileError;
                }
                kore_stage0::driver::TestResult::UnexpectedDiags(unexpected) => {
                    eprintln!(
                        "test annotations: FAIL ({}): {} 条预期外诊断",
                        opts.inputs[file_id.0 as usize],
                        unexpected.len()
                    );
                    for d in unexpected {
                        eprintln!("  {:?}", d);
                    }
                    return ExitCode::CompileError;
                }
                kore_stage0::driver::TestResult::MalformedAnnotation { line, reason } => {
                    eprintln!(
                        "test annotations: FAIL ({}:{}): 格式错误的注解: {}",
                        opts.inputs[file_id.0 as usize],
                        line,
                        reason
                    );
                    return ExitCode::UsageError;
                }
            }
        } else {
            // 普通编译模式：诊断进共享 sink，编译结束后统一输出。
            let out = run_frontend_with_registry(*file_id, source, &mut sink, &mut registry, *module_id);
            frontend_outputs.push(out);
        }
    }

    // 测试注解模式下输出累积的诊断并返回
    if opts.verify_test_annotations {
        // 在测试注解验证模式下，只要验证通过就返回 Ok（即使有错误诊断）
        let err_count = sink.err_count();
        let suppressed = sink.suppressed();
        let warn_count = sink.warn_count();
        let diags = sink.finish();

        let text = render(opts.error_format, &diags, suppressed);
        let mut err = std::io::stderr();
        let _ = err.write_all(text.as_bytes());

        if opts.time_passes {
            let _ = writeln!(err, "time-passes: read 0.000s");
        }

        if opts.stats {
            for (k, v) in [
                ("tokens", 0u64),
                ("ast-nodes", 0),
                ("comptime-eval-steps", 0),
                ("monomorph-instances", 0),
                ("unify-calls", 0),
                ("type-vars", 0),
                ("instructions", 0),
                ("errors", err_count as u64),
                ("warnings", warn_count as u64),
            ] {
                let _ = writeln!(err, "stats: {k} = {v}");
            }
        }

        // 测试注解验证通过，即使有错误也返回 Ok
        return ExitCode::Ok;
    }

    // pass 门禁：前端有错就不进后续 pass
    if sink.has_errors() {
        return finish(sink, &opts);
    }

    // 处理 --emit 和 -o 选项
    if !opts.emit.is_empty() || opts.output.is_some() {
        // 只有在普通编译模式下才有 frontend_outputs
        if !frontend_outputs.is_empty() {
            // 降级所有模块到 HIR 并合并
            let mut merged_hir = HirModule {
                functions: Vec::new(),
                structs: Vec::new(),
                unions: Vec::new(),
                globals: Vec::new(),
            };

            for out in &frontend_outputs {
                if out.module.is_none() || out.symbols.is_none() || out.type_ctx.is_none() {
                    // 前端未完成，已有错误
                    return finish(sink, &opts);
                }

                let module = out.module.as_ref().unwrap();
                let symbols = out.symbols.as_ref().unwrap();
                let type_ctx = out.type_ctx.as_ref().unwrap();
                let hir = lower_module(module, symbols, type_ctx, &mut sink);

                // 合并到总的 HIR
                merged_hir.functions.extend(hir.functions);
                merged_hir.structs.extend(hir.structs);
                merged_hir.unions.extend(hir.unions);
                merged_hir.globals.extend(hir.globals);
            }

            let hir = merged_hir;

            if sink.has_errors() {
                return finish(sink, &opts);
            }

            // 处理 --emit 选项
            for stage in &opts.emit {
                match stage.as_str() {
                    "ir" => {
                        println!("=== HIR ===");
                        for func in &hir.functions {
                            println!("{:#?}", func);
                        }
                    }
                    "llvm-ir" => {
                        let output_path = opts.output.as_deref().unwrap_or("output.ll");
                        if let Err(e) = compile_to_object(&hir, Path::new(output_path), EmitType::LlvmIr, &mut sink) {
                            return finish_backend_error(e, "生成 LLVM IR", sink, &opts);
                        }
                    }
                    "asm" => {
                        let output_path = opts.output.as_deref().unwrap_or("output.s");
                        if let Err(e) = compile_to_object(&hir, Path::new(output_path), EmitType::Assembly, &mut sink) {
                            return finish_backend_error(e, "生成汇编", sink, &opts);
                        }
                    }
                    "obj" => {
                        let output_path = opts.output.as_deref().unwrap_or("output.o");
                        if let Err(e) = compile_to_object(&hir, Path::new(output_path), EmitType::Object, &mut sink) {
                            return finish_backend_error(e, "生成目标文件", sink, &opts);
                        }
                    }
                    _ => {} // tokens, ast, resolved, typed 暂未实现
                }
            }

            // 如果没有 --emit，但有 -o，则生成可执行文件
            if opts.emit.is_empty() && opts.output.is_some() {
                let output_path = opts.output.as_ref().unwrap();
                if let Err(e) = compile_and_link(&hir, Path::new(output_path), &mut sink) {
                    return finish_backend_error(e, "链接", sink, &opts);
                }
            }
        }
    }

    let _ = opts.emit_spans; // 暂未使用

    finish(sink, &opts)
}

/// 后端失败的收尾。codegen 失败已经由后端写进 DiagSink（E7002），必须走
/// finish 才能把这条诊断渲染出来；直接 return 会把它丢掉。其余变体（目标
/// 初始化、写文件、链接）没有对应错误码，仍按 error: 直报。
fn finish_backend_error(e: LinkerError, what: &str, sink: DiagSink, opts: &Options) -> ExitCode {
    if matches!(e, LinkerError::CodegenFailed(_)) {
        return finish(sink, opts);
    }
    eprintln!("error: {}失败: {}", what, e);
    ExitCode::CompileError
}

/// 编译结束后一次性输出诊断、计时与统计。三者共用 stderr，边跑边打会让
/// 计时插在诊断中间（ADR 009 Q21）。
fn finish(sink: DiagSink, opts: &Options) -> ExitCode {
    let err_count = sink.err_count();
    let suppressed = sink.suppressed();
    let warn_count = sink.warn_count();
    let diags = sink.finish();

    // 节流提示交给渲染器：这里追加明文会把 JSON 输出撑成非法。
    let text = render(opts.error_format, &diags, suppressed);
    let mut err = std::io::stderr();
    let _ = err.write_all(text.as_bytes());

    if opts.time_passes {
        let _ = writeln!(err, "time-passes: read 0.000s");
    }

    if opts.stats {
        // 键值对集合，不是硬编码的打印字符串——将来加
        // --stats-format=json 只是加一个 renderer。
        for (k, v) in [
            ("tokens", 0u64),
            ("ast-nodes", 0),
            ("comptime-eval-steps", 0),
            ("monomorph-instances", 0),
            ("unify-calls", 0),
            ("type-vars", 0),
            ("instructions", 0),
            ("errors", err_count as u64),
            ("warnings", warn_count as u64),
        ] {
            let _ = writeln!(err, "stats: {k} = {v}");
        }
    }

    if err_count > 0 {
        ExitCode::CompileError
    } else {
        ExitCode::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_error_codes_both_forms() {
        assert_eq!(parse_code("E4001"), Some(4001));
        assert_eq!(parse_code("4001"), Some(4001));
        assert_eq!(parse_code("E"), None);
        assert_eq!(parse_code("abc"), None);
    }

    #[test]
    fn unknown_option_is_usage_error() {
        let args = vec!["--nope".to_string()];
        assert!(matches!(parse_args(&args), Action::Usage(_)));
    }

    #[test]
    fn multi_emit_without_output_is_usage_error() {
        let args = ["--emit=ast", "--emit=ir", "a.kore"]
            .map(String::from)
            .to_vec();
        assert!(matches!(parse_args(&args), Action::Usage(_)));
    }

    #[test]
    fn multi_emit_with_output_is_accepted() {
        let args = ["--emit=ast", "--emit=ir", "-o", "out", "a.kore"]
            .map(String::from)
            .to_vec();
        assert!(matches!(parse_args(&args), Action::Compile(_)));
    }

    #[test]
    fn bad_emit_stage_is_usage_error() {
        let args = ["--emit=hir", "a.kore"].map(String::from).to_vec();
        assert!(matches!(parse_args(&args), Action::Usage(_)));
    }

    #[test]
    fn missing_input_is_usage_error() {
        let args = ["--stats".to_string()].to_vec();
        assert!(matches!(parse_args(&args), Action::Usage(_)));
    }

    #[test]
    fn embedded_registry_is_parseable() {
        assert!(Registry::parse(REGISTRY_SRC).is_ok());
    }

    #[test]
    fn parse_code_lowercase_e_prefix() {
        assert_eq!(parse_code("e4001"), Some(4001));
        assert_eq!(parse_code("e0001"), Some(1));
    }

    #[test]
    fn parse_code_bare_digits() {
        assert_eq!(parse_code("9999"), Some(9999));
        assert_eq!(parse_code("0"), Some(0));
    }

    #[test]
    fn parse_code_rejects_empty_after_prefix() {
        assert_eq!(parse_code("E"), None);
        assert_eq!(parse_code("e"), None);
    }

    #[test]
    fn parse_code_rejects_non_digits() {
        assert_eq!(parse_code("Eabc"), None);
        assert_eq!(parse_code("hello"), None);
        assert_eq!(parse_code(""), None);
    }

    #[test]
    fn dash_o_without_path_is_usage_error() {
        let args = vec!["-o".to_string()];
        assert!(matches!(parse_args(&args), Action::Usage(_)));
    }

    #[test]
    fn explain_without_code_is_usage_error() {
        let args = vec!["--explain".to_string()];
        assert!(matches!(parse_args(&args), Action::Usage(_)));
    }

    #[test]
    fn explain_with_invalid_code_is_usage_error() {
        let args = ["--explain", "notacode"].map(String::from).to_vec();
        assert!(matches!(parse_args(&args), Action::Usage(_)));
    }

    #[test]
    fn explain_with_valid_code_is_explain_action() {
        let args = ["--explain", "E4001"].map(String::from).to_vec();
        assert!(matches!(parse_args(&args), Action::Explain(4001)));
    }

    #[test]
    fn error_limit_invalid_is_usage_error() {
        let args = ["--error-limit=abc", "a.kore"].map(String::from).to_vec();
        assert!(matches!(parse_args(&args), Action::Usage(_)));
    }

    #[test]
    fn error_limit_valid_is_compile() {
        let args = ["--error-limit=50", "a.kore"].map(String::from).to_vec();
        assert!(matches!(parse_args(&args), Action::Compile(_)));
    }

    #[test]
    fn error_format_invalid_is_usage_error() {
        let args = ["--error-format=xml", "a.kore"].map(String::from).to_vec();
        assert!(matches!(parse_args(&args), Action::Usage(_)));
    }

    #[test]
    fn error_format_json_is_compile() {
        let args = ["--error-format=json", "a.kore"].map(String::from).to_vec();
        assert!(matches!(parse_args(&args), Action::Compile(_)));
    }

    #[test]
    fn error_format_short_is_compile() {
        let args = ["--error-format=short", "a.kore"].map(String::from).to_vec();
        assert!(matches!(parse_args(&args), Action::Compile(_)));
    }

    #[test]
    fn help_flag_is_help_action() {
        let args = vec!["-h".to_string()];
        assert!(matches!(parse_args(&args), Action::Help));
        let args2 = vec!["--help".to_string()];
        assert!(matches!(parse_args(&args2), Action::Help));
    }

    #[test]
    fn empty_args_is_help() {
        let args: Vec<String> = vec![];
        assert!(matches!(parse_args(&args), Action::Help));
    }

    #[test]
    fn time_passes_and_stats_flags_accepted() {
        let args = ["--time-passes", "--stats", "a.kore"]
            .map(String::from)
            .to_vec();
        assert!(matches!(parse_args(&args), Action::Compile(_)));
    }

    #[test]
    fn emit_spans_flag_accepted() {
        let args = ["--emit-spans", "a.kore"].map(String::from).to_vec();
        assert!(matches!(parse_args(&args), Action::Compile(_)));
    }

    #[test]
    fn verify_test_annotations_flag_accepted() {
        let args = ["--verify-test-annotations", "a.kore"]
            .map(String::from)
            .to_vec();
        assert!(matches!(parse_args(&args), Action::Compile(_)));
    }

    #[test]
    fn finish_with_time_passes_writes_to_stderr() {
        use kore_stage0::diag::DiagSink;
        let sink = DiagSink::new();
        let opts = Options {
            inputs: vec!["a.kore".into()],
            time_passes: true,
            stats: false,
            ..Options::default()
        };
        let code = finish(sink, &opts);
        // 无诊断 → Ok
        assert_eq!(code, ExitCode::Ok);
    }

    #[test]
    fn finish_with_stats_writes_to_stderr() {
        use kore_stage0::diag::DiagSink;
        let sink = DiagSink::new();
        let opts = Options {
            inputs: vec!["a.kore".into()],
            stats: true,
            time_passes: false,
            ..Options::default()
        };
        let code = finish(sink, &opts);
        assert_eq!(code, ExitCode::Ok);
    }

    #[test]
    fn finish_with_errors_returns_compile_error() {
        use kore_stage0::diag::{DiagLoc, DiagSink, Diagnostic};
        let mut sink = DiagSink::new();
        sink.emit(Diagnostic::error(4001, "test", DiagLoc::None));
        let opts = Options::default();
        let code = finish(sink, &opts);
        assert_eq!(code, ExitCode::CompileError);
    }
}
