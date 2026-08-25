//! 编译期求值器：AST 解释器，执行编译期绑定与常量表达式。

use crate::diag::{Diagnostic, DiagLoc, DiagSink, Span};
use crate::frontend::ast::{Expr, Item, Module, Pattern, Stmt};
use super::env::EvalEnv;
use super::value::Value;

/// 编译期求值步数上限。防止编译期无限循环。
const EVAL_STEP_LIMIT: usize = 1_000_000;

/// 编译期求值器。
pub struct Evaluator<'d> {
    env: EvalEnv,
    sink: &'d mut DiagSink,
    step_count: usize,
}

impl<'d> Evaluator<'d> {
    pub fn new(sink: &'d mut DiagSink) -> Self {
        let mut env = EvalEnv::new();
        // 预定义布尔常量
        env.define("true".to_string(), Value::Bool(true));
        env.define("false".to_string(), Value::Bool(false));
        Self {
            env,
            sink,
            step_count: 0,
        }
    }

    /// 求值模块中的所有编译期绑定（`::` 绑定）。
    pub fn eval_module(&mut self, module: &Module) {
        for item in &module.items {
            self.eval_item(item);
        }
    }

    /// 返回当前编译期求值步数（用于统计报告）。
    pub fn step_count(&self) -> usize {
        self.step_count
    }

    /// 求值顶层项。
    fn eval_item(&mut self, item: &Item) {
        match item {
            Item::Func(func) => {
                // 函数定义是编译期绑定，记录函数名和位置
                self.env.define(
                    func.name.clone(),
                    Value::Func {
                        name: func.name.clone(),
                        span: func.span,
                    },
                );
            }
            Item::Struct(def) => {
                // 结构体定义是类型，暂时不实现类型值
                self.env.define(
                    def.name.clone(),
                    Value::Type(Box::new(crate::frontend::ast::TypeExpr::Named(
                        def.name.clone(),
                        def.span,
                    ))),
                );
            }
            Item::Union(def) => {
                self.env.define(
                    def.name.clone(),
                    Value::Type(Box::new(crate::frontend::ast::TypeExpr::Named(
                        def.name.clone(),
                        def.span,
                    ))),
                );
            }
            Item::Use(_) => {
                // use 语句不产生编译期值
            }
        }
    }

    /// 求值表达式。
    pub fn eval_expr(&mut self, expr: &Expr) -> Value {
        // 递增步数计数器，检查是否超限
        self.step_count += 1;
        if self.step_count > EVAL_STEP_LIMIT {
            self.sink.emit(Diagnostic::error(
                6002,
                format!("编译期求值步数超过上限 {}", EVAL_STEP_LIMIT),
                DiagLoc::At(expr.span()),
            ));
            return Value::Error;
        }

        match expr {
            Expr::Int(s, _) => {
                s.parse::<i64>().map(Value::Int).unwrap_or_else(|_| {
                    self.error(expr.span(), format!("整数字面量解析失败: {}", s));
                    Value::Error
                })
            }
            Expr::Float(s, _) => {
                s.parse::<f64>().map(Value::Float).unwrap_or_else(|_| {
                    self.error(expr.span(), format!("浮点字面量解析失败: {}", s));
                    Value::Error
                })
            }
            Expr::Bool(b, _) => Value::Bool(*b),
            Expr::Str(s, _) => Value::Str(s.clone()),

            Expr::Path(segments, span) => {
                if segments.len() == 1 {
                    let name = &segments[0];
                    if let Some(val) = self.env.lookup(name) {
                        val.clone()
                    } else {
                        self.error(*span, format!("未定义的编译期名字: {}", name));
                        Value::Error
                    }
                } else {
                    self.error(*span, "编译期求值暂不支持多段路径".to_string());
                    Value::Error
                }
            }

            Expr::Binary { op, lhs, rhs, span } => {
                let l = self.eval_expr(lhs);
                let r = self.eval_expr(rhs);
                self.eval_binary(op, l, r, *span)
            }

            Expr::Unary { op, operand, span } => {
                let val = self.eval_expr(operand);
                self.eval_unary(op, val, *span)
            }

            Expr::Block { stmts, span } => {
                self.env.push_scope();
                let result = self.eval_block(stmts, *span);
                self.env.pop_scope();
                result
            }

            Expr::Branch { scrutinee, arms, span } => {
                // 如果有 scrutinee，先求值
                let _scrut_val = if let Some(s) = scrutinee {
                    Some(self.eval_expr(s))
                } else {
                    None
                };

                for arm in arms {
                    match &arm.pattern {
                        Pattern::Lit(lit_expr) => {
                            // 字面量模式：求值字面量，检查是否为 truthy
                            let val = self.eval_expr(lit_expr);
                            if val.is_truthy() {
                                return self.eval_expr(&arm.body);
                            }
                        }
                        Pattern::Cond(cond) => {
                            let val = self.eval_expr(cond);
                            if val.is_truthy() {
                                return self.eval_expr(&arm.body);
                            }
                        }
                        Pattern::Wildcard(_) => {
                            return self.eval_expr(&arm.body);
                        }
                        Pattern::Bind(name, _) if name.as_str() == "_" => {
                            // _ 绑定被视为通配符（不关心的值）
                            return self.eval_expr(&arm.body);
                        }
                        _ => {
                            self.error(*span, "编译期求值暂不支持复杂模式匹配".to_string());
                            return Value::Error;
                        }
                    }
                }
                // 无匹配分支，返回 unit
                Value::Unit
            }

            _ => {
                self.error(expr.span(), "不支持在编译期求值此表达式".to_string());
                Value::Error
            }
        }
    }

    /// 求值二元运算。
    fn eval_binary(&mut self, op: &'static str, lhs: Value, rhs: Value, span: Span) -> Value {
        match (op, &lhs, &rhs) {
            // 算术运算
            ("+", Value::Int(a), Value::Int(b)) => Value::Int(a + b),
            ("-", Value::Int(a), Value::Int(b)) => Value::Int(a - b),
            ("*", Value::Int(a), Value::Int(b)) => Value::Int(a * b),
            ("/", Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    self.error(span, "编译期除零错误".to_string());
                    Value::Error
                } else {
                    Value::Int(a / b)
                }
            }
            ("%", Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    self.error(span, "编译期取模零错误".to_string());
                    Value::Error
                } else {
                    Value::Int(a % b)
                }
            }

            // 比较运算
            ("==", Value::Int(a), Value::Int(b)) => Value::Bool(a == b),
            ("!=", Value::Int(a), Value::Int(b)) => Value::Bool(a != b),
            ("<", Value::Int(a), Value::Int(b)) => Value::Bool(a < b),
            ("<=", Value::Int(a), Value::Int(b)) => Value::Bool(a <= b),
            (">", Value::Int(a), Value::Int(b)) => Value::Bool(a > b),
            (">=", Value::Int(a), Value::Int(b)) => Value::Bool(a >= b),

            // 逻辑运算
            ("and", Value::Bool(a), Value::Bool(b)) => Value::Bool(*a && *b),
            ("or", Value::Bool(a), Value::Bool(b)) => Value::Bool(*a || *b),
            ("&&", Value::Bool(a), Value::Bool(b)) => Value::Bool(*a && *b),
            ("||", Value::Bool(a), Value::Bool(b)) => Value::Bool(*a || *b),

            _ => {
                self.error(
                    span,
                    format!(
                        "类型不匹配：运算符 {} 不支持类型 {} 和 {}",
                        op,
                        lhs.type_name(),
                        rhs.type_name()
                    ),
                );
                Value::Error
            }
        }
    }

    /// 求值一元运算。
    fn eval_unary(&mut self, op: &'static str, operand: Value, span: Span) -> Value {
        match (op, &operand) {
            ("-", Value::Int(n)) => Value::Int(-n),
            ("!", Value::Bool(b)) => Value::Bool(!b),
            ("not", Value::Bool(b)) => Value::Bool(!b),
            _ => {
                self.error(
                    span,
                    format!("类型不匹配：运算符 {} 不支持类型 {}", op, operand.type_name()),
                );
                Value::Error
            }
        }
    }

    /// 求值块。
    fn eval_block(&mut self, stmts: &[Stmt], _span: Span) -> Value {
        let mut last_val = Value::Unit;

        for stmt in stmts {
            match stmt {
                Stmt::Expr(expr) => {
                    last_val = self.eval_expr(expr);
                }
                Stmt::Let { name, is_mut: _, ty: _, init, span: _ } => {
                    let val = self.eval_expr(init);
                    self.env.define(name.clone(), val);
                    last_val = Value::Unit;
                }
                Stmt::Assign { target, value, span: assign_span } => {
                    let val = self.eval_expr(value);
                    if let Expr::Path(segments, _) = target {
                        if segments.len() == 1 {
                            let name = &segments[0];
                            if !self.env.update(name, val) {
                                self.error(*assign_span, format!("未定义的名字: {}", name));
                            }
                        } else {
                            self.error(*assign_span, "赋值目标必须是简单标识符".to_string());
                        }
                    } else {
                        self.error(*assign_span, "赋值目标必须是标识符".to_string());
                    }
                    last_val = Value::Unit;
                }
                Stmt::Defer(_, span) => {
                    self.error(*span, "编译期求值不支持 defer".to_string());
                    last_val = Value::Error;
                }
            }
        }

        last_val
    }

    /// 报告编译期错误。
    fn error(&mut self, span: Span, msg: String) {
        self.sink.emit(Diagnostic::error(
            6001, // E6001: 编译期求值错误
            msg,
            DiagLoc::At(span),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::FileId;

    fn eval(source: &str) -> (Value, Vec<String>) {
        let mut sink = DiagSink::new();
        let file = FileId(0);

        let tokens = crate::frontend::lexer::tokenize(file, source, &mut sink);
        let module = crate::frontend::parser::parse(file, tokens, &mut sink);

        let val = {
            let mut evaluator = Evaluator::new(&mut sink);
            evaluator.eval_module(&module);

            let func = module.items.iter().find_map(|item| {
                if let Item::Func(func) = item {
                    if func.name == "f" {
                        return Some(func);
                    }
                }
                None
            });

            if let Some(func) = func {
                evaluator.eval_expr(&func.body)
            } else {
                Value::Unit
            }
        };

        let diags = sink.finish();
        let msgs: Vec<String> = diags.iter().map(|d| d.msg.clone()).collect();

        (val, msgs)
    }

    #[test]
    fn eval_integer_literal() {
        let (val, _) = eval("f :: () i32 => { 42 }");
        assert_eq!(val, Value::Int(42));
    }

    #[test]
    fn eval_binary_add() {
        let (val, _) = eval("f :: () i32 => { 10 + 32 }");
        assert_eq!(val, Value::Int(42));
    }

    #[test]
    fn eval_binary_mul() {
        let (val, _) = eval("f :: () i32 => { 6 * 7 }");
        assert_eq!(val, Value::Int(42));
    }

    #[test]
    fn eval_comparison() {
        let (val, _) = eval("f :: () bool => { 10 < 20 }");
        assert_eq!(val, Value::Bool(true));
    }

    #[test]
    fn eval_subtraction() {
        let (val, _) = eval("f :: () i32 => { 50 - 8 }");
        assert_eq!(val, Value::Int(42));
    }

    #[test]
    fn eval_division() {
        let (val, _) = eval("f :: () i32 => { 84 / 2 }");
        assert_eq!(val, Value::Int(42));
    }

    #[test]
    fn eval_modulo() {
        let (val, _) = eval("f :: () i32 => { 100 % 58 }");
        assert_eq!(val, Value::Int(42));
    }

    #[test]
    fn eval_divide_by_zero_errors() {
        let (val, msgs) = eval("f :: () i32 => { 10 / 0 }");
        assert_eq!(val, Value::Error);
        assert!(msgs.iter().any(|m| m.contains("除零")));
    }

    #[test]
    fn eval_modulo_by_zero_errors() {
        let (val, msgs) = eval("f :: () i32 => { 10 % 0 }");
        assert_eq!(val, Value::Error);
        assert!(msgs.iter().any(|m| m.contains("取模零")));
    }

    #[test]
    fn eval_all_comparisons() {
        let (v1, _) = eval("f :: () bool => { 10 == 10 }");
        assert_eq!(v1, Value::Bool(true));

        let (v2, _) = eval("f :: () bool => { 10 != 5 }");
        assert_eq!(v2, Value::Bool(true));

        let (v3, _) = eval("f :: () bool => { 5 <= 10 }");
        assert_eq!(v3, Value::Bool(true));

        let (v4, _) = eval("f :: () bool => { 10 > 5 }");
        assert_eq!(v4, Value::Bool(true));

        let (v5, _) = eval("f :: () bool => { 10 >= 10 }");
        assert_eq!(v5, Value::Bool(true));
    }

    #[test]
    fn eval_logical_operators() {
        let (v1, _) = eval("f :: () bool => { true and false }");
        assert_eq!(v1, Value::Bool(false));

        let (v2, _) = eval("f :: () bool => { true or false }");
        assert_eq!(v2, Value::Bool(true));
    }

    #[test]
    fn eval_unary_negation() {
        let (val, _) = eval("f :: () i32 => { -42 }");
        assert_eq!(val, Value::Int(-42));
    }

    #[test]
    fn eval_unary_not() {
        let (val, _) = eval("f :: () bool => { not true }");
        assert_eq!(val, Value::Bool(false));
    }

    #[test]
    fn eval_bool_literal() {
        let (v1, _) = eval("f :: () bool => { true }");
        assert_eq!(v1, Value::Bool(true));

        let (v2, _) = eval("f :: () bool => { false }");
        assert_eq!(v2, Value::Bool(false));
    }

    #[test]
    fn eval_string_literal() {
        let (val, _) = eval("f :: () str => { \"hello\" }");
        assert_eq!(val, Value::Str("hello".into()));
    }

    #[test]
    fn eval_float_literal() {
        let (val, _) = eval("f :: () f64 => { 3.14 }");
        if let Value::Float(f) = val {
            assert!((f - 3.14).abs() < 0.001);
        } else {
            panic!("Expected Float value");
        }
    }

    #[test]
    fn eval_nested_arithmetic() {
        let (val, _) = eval("f :: () i32 => { 2 * (3 + 4) }");
        assert_eq!(val, Value::Int(14));
    }

    #[test]
    fn eval_path_lookup() {
        let source = r#"
            f :: () i32 => {
                x := 99
                x
            }
        "#;
        let (val, msgs) = eval(source);
        assert_eq!(val, Value::Int(99));
        assert!(msgs.is_empty());
    }

    #[test]
    fn eval_undefined_name_errors() {
        let (val, msgs) = eval("f :: () i32 => { undefined_var }");
        assert_eq!(val, Value::Error);
        assert!(msgs.iter().any(|m| m.contains("未定义")));
    }

    #[test]
    fn eval_type_mismatch_errors() {
        let (val, msgs) = eval("f :: () i32 => { 42 + true }");
        assert_eq!(val, Value::Error);
        assert!(msgs.iter().any(|m| m.contains("类型不匹配")));
    }

    #[test]
    fn eval_block_returns_last_expr() {
        let source = r#"
            f :: () i32 => {
                x := 10
                y := 32
                x + y
            }
        "#;
        let (val, _) = eval(source);
        assert_eq!(val, Value::Int(42));
    }

    #[test]
    fn eval_let_binding() {
        let source = r#"
            f :: () i32 => {
                x := 20
                x * 2
            }
        "#;
        let (val, msgs) = eval(source);
        if !msgs.is_empty() {
            eprintln!("Diagnostics: {:?}", msgs);
        }
        assert_eq!(val, Value::Int(40));
    }

    #[test]
    fn eval_assignment() {
        let source = r#"
            f :: () i32 => {
                ~x := 10
                x = 20
                x
            }
        "#;
        let (val, _) = eval(source);
        assert_eq!(val, Value::Int(20));
    }

    #[test]
    fn eval_branch_with_cond() {
        let source = r#"
            f :: () i32 => {
                ? { true => 42, _ => 0 }
            }
        "#;
        let (val, msgs) = eval(source);
        if !msgs.is_empty() {
            eprintln!("诊断消息: {:?}", msgs);
        }
        assert_eq!(val, Value::Int(42));
    }

    #[test]
    fn eval_branch_wildcard() {
        let source = r#"
            f :: () i32 => {
                ? { false => 0, _ => 99 }
            }
        "#;
        let (val, msgs) = eval(source);
        if !msgs.is_empty() {
            eprintln!("诊断消息: {:?}", msgs);
        }
        assert_eq!(val, Value::Int(99));
    }

    #[test]
    fn eval_branch_no_match_returns_unit() {
        let source = r#"
            f :: () => {
                ? false => 42
            }
        "#;
        let (val, _) = eval(source);
        assert_eq!(val, Value::Unit);
    }

    #[test]
    fn eval_scoped_shadowing() {
        let source = r#"
            f :: () i32 => {
                x := 10
                {
                    x := 20
                    x
                }
            }
        "#;
        let (val, _) = eval(source);
        assert_eq!(val, Value::Int(20));
    }
}
