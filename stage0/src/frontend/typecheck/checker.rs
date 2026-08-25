//! 类型检查器核心逻辑。

use crate::diag::{DiagSink, Diagnostic, DiagLoc, ErrorCode, Span};
use crate::frontend::ast::{Expr, Item, Module, Stmt};
use crate::frontend::resolve::SymbolTable;
use super::context::TypeContext;
use super::types::Type;

/// 类型检查器。
pub struct TypeChecker<'a> {
    #[allow(dead_code)]
    symbols: &'a SymbolTable,
    type_ctx: TypeContext,
    sink: &'a mut DiagSink,
    /// 变量作用域栈。每进一个块 push 一层，离开时 pop。
    var_scopes: Vec<std::collections::HashMap<String, Type>>,
    /// 当前函数的返回类型（用于错误联合变体推断）
    current_return_type: Option<Type>,
    /// 当前分支表达式是否处于语句位置（结果被丢弃）。
    /// 语句位置的 `? x is {...}` 不要求各臂类型统一，因为没人接收其值。
    branch_in_stmt_pos: bool,
}

impl<'a> TypeChecker<'a> {
    pub fn new(symbols: &'a SymbolTable, sink: &'a mut DiagSink) -> Self {
        Self {
            symbols,
            type_ctx: TypeContext::new(),
            sink,
            var_scopes: Vec::new(),
            current_return_type: None,
            branch_in_stmt_pos: false,
        }
    }

    /// 获取类型上下文的不可变引用（用于降级阶段）。
    pub fn type_context(&self) -> &TypeContext {
        &self.type_ctx
    }

    /// 消费 TypeChecker，提取类型上下文。
    pub fn into_context(self) -> TypeContext {
        self.type_ctx
    }

    fn push_scope(&mut self) {
        self.var_scopes.push(std::collections::HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.var_scopes.pop();
    }

    fn define_var(&mut self, name: String, ty: Type) {
        if let Some(scope) = self.var_scopes.last_mut() {
            scope.insert(name, ty);
        }
    }

    fn lookup_var(&self, name: &str) -> Type {
        for scope in self.var_scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return ty.clone();
            }
        }
        Type::Unknown
    }

    /// 注册内置函数的类型签名。
    fn register_builtins(&mut self) {
        // print :: (str) void
        self.type_ctx.define_func(
            "print".to_string(),
            Type::Func {
                params: vec![Type::Str],
                ret: Box::new(Type::Void),
                err: None,
            },
        );

        // println :: (str) void
        self.type_ctx.define_func(
            "println".to_string(),
            Type::Func {
                params: vec![Type::Str],
                ret: Box::new(Type::Void),
                err: None,
            },
        );

        // eprint :: (str) void
        self.type_ctx.define_func(
            "eprint".to_string(),
            Type::Func {
                params: vec![Type::Str],
                ret: Box::new(Type::Void),
                err: None,
            },
        );

        // eprintln :: (str) void
        self.type_ctx.define_func(
            "eprintln".to_string(),
            Type::Func {
                params: vec![Type::Str],
                ret: Box::new(Type::Void),
                err: None,
            },
        );

        // read_file :: (str) str
        self.type_ctx.define_func(
            "read_file".to_string(),
            Type::Func {
                params: vec![Type::Str],
                ret: Box::new(Type::Str),
                err: None,
            },
        );

        // write_file :: (str, str) i32
        self.type_ctx.define_func(
            "write_file".to_string(),
            Type::Func {
                params: vec![Type::Str, Type::Str],
                ret: Box::new(Type::i32()),
                err: None,
            },
        );

        // exit :: (i32) void
        self.type_ctx.define_func(
            "exit".to_string(),
            Type::Func {
                params: vec![Type::i32()],
                ret: Box::new(Type::Void),
                err: None,
            },
        );
    }

    /// 检查整个模块：先注册所有类型定义，再检查每个函数体。
    pub fn check_module(&mut self, module: &Module) {
        // 注册内置函数签名到类型上下文。
        self.register_builtins();

        // Pass 1：收集结构体/联合类型定义以及函数签名。
        for item in &module.items {
            match item {
                Item::Struct(s) => {
                    let mut fields = Vec::new();
                    for f in &s.fields {
                        let ty = self.type_ctx.resolve_type_expr(&f.ty, self.sink);
                        fields.push((f.name.clone(), ty));
                    }
                    self.type_ctx.define_struct(s.name.clone(), fields);
                }
                Item::Union(u) => {
                    let mut variants = Vec::new();
                    for v in &u.variants {
                        let ty = if v.payload.is_empty() {
                            Type::Void
                        } else if v.payload.len() == 1 {
                            self.type_ctx.resolve_type_expr(&v.payload[0], self.sink)
                        } else {
                            // 多个 payload 需要元组类型（暂未实现）
                            Type::Unknown
                        };
                        variants.push((v.name.clone(), ty));
                    }
                    self.type_ctx.define_union(u.name.clone(), variants);
                }
                Item::Func(f) => {
                    let params: Vec<Type> = f.params.iter()
                        .map(|p| self.type_ctx.resolve_type_expr(&p.ty, self.sink))
                        .collect();
                    let ret = f.ret.as_ref()
                        .map(|te| self.type_ctx.resolve_type_expr(te, self.sink))
                        .unwrap_or(Type::Void);
                    let err = f.err.as_ref()
                        .map(|te| self.type_ctx.resolve_type_expr(te, self.sink));
                    let func_ty = Type::Func {
                        params,
                        ret: Box::new(ret),
                        err: err.map(Box::new),
                    };
                    self.type_ctx.define_func(f.name.clone(), func_ty);
                }
                _ => {}
            }
        }
        // Pass 2：检查函数体。
        for item in &module.items {
            if let Item::Func(f) = item {
                self.check_func(f);
            }
        }
    }

    /// 检查单个函数：把参数注入作用域，检查函数体，验证返回类型。
    pub fn check_func(&mut self, func: &crate::frontend::ast::Func) {
        self.push_scope();
        for param in &func.params {
            let ty = self.type_ctx.resolve_type_expr(&param.ty, self.sink);
            self.define_var(param.name.clone(), ty);
        }

        // 设置当前函数的返回类型（用于错误联合变体推断）
        let ret_ty = func.ret.as_ref()
            .map(|te| self.type_ctx.resolve_type_expr(te, self.sink))
            .unwrap_or(Type::Void);
        let err_ty = func.err.as_ref()
            .map(|te| self.type_ctx.resolve_type_expr(te, self.sink));

        self.current_return_type = if let Some(err) = err_ty {
            Some(Type::ErrUnion {
                ok: Box::new(ret_ty.clone()),
                err: Box::new(err),
            })
        } else {
            Some(ret_ty.clone())
        };

        let body_ty = self.check_expr(&func.body);

        // 检查完后清除返回类型
        self.current_return_type = None;

        // Never 类型（ret/stop/jmp）可以赋给任何期望类型，不报错。
        // Unknown 表示类型信息缺失，也跳过检查避免误报。
        if body_ty != Type::Never
            && body_ty != Type::Unknown
            && ret_ty != Type::Unknown
            && body_ty != ret_ty
        {
            let span = func.body.span();
            self.emit_error(
                ErrorCode::InternalCompilerError,
                format!(
                    "函数 '{}' 返回类型不匹配：声明 '{}'，实际 '{}'",
                    func.name, ret_ty, body_ty
                ),
                span,
            );
        }
        self.pop_scope();
    }

    /// 检查表达式，返回其类型。
    pub fn check_expr(&mut self, expr: &Expr) -> Type {
        // 语句位置标记只对当前表达式生效：递归前先取走，只有块尾表达式和分支臂
        // 会显式向下传递，避免 `f(? x is {...})` 这类值位置的子分支误继承。
        let stmt_pos = std::mem::take(&mut self.branch_in_stmt_pos);
        match expr {
            Expr::Int(_, _) => Type::i32(),
            Expr::Float(_, _) => Type::f64(),
            Expr::Bool(_, _) => Type::Bool,
            Expr::Str(_, _) => Type::Str,
            Expr::Nil(_) => Type::Void,

            Expr::Path(segments, _) => {
                // 查变量作用域，只取第一段（字段访问由 Expr::Field 处理）。
                // 变量作用域未找到时，回退查函数签名表（顶层函数名也是有效路径）。
                if let Some(name) = segments.first() {
                    let ty = self.lookup_var(name);
                    if ty != Type::Unknown {
                        return ty;
                    }
                    if let Some(func_ty) = self.type_ctx.get_func(name) {
                        return func_ty;
                    }
                }
                Type::Unknown
            }

            Expr::Binary { op, lhs, rhs, span } => {
                let lty = self.check_expr(lhs);
                let rty = self.check_expr(rhs);
                self.check_binop(op, &lty, &rty, *span)
            }

            Expr::Call { callee, args, .. } => {
                let func_ty = self.check_expr(callee);
                for arg in args {
                    self.check_expr(arg);
                }
                match func_ty {
                    Type::Func { ret, .. } => *ret,
                    _ => Type::Unknown,
                }
            }

            Expr::Field { base, name, span } => {
                let base_ty = self.check_expr(base);
                match base_ty {
                    Type::Struct(struct_name) => {
                        self.type_ctx.get_struct_field(&struct_name, name)
                            .unwrap_or_else(|| {
                                self.emit_error(
                                    ErrorCode::UndefinedName,
                                    format!("字段 '{}' 在结构体 '{}' 中未定义", name, struct_name),
                                    *span,
                                );
                                Type::Unknown
                            })
                    }
                    _ => {
                        self.emit_error(
                            ErrorCode::InternalCompilerError,
                            format!("类型 '{}' 没有字段", base_ty),
                            *span,
                        );
                        Type::Unknown
                    }
                }
            }

            Expr::Unary { op, operand, span } => {
                let ty = self.check_expr(operand);
                match *op {
                    "-" => {
                        if !ty.is_numeric() && ty != Type::Unknown {
                            self.emit_error(
                                ErrorCode::InternalCompilerError,
                                format!("一元负号不支持类型 '{}'", ty),
                                *span,
                            );
                            return Type::Unknown;
                        }
                        ty
                    }
                    "!" => {
                        if ty != Type::Bool && ty != Type::Unknown {
                            self.emit_error(
                                ErrorCode::InternalCompilerError,
                                format!("逻辑非要求布尔类型，得到 '{}'", ty),
                                *span,
                            );
                        }
                        Type::Bool
                    }
                    _ => {
                        self.emit_error(
                            ErrorCode::InternalCompilerError,
                            format!("未知一元运算符 '{}'", op),
                            *span,
                        );
                        Type::Unknown
                    }
                }
            }

            Expr::Deref(inner, span) => {
                let ty = self.check_expr(inner);
                match ty {
                    Type::Borrow(inner_ty) | Type::Own(inner_ty) => *inner_ty,
                    Type::Unknown => Type::Unknown,
                    _ => {
                        self.emit_error(
                            ErrorCode::InternalCompilerError,
                            format!("无法解引用类型 '{}'", ty),
                            *span,
                        );
                        Type::Unknown
                    }
                }
            }

            Expr::Index { base, index, span } => {
                let base_ty = self.check_expr(base);
                let _idx_ty = self.check_expr(index);
                match base_ty {
                    Type::Array { elem, .. } => *elem,
                    Type::Slice { elem } => *elem,
                    Type::Unknown => Type::Unknown,
                    _ => {
                        self.emit_error(
                            ErrorCode::InternalCompilerError,
                            format!("类型 '{}' 不支持下标访问", base_ty),
                            *span,
                        );
                        Type::Unknown
                    }
                }
            }

            Expr::Propagate(inner, span) => {
                let ty = self.check_expr(inner);
                match ty {
                    Type::ErrUnion { ok, .. } => *ok,
                    Type::Unknown => Type::Unknown,
                    _ => {
                        self.emit_error(
                            ErrorCode::InternalCompilerError,
                            format!("'!' 传播要求错误联合类型，得到 '{}'", ty),
                            *span,
                        );
                        Type::Unknown
                    }
                }
            }

            Expr::Block { stmts, .. } => {
                // 检查所有语句，块的类型是最后一个表达式语句的类型，否则为 void。
                self.push_scope();
                let mut last_ty = Type::Void;
                for (i, stmt) in stmts.iter().enumerate() {
                    if i + 1 == stmts.len() && let Stmt::Expr(e) = stmt {
                        // 块尾表达式继承块自身的位置：块被丢弃则尾表达式也被丢弃。
                        self.branch_in_stmt_pos = stmt_pos;
                        last_ty = self.check_expr(e);
                        continue;
                    }
                    self.check_stmt(stmt);
                }
                self.pop_scope();
                last_ty
            }

            Expr::Branch { scrutinee, arms, span: _ } => {
                if let Some(s) = scrutinee {
                    self.check_expr(s);
                }
                let mut unified: Option<Type> = None;
                // 记录首个 void 臂与首个非 void 臂，用于诊断 void/非 void 混用。
                let mut void_arm: Option<crate::diag::Span> = None;
                let mut value_arm: Option<(crate::diag::Span, Type)> = None;
                for arm in arms {
                    self.push_scope();
                    self.inject_pattern_bindings(&arm.pattern);
                    // 臂体继承分支自身的位置：分支被丢弃则臂体的值也被丢弃。
                    self.branch_in_stmt_pos = stmt_pos;
                    let arm_ty = self.check_expr(&arm.body);
                    self.pop_scope();
                    // Never 不参与类型统一（控制流分歧）。
                    if arm_ty == Type::Never || arm_ty == Type::Unknown {
                        continue;
                    }
                    if arm_ty == Type::Void {
                        if void_arm.is_none() {
                            void_arm = Some(arm.span);
                        }
                    } else if value_arm.is_none() {
                        value_arm = Some((arm.span, arm_ty.clone()));
                    }
                    match &unified {
                        None => unified = Some(arm_ty),
                        Some(prev) if *prev != arm_ty => {
                            // 多个臂类型不一致，退化为 Unknown。
                            unified = Some(Type::Unknown);
                        }
                        _ => {}
                    }
                }
                // void 臂与产值臂混用时，降级层只能按首个臂的类型分配 phi temp，
                // 另一类臂的值会被静默丢弃。必须在此报错而非放行到 codegen。
                // 语句位置的分支结果本就被丢弃，混用臂是合法的。
                if !stmt_pos
                    && let (Some(_), Some((value_span, value_ty))) = (void_arm, &value_arm)
                {
                    self.emit_error(
                        ErrorCode::TypeMismatch,
                        format!(
                            "分支各臂类型不一致：部分臂产出 '{}'，部分臂产出 'void'。\
                             请让所有臂产出同一类型，或都不产出值",
                            value_ty
                        ),
                        *value_span,
                    );
                    return Type::Unknown;
                }
                unified.unwrap_or(Type::Unknown)
            }

            Expr::Loop { subject, body, .. } => {
                if let Some(s) = subject {
                    self.check_expr(s);
                }
                self.check_expr(body);
                // 循环本身类型为 void（`stop` 带值时由调用方处理）。
                Type::Void
            }

            // 控制流表达式：类型为 never（不会产出值）。
            Expr::Ret(expr, _) => {
                if let Some(e) = expr {
                    self.check_expr(e);
                }
                Type::Never
            }
            Expr::Stop { .. } => Type::Never,

            Expr::Skip { .. } => Type::Never,

            Expr::Jmp { target, .. } => {
                if let Some(t) = target {
                    self.check_expr(t);
                }
                Type::Never
            }

            Expr::StructLit { name, fields, span } => {
                // 查找结构体定义，检查字段类型。
                if let Some(_struct_ty) = self.type_ctx.get_struct(name) {
                    for (field_name, field_expr) in fields {
                        let field_ty = self.check_expr(field_expr);
                        // 验证字段类型是否匹配结构体定义
                        if let Some(expected_ty) = self.type_ctx.get_struct_field(name, field_name) {
                            // Unknown 类型表示类型信息缺失，跳过检查避免误报
                            if field_ty != Type::Unknown
                                && expected_ty != Type::Unknown
                                && field_ty != expected_ty
                            {
                                self.emit_error(
                                    ErrorCode::TypeMismatch,
                                    format!(
                                        "结构体 '{}' 的字段 '{}' 类型不匹配：期望 '{}'，得到 '{}'",
                                        name, field_name, expected_ty, field_ty
                                    ),
                                    *span,
                                );
                            }
                        } else {
                            self.emit_error(
                                ErrorCode::UndefinedName,
                                format!("字段 '{}' 在结构体 '{}' 中未定义", field_name, name),
                                *span,
                            );
                        }
                    }
                    Type::Struct(name.clone())
                } else {
                    self.emit_error(
                        ErrorCode::UndefinedName,
                        format!("未定义的结构体 '{}'", name),
                        *span,
                    );
                    Type::Unknown
                }
            }

            Expr::ArrayLit { elements, span } => {
                if elements.is_empty() {
                    // 空数组，类型未知
                    return Type::Unknown;
                }
                let first_ty = self.check_expr(&elements[0]);
                for elem in &elements[1..] {
                    let elem_ty = self.check_expr(elem);
                    if elem_ty != first_ty && elem_ty != Type::Unknown && first_ty != Type::Unknown {
                        self.emit_error(
                            ErrorCode::InternalCompilerError,
                            format!("数组元素类型不一致: 期望 '{}', 得到 '{}'", first_ty, elem_ty),
                            *span,
                        );
                    }
                }
                Type::Array {
                    elem: Box::new(first_ty),
                    len: elements.len() as u64,
                }
            }

            Expr::VariantConstructor { name, payload, span } => {
                // 检查 payload 表达式类型
                let payload_ty = payload.as_ref().map(|p| self.check_expr(p));

                // 从类型上下文查找变体所属联合类型
                if let Some((union_name, expected_payload_ty)) = self.type_ctx.find_variant_union(name) {
                    // 验证 payload 类型与变体定义匹配
                    if let Some(actual_ty) = payload_ty {
                        if actual_ty != expected_payload_ty && actual_ty != Type::Unknown {
                            self.emit_error(
                                ErrorCode::InternalCompilerError,
                                format!(
                                    "变体 '.{}' payload 类型不匹配：期望 '{}'，实际 '{}'",
                                    name, expected_payload_ty, actual_ty
                                ),
                                *span,
                            );
                        }
                    } else if expected_payload_ty != Type::Void {
                        self.emit_error(
                            ErrorCode::InternalCompilerError,
                            format!("变体 '.{}' 需要 payload，类型为 '{}'", name, expected_payload_ty),
                            *span,
                        );
                    }
                    Type::Union(union_name)
                } else if name == "Ok" || name == "Err" {
                    // 特殊处理错误联合的 .Ok 和 .Err 变体
                    if let Some(Type::ErrUnion { ok, err }) = &self.current_return_type {
                        let expected_ty = if name == "Ok" { &**ok } else { &**err };

                        // 验证 payload 类型
                        if let Some(actual_ty) = payload_ty {
                            if actual_ty != *expected_ty && actual_ty != Type::Unknown {
                                self.emit_error(
                                    ErrorCode::InternalCompilerError,
                                    format!(
                                        "变体 '.{}' payload 类型不匹配：期望 '{}'，实际 '{}'",
                                        name, expected_ty, actual_ty
                                    ),
                                    *span,
                                );
                            }
                        } else if *expected_ty != Type::Void {
                            self.emit_error(
                                ErrorCode::InternalCompilerError,
                                format!("变体 '.{}' 需要 payload，类型为 '{}'", name, expected_ty),
                                *span,
                            );
                        }

                        self.current_return_type.clone().unwrap()
                    } else {
                        self.emit_error(
                            ErrorCode::InternalCompilerError,
                            format!("变体 '.{}' 只能在返回错误联合类型的函数中使用", name),
                            *span,
                        );
                        Type::Unknown
                    }
                } else {
                    self.emit_error(
                        ErrorCode::InternalCompilerError,
                        format!("未找到变体 '.{}'", name),
                        *span,
                    );
                    Type::Unknown
                }
            }
        }
    }

    /// 检查二元运算的类型。
    fn check_binop(&mut self, op: &str, lty: &Type, rty: &Type, span: Span) -> Type {
        match op {
            "+" | "-" | "*" | "/" => {
                if lty != rty {
                    self.emit_error(
                        ErrorCode::InternalCompilerError,
                        format!("类型不匹配：'{}' 和 '{}'", lty, rty),
                        span,
                    );
                    return Type::Unknown;
                }
                if !lty.is_numeric() {
                    self.emit_error(
                        ErrorCode::InternalCompilerError,
                        format!("运算符 '{}' 不支持类型 '{}'", op, lty),
                        span,
                    );
                    return Type::Unknown;
                }
                lty.clone()
            }

            "==" | "!=" | "<" | "<=" | ">" | ">=" => {
                if lty != rty {
                    self.emit_error(
                        ErrorCode::InternalCompilerError,
                        format!("类型不匹配：'{}' 和 '{}'", lty, rty),
                        span,
                    );
                }
                Type::Bool
            }

            "&&" | "||" => {
                if *lty != Type::Bool || *rty != Type::Bool {
                    self.emit_error(
                        ErrorCode::InternalCompilerError,
                        format!("逻辑运算符要求布尔类型，得到 '{}' 和 '{}'", lty, rty),
                        span,
                    );
                }
                Type::Bool
            }

            "&" | "|" | "^" | "<<" | ">>" => {
                if lty != rty && *lty != Type::Unknown && *rty != Type::Unknown {
                    self.emit_error(
                        ErrorCode::InternalCompilerError,
                        format!("位操作类型不匹配：'{}' 和 '{}'", lty, rty),
                        span,
                    );
                    return Type::Unknown;
                }
                if !lty.is_numeric() && *lty != Type::Unknown {
                    self.emit_error(
                        ErrorCode::InternalCompilerError,
                        format!("位操作符 '{}' 要求整数类型，得到 '{}'", op, lty),
                        span,
                    );
                    return Type::Unknown;
                }
                lty.clone()
            }

            _ => Type::Unknown,
        }
    }

    /// 检查语句。
    pub fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, ty, init, .. } => {
                let init_ty = self.check_expr(init);
                // 如果有显式类型标注，解析并与初始值类型对比。
                let resolved_ty = if let Some(type_expr) = ty {
                    let ann_ty = self.type_ctx.resolve_type_expr(type_expr, self.sink);
                    if !self.is_assignable(&init_ty, &ann_ty) {
                        self.emit_error(
                            ErrorCode::InternalCompilerError,
                            format!("类型不匹配：标注为 '{}'，初始值类型为 '{}'", ann_ty, init_ty),
                            init.span(),
                        );
                    }
                    ann_ty
                } else {
                    init_ty
                };
                self.define_var(name.clone(), resolved_ty);
            }

            Stmt::Assign { target, value, span } => {
                let target_ty = self.check_expr(target);
                let value_ty = self.check_expr(value);
                if !self.is_assignable(&value_ty, &target_ty) {
                    self.emit_error(
                        ErrorCode::InternalCompilerError,
                        format!("赋值类型不匹配：'{}' 和 '{}'", target_ty, value_ty),
                        *span,
                    );
                }
            }

            // 语句位置：表达式的值被丢弃，分支各臂不要求类型统一。
            Stmt::Expr(expr) => {
                self.branch_in_stmt_pos = true;
                self.check_expr(expr);
                self.branch_in_stmt_pos = false;
            }

            Stmt::Defer(expr, _) => {
                self.branch_in_stmt_pos = true;
                self.check_expr(expr);
                self.branch_in_stmt_pos = false;
            }
        }
    }

    /// 将模式中绑定的变量注入当前作用域（类型暂为 Unknown）。
    fn inject_pattern_bindings(&mut self, pattern: &crate::frontend::ast::Pattern) {
        use crate::frontend::ast::Pattern;
        match pattern {
            Pattern::Bind(name, _) => {
                self.define_var(name.clone(), Type::Unknown);
            }
            Pattern::Variant { bindings, .. } => {
                for name in bindings {
                    self.define_var(name.clone(), Type::Unknown);
                }
            }
            _ => {}
        }
    }

    /// 检查 `from` 类型是否可以隐式转换为 `to` 类型。
    fn is_assignable(&self, from: &Type, to: &Type) -> bool {
        // 完全相同
        if from == to {
            return true;
        }
        // Unknown 兼容任何类型
        if *from == Type::Unknown || *to == Type::Unknown {
            return true;
        }
        // 数组到切片的隐式转换：[N]T -> []T
        if let (Type::Array { elem: from_elem, .. }, Type::Slice { elem: to_elem }) = (from, to) {
            return from_elem == to_elem;
        }
        false
    }

    fn emit_error(&mut self, code: ErrorCode, msg: String, span: Span) {
        self.sink.emit(Diagnostic::error(
            code.as_u16(),
            msg,
            DiagLoc::At(span),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::FileId;
    use crate::frontend::ast::Expr;

    fn dummy_span() -> Span {
        Span::new(FileId(0), 0, 0)
    }

    #[test]
    fn check_int_literal() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);

        let expr = Expr::Int("42".to_string(), dummy_span());

        assert_eq!(checker.check_expr(&expr), Type::i32());
    }

    #[test]
    fn check_float_literal() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);

        let expr = Expr::Float("3.14".to_string(), dummy_span());

        assert_eq!(checker.check_expr(&expr), Type::f64());
    }

    #[test]
    fn check_bool_literal() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);

        let expr = Expr::Bool(true, dummy_span());

        assert_eq!(checker.check_expr(&expr), Type::Bool);
    }

    #[test]
    fn check_binop_add_i32() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);

        let expr = Expr::Binary {
            op: "+",
            lhs: Box::new(Expr::Int("1".to_string(), dummy_span())),
            rhs: Box::new(Expr::Int("2".to_string(), dummy_span())),
            span: dummy_span(),
        };

        assert_eq!(checker.check_expr(&expr), Type::i32());
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn check_binop_type_mismatch() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);

        let expr = Expr::Binary {
            op: "+",
            lhs: Box::new(Expr::Int("1".to_string(), dummy_span())),
            rhs: Box::new(Expr::Float("2.0".to_string(), dummy_span())),
            span: dummy_span(),
        };

        checker.check_expr(&expr);
        assert!(sink.err_count() > 0);
    }

    #[test]
    fn check_comparison_returns_bool() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);

        let expr = Expr::Binary {
            op: "<",
            lhs: Box::new(Expr::Int("1".to_string(), dummy_span())),
            rhs: Box::new(Expr::Int("2".to_string(), dummy_span())),
            span: dummy_span(),
        };

        assert_eq!(checker.check_expr(&expr), Type::Bool);
    }

    #[test]
    fn check_logical_op_requires_bool() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);

        let expr = Expr::Binary {
            op: "&&",
            lhs: Box::new(Expr::Int("1".to_string(), dummy_span())),
            rhs: Box::new(Expr::Bool(true, dummy_span())),
            span: dummy_span(),
        };

        checker.check_expr(&expr);
        assert!(sink.err_count() > 0);
    }

    // ---- 新增 case 的测试 ----

    #[test]
    fn unary_neg_i32_returns_i32() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Unary {
            op: "-",
            operand: Box::new(Expr::Int("1".to_string(), dummy_span())),
            span: dummy_span(),
        };
        assert_eq!(checker.check_expr(&expr), Type::i32());
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn unary_neg_bool_emits_error() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Unary {
            op: "-",
            operand: Box::new(Expr::Bool(true, dummy_span())),
            span: dummy_span(),
        };
        checker.check_expr(&expr);
        assert!(sink.err_count() > 0);
    }

    #[test]
    fn unary_not_bool_returns_bool() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Unary {
            op: "!",
            operand: Box::new(Expr::Bool(false, dummy_span())),
            span: dummy_span(),
        };
        assert_eq!(checker.check_expr(&expr), Type::Bool);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn unary_not_i32_emits_error() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Unary {
            op: "!",
            operand: Box::new(Expr::Int("1".to_string(), dummy_span())),
            span: dummy_span(),
        };
        checker.check_expr(&expr);
        assert!(sink.err_count() > 0);
    }

    #[test]
    fn deref_unknown_returns_unknown_no_error() {
        // Path 返回 Unknown，解引用 Unknown 不应报错（类型信息缺失）
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Deref(
            Box::new(Expr::Path(vec!["x".to_string()], dummy_span())),
            dummy_span(),
        );
        assert_eq!(checker.check_expr(&expr), Type::Unknown);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn deref_non_pointer_emits_error() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        // i32 不是指针，解引用应报错
        let expr = Expr::Deref(
            Box::new(Expr::Int("1".to_string(), dummy_span())),
            dummy_span(),
        );
        checker.check_expr(&expr);
        assert!(sink.err_count() > 0);
    }

    #[test]
    fn index_unknown_returns_unknown_no_error() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Index {
            base: Box::new(Expr::Path(vec!["arr".to_string()], dummy_span())),
            index: Box::new(Expr::Int("0".to_string(), dummy_span())),
            span: dummy_span(),
        };
        assert_eq!(checker.check_expr(&expr), Type::Unknown);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn index_non_array_emits_error() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Index {
            base: Box::new(Expr::Int("1".to_string(), dummy_span())),
            index: Box::new(Expr::Int("0".to_string(), dummy_span())),
            span: dummy_span(),
        };
        checker.check_expr(&expr);
        assert!(sink.err_count() > 0);
    }

    #[test]
    fn propagate_unknown_returns_unknown_no_error() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Propagate(
            Box::new(Expr::Path(vec!["r".to_string()], dummy_span())),
            dummy_span(),
        );
        assert_eq!(checker.check_expr(&expr), Type::Unknown);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn propagate_non_err_union_emits_error() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Propagate(
            Box::new(Expr::Int("1".to_string(), dummy_span())),
            dummy_span(),
        );
        checker.check_expr(&expr);
        assert!(sink.err_count() > 0);
    }

    #[test]
    fn block_empty_returns_void() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Block { stmts: vec![], span: dummy_span() };
        assert_eq!(checker.check_expr(&expr), Type::Void);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn block_last_expr_stmt_returns_its_type() {
        use crate::frontend::ast::Stmt;
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Block {
            stmts: vec![Stmt::Expr(Expr::Int("42".to_string(), dummy_span()))],
            span: dummy_span(),
        };
        assert_eq!(checker.check_expr(&expr), Type::i32());
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn ret_without_value_returns_never() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Ret(None, dummy_span());
        assert_eq!(checker.check_expr(&expr), Type::Never);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn ret_with_value_returns_never() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Ret(
            Some(Box::new(Expr::Int("1".to_string(), dummy_span()))),
            dummy_span(),
        );
        assert_eq!(checker.check_expr(&expr), Type::Never);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn stop_returns_never() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Stop { label: None, span: dummy_span() };
        assert_eq!(checker.check_expr(&expr), Type::Never);
    }

    #[test]
    fn skip_returns_never() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Skip { label: None, span: dummy_span() };
        assert_eq!(checker.check_expr(&expr), Type::Never);
    }

    #[test]
    fn jmp_returns_never() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let expr = Expr::Jmp {
            target: Some(Box::new(Expr::Path(vec!["label".to_string()], dummy_span()))),
            label: None,
            span: dummy_span(),
        };
        assert_eq!(checker.check_expr(&expr), Type::Never);
    }

    #[test]
    fn loop_returns_void() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);
        let body = Expr::Block { stmts: vec![], span: dummy_span() };
        let expr = Expr::Loop {
            subject: None,
            body: Box::new(body),
            label: None,
            span: dummy_span(),
        };
        assert_eq!(checker.check_expr(&expr), Type::Void);
        assert_eq!(sink.err_count(), 0);
    }

    #[test]
    fn struct_field_type_mismatch_emits_error() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);

        // 定义结构体: Point { x: i32, y: i32 }
        checker.type_ctx.define_struct(
            "Point".to_string(),
            vec![
                ("x".to_string(), Type::i32()),
                ("y".to_string(), Type::i32()),
            ],
        );

        // 构造 Point { x: 42, y: true } - y 类型错误
        let fields = vec![
            ("x".to_string(), Expr::Int("42".to_string(), dummy_span())),
            ("y".to_string(), Expr::Bool(true, dummy_span())),
        ];
        let expr = Expr::StructLit {
            name: "Point".to_string(),
            fields,
            span: dummy_span(),
        };

        let ty = checker.check_expr(&expr);
        assert_eq!(ty, Type::Struct("Point".to_string()));
        assert_eq!(sink.err_count(), 1); // 期望一个类型不匹配错误

        let diags = sink.finish();
        assert_eq!(diags[0].code, ErrorCode::TypeMismatch.as_u16());
        assert!(diags[0].msg.contains("字段 'y' 类型不匹配"));
    }

    #[test]
    fn struct_field_type_correct_no_error() {
        let symbols = SymbolTable::new();
        let mut sink = DiagSink::new();
        let mut checker = TypeChecker::new(&symbols, &mut sink);

        // 定义结构体: Point { x: i32, y: i32 }
        checker.type_ctx.define_struct(
            "Point".to_string(),
            vec![
                ("x".to_string(), Type::i32()),
                ("y".to_string(), Type::i32()),
            ],
        );

        // 构造 Point { x: 42, y: 10 } - 类型正确
        let fields = vec![
            ("x".to_string(), Expr::Int("42".to_string(), dummy_span())),
            ("y".to_string(), Expr::Int("10".to_string(), dummy_span())),
        ];
        let expr = Expr::StructLit {
            name: "Point".to_string(),
            fields,
            span: dummy_span(),
        };

        let ty = checker.check_expr(&expr);
        assert_eq!(ty, Type::Struct("Point".to_string()));
        assert_eq!(sink.err_count(), 0); // 无错误
    }
}
