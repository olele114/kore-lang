//! AST 的 S 表达式打印。`--emit=ast` 走这里。
//!
//! 用 S 表达式而不是 JSON：ADR 009 第 174 行定了这条——几十行 Kore 就能在
//! 自举期解析 S 表达式，JSON 得先有一个完整的 JSON 解析器。
//!
//! span 默认不打印。位置随源码微调而漂移，带 span 的 dump 做不了稳定比对；
//! `--emit-spans` 才输出。

use super::node::*;
use crate::diag::Span;

/// 打印配置。
#[derive(Debug, Clone, Copy, Default)]
pub struct PrintOpts {
    /// 是否输出 span。
    pub spans: bool,
}

/// 把一个模块打印成 S 表达式。
pub fn print_module(m: &Module, opts: PrintOpts) -> String {
    let mut p = Printer { out: String::new(), depth: 0, opts };
    p.module(m);
    p.out
}

struct Printer {
    out: String,
    depth: usize,
    opts: PrintOpts,
}

impl Printer {
    fn indent(&mut self) {
        for _ in 0..self.depth {
            self.out.push_str("  ");
        }
    }

    /// 开一个列表并换行缩进，调用者负责随后 `close`。
    fn open(&mut self, head: &str) {
        self.indent();
        self.out.push('(');
        self.out.push_str(head);
        self.out.push('\n');
        self.depth += 1;
    }

    /// 开一个列表并输出 span，然后换行缩进。
    fn open_with_span(&mut self, head: &str, span: Span) {
        self.indent();
        self.out.push('(');
        self.out.push_str(head);
        self.span(span);
        self.out.push('\n');
        self.depth += 1;
    }

    fn close(&mut self) {
        self.depth -= 1;
        self.indent();
        self.out.push_str(")\n");
    }

    /// 单行的叶子节点。
    fn leaf(&mut self, head: &str, span: Span) {
        self.indent();
        self.out.push('(');
        self.out.push_str(head);
        self.span(span);
        self.out.push_str(")\n");
    }

    fn span(&mut self, s: Span) {
        if self.opts.spans {
            self.out
                .push_str(&format!(" :span {} {} {}", s.file.0, s.lo, s.hi));
        }
    }

    fn module(&mut self, m: &Module) {
        self.open("module");
        for it in &m.items {
            self.item(it);
        }
        self.close();
    }

    fn item(&mut self, it: &Item) {
        match it {
            Item::Func(f) => {
                self.open_with_span(&format!("func {}", f.name), f.span);
                for p in &f.params {
                    self.indent();
                    let mark = if p.is_mut { "~" } else { "" };
                    self.out
                        .push_str(&format!("(param {}{} {})\n", mark, p.name, ty(&p.ty)));
                }
                if let Some(t) = &f.ret {
                    self.indent();
                    self.out.push_str(&format!("(ret-type {})\n", ty(t)));
                }
                if let Some(t) = &f.err {
                    self.indent();
                    self.out.push_str(&format!("(err-type {})\n", ty(t)));
                }
                self.expr(&f.body);
                self.close();
            }
            Item::Struct(s) => {
                self.open_with_span(&format!("struct {}", s.name), s.span);
                for f in &s.fields {
                    self.indent();
                    self.out
                        .push_str(&format!("(field {} {})\n", f.name, ty(&f.ty)));
                }
                self.close();
            }
            Item::Union(u) => {
                self.open_with_span(&format!("union {}", u.name), u.span);
                for v in &u.variants {
                    self.indent();
                    let payload: Vec<String> = v.payload.iter().map(ty).collect();
                    self.out
                        .push_str(&format!("(variant .{} {})\n", v.name, payload.join(" ")));
                }
                self.close();
            }
            Item::Use(u) => {
                self.indent();
                self.out
                    .push_str(&format!("(use {})\n", u.segments.join(".")));
            }
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { name, is_mut, ty: t, init, .. } => {
                let mark = if *is_mut { "~" } else { "" };
                let tstr = t.as_ref().map(ty).unwrap_or_else(|| "_".into());
                self.open(&format!("let {}{} {}", mark, name, tstr));
                self.expr(init);
                self.close();
            }
            Stmt::Assign { target, value, .. } => {
                self.open("assign");
                self.expr(target);
                self.expr(value);
                self.close();
            }
            Stmt::Defer(e, _) => {
                self.open("defer");
                self.expr(e);
                self.close();
            }
            Stmt::Expr(e) => self.expr(e),
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Int(v, s) => self.leaf(&format!("int {v}"), *s),
            Expr::Float(v, s) => self.leaf(&format!("float {v}"), *s),
            Expr::Str(v, s) => self.leaf(&format!("str {v:?}"), *s),
            Expr::Bool(v, s) => self.leaf(&format!("bool {v}"), *s),
            Expr::Nil(s) => self.leaf("nil", *s),
            Expr::Path(segs, s) => self.leaf(&format!("path {}", segs.join(".")), *s),
            Expr::Branch { scrutinee, arms, .. } => {
                self.open("branch");
                if let Some(sc) = scrutinee {
                    self.expr(sc);
                }
                for a in arms {
                    self.open("arm");
                    self.pattern(&a.pattern);
                    self.expr(&a.body);
                    self.close();
                }
                self.close();
            }
            Expr::Loop { subject, body, .. } => {
                self.open("loop");
                if let Some(sub) = subject {
                    self.expr(sub);
                }
                self.expr(body);
                self.close();
            }
            Expr::Call { callee, args, .. } => {
                self.open("call");
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
                self.close();
            }
            Expr::Field { base, name, .. } => {
                self.open(&format!("field {name}"));
                self.expr(base);
                self.close();
            }
            Expr::Index { base, index, .. } => {
                self.open("index");
                self.expr(base);
                self.expr(index);
                self.close();
            }
            Expr::Deref(inner, _) => {
                self.open("deref");
                self.expr(inner);
                self.close();
            }
            Expr::Propagate(inner, _) => {
                self.open("propagate");
                self.expr(inner);
                self.close();
            }
            Expr::Unary { op, operand, .. } => {
                self.open(&format!("unary {op}"));
                self.expr(operand);
                self.close();
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                self.open(&format!("binary {op}"));
                self.expr(lhs);
                self.expr(rhs);
                self.close();
            }
            Expr::Block { stmts, .. } => {
                self.open("block");
                for s in stmts {
                    self.stmt(s);
                }
                self.close();
            }
            Expr::Ret(e, span) => match e {
                Some(e) => {
                    self.open("ret");
                    self.expr(e);
                    self.close();
                }
                None => self.leaf("ret", *span),
            },
            Expr::Stop { label, span } => {
                if let Some(lbl) = label {
                    self.open(&format!("stop @{}", lbl));
                } else {
                    self.open("stop");
                }
                self.leaf("", *span);
                self.close();
            }
            Expr::Skip { label, span } => {
                if let Some(lbl) = label {
                    self.leaf(&format!("skip @{}", lbl), *span)
                } else {
                    self.leaf("skip", *span)
                }
            }
            Expr::Jmp { target, label, span } => {
                match (target.as_ref(), label.as_ref()) {
                    (Some(e), None) => {
                        self.open("jmp");
                        self.expr(e);
                        self.close();
                    }
                    (None, Some(lbl)) => self.leaf(&format!("jmp @{}", lbl), *span),
                    _ => self.leaf("jmp", *span),
                }
            }
            Expr::StructLit { name, fields, .. } => {
                self.open(&format!("struct-lit {name}"));
                for (fname, fexpr) in fields {
                    self.open(&format!("field {fname}"));
                    self.expr(fexpr);
                    self.close();
                }
                self.close();
            }
            Expr::ArrayLit { elements, .. } => {
                self.open("array-lit");
                for elem in elements {
                    self.expr(elem);
                }
                self.close();
            }
            Expr::VariantConstructor { name, payload, .. } => {
                self.open(&format!("variant-ctor .{name}"));
                if let Some(p) = payload {
                    self.expr(p);
                }
                self.close();
            }
        }
    }

    fn pattern(&mut self, p: &Pattern) {
        match p {
            Pattern::Variant { name, bindings, .. } => {
                self.indent();
                self.out
                    .push_str(&format!("(pat-variant .{} {})\n", name, bindings.join(" ")));
            }
            Pattern::Bind(n, _) => {
                self.indent();
                self.out.push_str(&format!("(pat-bind {n})\n"));
            }
            Pattern::Lit(e) => {
                self.open("pat-lit");
                self.expr(e);
                self.close();
            }
            Pattern::Wildcard(_) => {
                self.indent();
                self.out.push_str("(pat-wildcard)\n");
            }
            Pattern::Cond(e) => {
                self.open("pat-cond");
                self.expr(e);
                self.close();
            }
        }
    }
}

/// 类型表达式压成一行。类型嵌套浅，多行反而看不清。
fn ty(t: &TypeExpr) -> String {
    match t {
        TypeExpr::Named(n, _) => n.clone(),
        TypeExpr::Borrow(inner, _) => format!("^{}", ty(inner)),
        TypeExpr::Own(inner, _) => format!("own ^{}", ty(inner)),
        TypeExpr::Array(inner, n, _) => format!("[{}]{}", n, ty(inner)),
        TypeExpr::Slice(inner, _) => format!("[]{}", ty(inner)),
        TypeExpr::ErrUnion(ok, err, _) => format!("{} ! {}", ty(ok), ty(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::FileId;

    fn sp() -> Span {
        Span::new(FileId(0), 4, 9)
    }

    fn sample() -> Module {
        let s = sp();
        Module {
            items: vec![Item::Func(Func {
                is_public: false,
                name: "main".into(),
                params: Vec::new(),
                ret: Some(TypeExpr::Named("void".into(), s)),
                err: None,
                body: Expr::Block { stmts: Vec::new(), span: s },
                span: s,
            })],
            span: s,
        }
    }

    #[test]
    fn spans_are_hidden_by_default() {
        let out = print_module(&sample(), PrintOpts::default());
        assert!(!out.contains(":span"), "默认不该带 span：{out}");
        assert!(out.contains("(func main"));
        assert!(out.contains("(ret-type void)"));
    }

    #[test]
    fn spans_appear_when_requested() {
        let out = print_module(&sample(), PrintOpts { spans: true });
        assert!(out.contains(":span 0 4 9"), "{out}");
    }

    #[test]
    fn parens_are_balanced() {
        let out = print_module(&sample(), PrintOpts { spans: true });
        let opens = out.chars().filter(|c| *c == '(').count();
        let closes = out.chars().filter(|c| *c == ')').count();
        assert_eq!(opens, closes, "括号不配平：{out}");
    }

    #[test]
    fn types_render_in_kore_notation() {
        let s = sp();
        let t = TypeExpr::Own(Box::new(TypeExpr::Array(
            Box::new(TypeExpr::Named("u8".into(), s)),
            16,
            s,
        )), s);
        assert_eq!(ty(&t), "own ^[16]u8");
        let e = TypeExpr::ErrUnion(
            Box::new(TypeExpr::Named("i32".into(), s)),
            Box::new(TypeExpr::Named("IoErr".into(), s)),
            s,
        );
        assert_eq!(ty(&e), "i32 ! IoErr");
    }
}
