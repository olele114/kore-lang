//! AST 节点。**只覆盖 Kore0 子集**（ADR 007:240–241）。
//!
//! 有：函数、结构体、联合、指针、数组、基础类型、`?`/`@`、`own`/`defer`。
//! 无：trait、泛型、编译期求值、动态派发、复杂类型推断。这些节点在 stage0
//! 里根本不存在，而不是存在但报「未实现」——stage1 才会长出它们。

use crate::diag::Span;

/// 一个源文件。
#[derive(Debug, Clone)]
pub struct Module {
    pub items: Vec<Item>,
    pub span: Span,
}

/// 顶层项。
#[derive(Debug, Clone)]
pub enum Item {
    /// 函数：`f :: (a i32) i32 => expr`。
    Func(Func),
    /// 结构体：`Vec3 :: {x, y, z f32}`。
    Struct(StructDef),
    /// 联合：`Shape :: .Circle(f32) | .Rect(f32, f32)`。
    Union(UnionDef),
    /// 导入：`use std.io`。
    Use(UsePath),
}

#[derive(Debug, Clone)]
pub struct Func {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    /// 错误联合 `T ! E` 的错误侧。
    pub err: Option<TypeExpr>,
    pub body: Expr,
    pub span: Span,
    /// 是否为公共符号（`pub` 标记）。
    pub is_public: bool,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    /// 可变标记 `~`。
    pub is_mut: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
    /// 是否为公共符号（`pub` 标记）。
    pub is_public: bool,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct UnionDef {
    pub name: String,
    pub variants: Vec<Variant>,
    pub span: Span,
    /// 是否为公共符号（`pub` 标记）。
    pub is_public: bool,
}

/// 联合的变体，如 `.Circle(f32)`。
#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
    pub payload: Vec<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct UsePath {
    pub segments: Vec<String>,
    pub span: Span,
}

/// 类型表达式。
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// 基础类型或具名类型。
    Named(String, Span),
    /// 借用指针 `^T`。
    Borrow(Box<TypeExpr>, Span),
    /// 所有指针 `own ^T`。
    Own(Box<TypeExpr>, Span),
    /// 固定大小数组 `[N]T`。
    Array(Box<TypeExpr>, u64, Span),
    /// 动态大小切片 `[]T`。
    Slice(Box<TypeExpr>, Span),
    /// 错误联合 `T ! E`。
    ErrUnion(Box<TypeExpr>, Box<TypeExpr>, Span),
}

/// 语句。ADR 002 定的表达式/语句边界在 stage0 里体现为：块里放语句，
/// 语句可以是表达式。
#[derive(Debug, Clone)]
pub enum Stmt {
    /// 运行期绑定：`x : T = e` 或 `x := e`。
    Let {
        name: String,
        is_mut: bool,
        ty: Option<TypeExpr>,
        init: Expr,
        span: Span,
    },
    /// 赋值：`x = e`。
    Assign { target: Expr, value: Expr, span: Span },
    /// `defer expr`。
    Defer(Expr, Span),
    /// 表达式语句。
    Expr(Expr),
}

/// 表达式。
#[derive(Debug, Clone)]
pub enum Expr {
    Int(String, Span),
    Float(String, Span),
    Str(String, Span),
    Bool(bool, Span),
    Nil(Span),
    Path(Vec<String>, Span),
    /// 分支 `?`，统一了 if / else / switch / match。
    Branch { scrutinee: Option<Box<Expr>>, arms: Vec<Arm>, span: Span },
    /// 循环 `@`，统一了 for / while / foreach。支持标签 `@label @{ ... }`。
    Loop { label: Option<String>, subject: Option<Box<Expr>>, body: Box<Expr>, span: Span },
    Call { callee: Box<Expr>, args: Vec<Expr>, span: Span },
    Field { base: Box<Expr>, name: String, span: Span },
    Index { base: Box<Expr>, index: Box<Expr>, span: Span },
    /// 解引用 `e^`。
    Deref(Box<Expr>, Span),
    /// 传播 `e!`。
    Propagate(Box<Expr>, Span),
    Unary { op: &'static str, operand: Box<Expr>, span: Span },
    Binary { op: &'static str, lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
    /// 块。
    Block { stmts: Vec<Stmt>, span: Span },
    /// `ret` 或 `ret expr`（never 类型）。
    Ret(Option<Box<Expr>>, Span),
    /// `stop` 或 `stop @label`（never 类型）。
    Stop { label: Option<String>, span: Span },
    /// `skip` 或 `skip @label`（never 类型）。
    Skip { label: Option<String>, span: Span },
    /// 保证尾调用 `jmp f(..)` 或 `jmp @label`（never 类型）。
    Jmp { target: Option<Box<Expr>>, label: Option<String>, span: Span },
    /// 结构体字面量 `Point{x: 1, y: 2}`
    StructLit { name: String, fields: Vec<(String, Expr)>, span: Span },
    /// 数组字面量 `[1, 2, 3]`
    ArrayLit { elements: Vec<Expr>, span: Span },
    /// 变体构造 `.Ok(42)` 或 `.None`
    VariantConstructor { name: String, payload: Option<Box<Expr>>, span: Span },
}

/// 分支的臂：`模式 => 表达式`。
#[derive(Debug, Clone)]
pub struct Arm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

/// 模式。Kore0 只要求认出变体、绑定与通配。
#[derive(Debug, Clone)]
pub enum Pattern {
    /// `.Circle(r)`。
    Variant { name: String, bindings: Vec<String>, span: Span },
    /// 绑定一个名字。
    Bind(String, Span),
    /// 字面量模式。
    Lit(Box<Expr>),
    /// `_`。
    Wildcard(Span),
    /// 无模式的臂（守卫的条件位），条件是一个表达式。
    Cond(Box<Expr>),
}

impl Expr {
    /// 每个表达式都要能报出自己的位置——诊断挂不到 span 上就没法指向源码。
    pub fn span(&self) -> Span {
        match self {
            Expr::Int(_, s)
            | Expr::Float(_, s)
            | Expr::Str(_, s)
            | Expr::Bool(_, s)
            | Expr::Nil(s)
            | Expr::Path(_, s)
            | Expr::Deref(_, s)
            | Expr::Propagate(_, s) => *s,
            Expr::Branch { span, .. }
            | Expr::Loop { span, .. }
            | Expr::Call { span, .. }
            | Expr::Field { span, .. }
            | Expr::Index { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Block { span, .. }
            | Expr::Ret(_, span)
            | Expr::Stop { span, .. }
            | Expr::Skip { span, .. }
            | Expr::Jmp { span, .. }
            | Expr::StructLit { span, .. }
            | Expr::ArrayLit { span, .. }
            | Expr::VariantConstructor { span, .. } => *span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::FileId;

    #[test]
    fn every_expr_reports_its_span() {
        let s = Span::new(FileId(0), 3, 7);
        assert_eq!(Expr::Int("1".into(), s).span(), s);
        assert_eq!(Expr::Block { stmts: Vec::new(), span: s }.span(), s);
        assert_eq!(Expr::Deref(Box::new(Expr::Nil(s)), s).span(), s);
    }
}
