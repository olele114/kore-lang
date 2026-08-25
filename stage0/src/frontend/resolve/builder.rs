//! 名字消解的建表逻辑。遍历 AST，填充符号表与作用域栈。
//!
//! ADR 004 §34–48：两趟扫描。第一趟收集所有模块级 `::` 绑定（函数、结构、
//! 联合），第二趟处理函数体与局部作用域。这样保证模块内前向引用合法。

use crate::diag::codes::ErrorCode;
use crate::diag::{DiagSink, FileId, Span};
use crate::frontend::ast::node::*;
use crate::frontend::ast::visitor::{walk_expr, walk_stmt, Visitor};
use crate::frontend::resolve::module::{emit_undefined_module, ModuleId, ModuleRegistry};
use crate::frontend::resolve::scope::{emit_redefinition_error, ScopeStack};
use crate::frontend::resolve::symbols::{Symbol, SymbolId, SymbolKind, SymbolTable};
use std::collections::HashMap;

/// 名字消解器。持有符号表、作用域栈和诊断接收器。
pub struct Resolver<'a> {
    table: SymbolTable,
    scopes: ScopeStack,
    sink: &'a mut DiagSink,
    /// 第一趟收集的模块级符号的位置，用于重定义检测。
    module_spans: HashMap<String, Span>,
    /// 当前处理的阶段。
    phase: Phase,
    /// 模块注册表引用（可选，单文件编译时为 None）
    registry: Option<&'a mut ModuleRegistry>,
    /// 当前模块 ID（多文件编译时使用）
    #[allow(dead_code)]
    current_module: Option<ModuleId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// 第一趟：只收集模块级 `::` 绑定。
    CollectModuleItems,
    /// 第二趟：处理函数体和局部作用域。
    ResolveLocals,
}

impl<'a> Resolver<'a> {
    pub fn new(sink: &'a mut DiagSink) -> Self {
        let mut resolver = Resolver {
            table: SymbolTable::new(),
            scopes: ScopeStack::default(),
            sink,
            module_spans: HashMap::new(),
            phase: Phase::CollectModuleItems,
            registry: None,
            current_module: None,
        };

        // 预注册内置函数
        resolver.register_builtins();

        resolver
    }

    /// 创建带有模块注册表的消解器（用于多文件编译）
    pub fn with_registry(
        sink: &'a mut DiagSink,
        registry: &'a mut ModuleRegistry,
        current_module: ModuleId,
    ) -> Self {
        let mut resolver = Resolver {
            table: SymbolTable::new(),
            scopes: ScopeStack::default(),
            sink,
            module_spans: HashMap::new(),
            phase: Phase::CollectModuleItems,
            registry: Some(registry),
            current_module: Some(current_module),
        };

        // 预注册内置函数
        resolver.register_builtins();

        resolver
    }

    /// 注册内置函数到符号表和模块作用域
    fn register_builtins(&mut self) {
        let builtins = vec![
            ("print", SymbolKind::Func),
            ("println", SymbolKind::Func),
            ("eprint", SymbolKind::Func),
            ("eprintln", SymbolKind::Func),
            ("read_file", SymbolKind::Func),
            ("write_file", SymbolKind::Func),
        ];

        for (name, kind) in builtins {
            let sym = Symbol {
                name: name.to_string(),
                kind,
                span: Span::new(FileId(0), 0, 0), // 内置函数使用虚拟位置
                is_mut: false,
                is_public: false,
            };
            let id = self.table.insert(sym);

            // 插入模块作用域，使用 unwrap 因为内置函数不应该冲突
            self.scopes.define_module(name.to_string(), id).unwrap();
            self.module_spans.insert(name.to_string(), Span::new(FileId(0), 0, 0));
        }
    }

    /// 两趟消解的入口。返回填充好的符号表。
    pub fn resolve(mut self, module: &Module) -> SymbolTable {
        // 第一趟：收集模块级项。
        self.phase = Phase::CollectModuleItems;
        self.visit_module(module);

        // 在第一趟结束后，立即填充导出表
        // 这样后续模块在第二趟就能看到这个模块的导出
        if let (Some(registry), Some(current_module)) = (self.registry.as_mut(), self.current_module) {
            if let Some(module_info) = registry.get_module_mut(current_module) {
                // 收集所有模块级符号（包括私有的），以便进行可见性检查
                for (_sym_id, symbol) in self.table.iter() {
                    // 只收集模块级别的符号（函数、类型等），不包括局部变量
                    match symbol.kind {
                        SymbolKind::Func | SymbolKind::Struct | SymbolKind::Union => {
                            module_info.exports.insert(symbol.clone());
                        }
                        _ => {}
                    }
                }
            }
        }

        // 第二趟：处理函数体。
        self.phase = Phase::ResolveLocals;
        self.visit_module(module);

        self.table
    }

    /// 定义一个模块级符号。
    fn define_module_symbol(&mut self, name: String, kind: SymbolKind, span: Span, is_public: bool) -> Option<SymbolId> {
        // 检查重定义。
        if let Some(old_span) = self.module_spans.get(&name) {
            emit_redefinition_error(self.sink, &name, span, *old_span);
            return None;
        }

        let sym = Symbol {
            name: name.clone(),
            kind,
            span,
            is_mut: false,
            is_public,
        };
        let id = self.table.insert(sym);

        // 记录位置。
        self.module_spans.insert(name.clone(), span);

        // 插入模块作用域。
        if let Err(old_id) = self.scopes.define_module(name.clone(), id) {
            // 理论上不会到这里，因为我们已经检查过 module_spans。
            if let Some(old_sym) = self.table.get(old_id) {
                emit_redefinition_error(self.sink, &name, span, old_sym.span);
            }
            return None;
        }

        Some(id)
    }

    /// 定义一个局部符号。
    fn define_local_symbol(&mut self, name: String, kind: SymbolKind, span: Span, is_mut: bool) -> Option<SymbolId> {
        let sym = Symbol {
            name: name.clone(),
            kind,
            span,
            is_mut,
            is_public: false,
        };
        let id = self.table.insert(sym);

        // 插入当前局部作用域。
        if let Err(old_id) = self.scopes.define_local(name.clone(), id) {
            // 同一作用域重定义。
            if let Some(old_sym) = self.table.get(old_id) {
                emit_redefinition_error(self.sink, &name, span, old_sym.span);
            }
            return None;
        }

        Some(id)
    }

    /// 查找名字。三层查找链：局部 → 模块 → 内建。
    /// Kore0 阶段无内建，所以只查前两层。
    fn lookup(&self, name: &str) -> Option<SymbolId> {
        self.scopes.lookup(name).or_else(|| self.scopes.lookup_module(name))
    }
}

impl<'a> Visitor for Resolver<'a> {
    fn visit_item(&mut self, it: &Item) {
        match self.phase {
            Phase::CollectModuleItems => {
                // 第一趟：只收集顶层项的名字。
                match it {
                    Item::Func(f) => {
                        self.define_module_symbol(f.name.clone(), SymbolKind::Func, f.span, f.is_public);
                    }
                    Item::Struct(s) => {
                        self.define_module_symbol(s.name.clone(), SymbolKind::Struct, s.span, s.is_public);
                        // 字段的消解延迟到类型检查，这里不处理。
                    }
                    Item::Union(u) => {
                        self.define_module_symbol(u.name.clone(), SymbolKind::Union, u.span, u.is_public);
                        // 变体的消解延迟到类型检查。
                    }
                    Item::Use(use_path) => {
                        // 处理 use 语句：将模块名绑定到当前作用域
                        if let Some(registry) = self.registry.as_ref() {
                            let target_module_name = use_path.segments.last().unwrap();

                            if let Some(target_module_id) = registry.find_module_by_name(target_module_name) {
                                // 将模块名绑定为符号
                                let symbol = Symbol {
                                    name: target_module_name.clone(),
                                    kind: SymbolKind::Module(target_module_id),
                                    span: use_path.span,
                                    is_mut: false,
                                    is_public: false,
                                };
                                let sym_id = self.table.insert(symbol);

                                // 插入模块级作用域
                                if let Err(old_id) = self.scopes.define_module(target_module_name.clone(), sym_id) {
                                    if let Some(old_sym) = self.table.get(old_id) {
                                        emit_redefinition_error(self.sink, target_module_name, use_path.span, old_sym.span);
                                    }
                                }

                                // 记录依赖关系
                                // 依赖关系已在 main.rs 模块注册时建立
                            } else {
                                // 模块未找到
                                emit_undefined_module(self.sink, target_module_name, use_path.span);
                            }
                        }
                        // 如果 registry 为 None（单文件编译），则忽略 use 语句
                    }
                }
            }
            Phase::ResolveLocals => {
                // 第二趟：处理函数体。
                match it {
                    Item::Func(f) => {
                        // 进入函数作用域。
                        self.scopes.push_scope();

                        // 定义参数。
                        for p in &f.params {
                            self.define_local_symbol(p.name.clone(), SymbolKind::Param, p.span, p.is_mut);
                        }

                        // 遍历函数体。
                        self.visit_expr(&f.body);

                        // 退出函数作用域。
                        self.scopes.pop_scope();
                    }
                    Item::Struct(_) | Item::Union(_) | Item::Use(_) => {
                        // 第二趟不再处理。
                    }
                }
            }
        }
    }

    fn visit_stmt(&mut self, s: &Stmt) {
        if self.phase != Phase::ResolveLocals {
            return;
        }

        match s {
            Stmt::Let { name, ty, init, span, is_mut } => {
                // 先遍历初始化表达式（右侧），再定义名字（左侧）。
                // 这样保证 `x := x + 1` 里右侧的 x 引用外层作用域。
                if let Some(t) = ty {
                    self.visit_type(t);
                }
                self.visit_expr(init);

                // 定义局部变量。
                self.define_local_symbol(name.clone(), SymbolKind::Local, *span, *is_mut);
            }
            _ => walk_stmt(self, s),
        }
    }

    fn visit_expr(&mut self, e: &Expr) {
        if self.phase != Phase::ResolveLocals {
            return;
        }

        match e {
            Expr::Path(segments, span) => {
                // Kore0 只支持单段路径，多段路径延迟到 stage1。
                if segments.len() == 1 {
                    let name = &segments[0];
                    if self.lookup(name).is_none() {
                        // 未定义的名字。ADR 009 错误码 E4002。
                        self.sink.emit(crate::diag::Diagnostic::error(
                            ErrorCode::UndefinedName as u16,
                            format!("未定义的名字 `{}`", name),
                            crate::diag::DiagLoc::At(*span),
                        ));
                    }
                } else {
                    // 多段路径暂不支持。
                    self.sink.emit(crate::diag::Diagnostic::error(
                        ErrorCode::UndefinedName as u16,
                        format!("多段路径 `{}` 在 Kore0 中不支持", segments.join("::")),
                        crate::diag::DiagLoc::At(*span),
                    ));
                }
            }
            Expr::Field { base, name, span } => {
                // 检查是否为限定名称（module.symbol）
                if let Expr::Path(segments, _) = base.as_ref() {
                    if segments.len() == 1 {
                        let module_name = &segments[0];
                        if let Some(sym_id) = self.lookup(module_name) {
                            if let Some(symbol) = self.table.get(sym_id) {
                                if let SymbolKind::Module(module_id) = symbol.kind {
                                    // 这是限定名称：module.symbol
                                    if let Some(registry) = self.registry.as_ref() {
                                        if let Some(exports) = registry.get_exports(module_id) {
                                            match exports.lookup(name) {
                                                None => {
                                                    self.sink.emit(crate::diag::Diagnostic::error(
                                                        ErrorCode::UndefinedSymbol as u16,
                                                        format!("模块 `{}` 中未定义符号 `{}`", module_name, name),
                                                        crate::diag::DiagLoc::At(*span),
                                                    ));
                                                }
                                                Some(sym_id) => {
                                                    // 检查可见性（pub）
                                                    if let Some(symbol) = exports.get(sym_id) {
                                                        if !symbol.is_public {
                                                            self.sink.emit(crate::diag::Diagnostic::error(
                                                                ErrorCode::PrivateSymbol as u16,
                                                                format!("符号 `{}::{}` 是私有的", module_name, name),
                                                                crate::diag::DiagLoc::At(*span),
                                                            ));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    // 已处理限定名称，不继续递归
                                    return;
                                }
                            }
                        }
                    }
                }
                // 否则是普通字段访问，继续递归
                walk_expr(self, e);
            }
            Expr::Block { stmts, .. } => {
                // 块创建新作用域。
                self.scopes.push_scope();
                for stmt in stmts {
                    self.visit_stmt(stmt);
                }
                self.scopes.pop_scope();
            }
            _ => walk_expr(self, e),
        }
    }

    fn visit_pattern(&mut self, p: &Pattern) {
        if self.phase != Phase::ResolveLocals {
            return;
        }

        match p {
            Pattern::Variant { name: _, bindings, span } => {
                // 变体名的消解延迟到类型检查。
                // 绑定的名字定义在当前作用域（跳过通配符 "_"）。
                for binding in bindings {
                    if binding != "_" {
                        self.define_local_symbol(binding.clone(), SymbolKind::Local, *span, false);
                    }
                }
            }
            Pattern::Bind(name, span) => {
                // 通配符 "_" 不定义名字，跳过
                if name != "_" {
                    self.define_local_symbol(name.clone(), SymbolKind::Local, *span, false);
                }
            }
            Pattern::Lit(_) | Pattern::Wildcard(_) | Pattern::Cond(_) => {
                // 字面量、通配符、条件不引入新名字。
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{FileId, Span};

    fn span() -> Span {
        Span::new(FileId(0), 0, 1)
    }

    #[test]
    fn module_level_func_is_registered() {
        let mut sink = DiagSink::new();
        let module = Module {
            items: vec![Item::Func(Func {
                is_public: false,
                name: "main".into(),
                params: Vec::new(),
                ret: None,
                err: None,
                body: Expr::Int("42".into(), span()),
                span: span(),
            })],
            span: span(),
        };

        let resolver = Resolver::new(&mut sink);
        let table = resolver.resolve(&module);

        // 符号表应包含 6 个内置函数 + 1 个 main 函数 = 7 个符号
        assert_eq!(table.len(), 7);

        // 查找 main 函数
        let main_sym = table.iter().find(|(_, sym)| sym.name == "main").unwrap();
        assert_eq!(main_sym.1.name, "main");
        assert_eq!(main_sym.1.kind, SymbolKind::Func);
        assert!(!sink.has_errors());
    }

    #[test]
    fn module_level_redefinition_is_error() {
        let mut sink = DiagSink::new();
        let module = Module {
            items: vec![
                Item::Func(Func {
                    is_public: false,
                    name: "foo".into(),
                    params: Vec::new(),
                    ret: None,
                    err: None,
                    body: Expr::Nil(span()),
                    span: span(),
                }),
                Item::Func(Func {
                    is_public: false,
                    name: "foo".into(),
                    params: Vec::new(),
                    ret: None,
                    err: None,
                    body: Expr::Nil(span()),
                    span: span(),
                }),
            ],
            span: span(),
        };

        let resolver = Resolver::new(&mut sink);
        let _table = resolver.resolve(&module);

        assert_eq!(sink.err_count(), 1);
        let diag = &sink.peek()[0];
        assert_eq!(diag.code, 4003);
        assert!(diag.msg.contains("重定义名字"));
    }

    #[test]
    fn func_params_are_in_func_scope() {
        let mut sink = DiagSink::new();
        let module = Module {
            items: vec![Item::Func(Func {
                is_public: false,
                name: "add".into(),
                params: vec![
                    Param {
                        name: "x".into(),
                        ty: TypeExpr::Named("i32".into(), span()),
                        is_mut: false,
                        span: span(),
                    },
                    Param {
                        name: "y".into(),
                        ty: TypeExpr::Named("i32".into(), span()),
                        is_mut: false,
                        span: span(),
                    },
                ],
                ret: None,
                err: None,
                body: Expr::Binary {
                    op: "+",
                    lhs: Box::new(Expr::Path(vec!["x".into()], span())),
                    rhs: Box::new(Expr::Path(vec!["y".into()], span())),
                    span: span(),
                },
                span: span(),
            })],
            span: span(),
        };

        let resolver = Resolver::new(&mut sink);
        let table = resolver.resolve(&module);

        // 6 内置函数 + 1 func + 2 params = 9 symbols
        assert_eq!(table.len(), 9);
        assert!(!sink.has_errors());
    }

    #[test]
    fn local_let_defines_in_scope() {
        let mut sink = DiagSink::new();
        let module = Module {
            items: vec![Item::Func(Func {
                is_public: false,
                name: "f".into(),
                params: Vec::new(),
                ret: None,
                err: None,
                body: Expr::Block {
                    stmts: vec![
                        Stmt::Let {
                            name: "x".into(),
                            is_mut: false,
                            ty: None,
                            init: Expr::Int("1".into(), span()),
                            span: span(),
                        },
                        Stmt::Expr(Expr::Path(vec!["x".into()], span())),
                    ],
                    span: span(),
                },
                span: span(),
            })],
            span: span(),
        };

        let resolver = Resolver::new(&mut sink);
        let table = resolver.resolve(&module);

        // 6 内置函数 + 1 func + 1 local = 8 symbols
        assert_eq!(table.len(), 8);
        assert!(!sink.has_errors());
    }

    #[test]
    fn undefined_name_is_error() {
        let mut sink = DiagSink::new();
        let module = Module {
            items: vec![Item::Func(Func {
                is_public: false,
                name: "f".into(),
                params: Vec::new(),
                ret: None,
                err: None,
                body: Expr::Path(vec!["undefined".into()], span()),
                span: span(),
            })],
            span: span(),
        };

        let resolver = Resolver::new(&mut sink);
        let _table = resolver.resolve(&module);

        assert_eq!(sink.err_count(), 1);
        let diag = &sink.peek()[0];
        assert_eq!(diag.code, ErrorCode::UndefinedName as u16);
    }

    #[test]
    fn nested_block_creates_scope() {
        let mut sink = DiagSink::new();
        let module = Module {
            items: vec![Item::Func(Func {
                is_public: false,
                name: "f".into(),
                params: Vec::new(),
                ret: None,
                err: None,
                body: Expr::Block {
                    stmts: vec![
                        Stmt::Let {
                            name: "x".into(),
                            is_mut: false,
                            ty: None,
                            init: Expr::Int("1".into(), span()),
                            span: span(),
                        },
                        Stmt::Expr(Expr::Block {
                            stmts: vec![
                                Stmt::Let {
                                    name: "x".into(), // 遮蔽外层 x
                                    is_mut: false,
                                    ty: None,
                                    init: Expr::Int("2".into(), span()),
                                    span: span(),
                                },
                                Stmt::Expr(Expr::Path(vec!["x".into()], span())),
                            ],
                            span: span(),
                        }),
                    ],
                    span: span(),
                },
                span: span(),
            })],
            span: span(),
        };

        let resolver = Resolver::new(&mut sink);
        let table = resolver.resolve(&module);

        // 6 内置函数 + 1 func + 2 locals (外层 x + 内层 x) = 9 symbols
        assert_eq!(table.len(), 9);
        assert!(!sink.has_errors()); // 遮蔽合法，无错误
    }

    #[test]
    fn pattern_bind_defines_name() {
        let mut sink = DiagSink::new();
        let module = Module {
            items: vec![Item::Func(Func {
                is_public: false,
                name: "f".into(),
                params: Vec::new(),
                ret: None,
                err: None,
                body: Expr::Branch {
                    scrutinee: Some(Box::new(Expr::Int("1".into(), span()))),
                    arms: vec![Arm {
                        pattern: Pattern::Bind("x".into(), span()),
                        body: Expr::Path(vec!["x".into()], span()),
                        span: span(),
                    }],
                    span: span(),
                },
                span: span(),
            })],
            span: span(),
        };

        let resolver = Resolver::new(&mut sink);
        let table = resolver.resolve(&module);

        // 6 内置函数 + 1 func + 1 pattern binding = 8 symbols
        assert_eq!(table.len(), 8);
        assert!(!sink.has_errors());
    }
}
