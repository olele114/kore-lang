//! HIR（高级中间表示）核心数据结构。
//!
//! HIR 是 AST 到 LLVM IR 的桥接层，特点：
//! - 显式 CFG 基本块（每个块有唯一 Terminator）
//! - 类型擦除为扁平类型（原始类型 + 指针 + 聚合）
//! - 嵌套表达式拆分为临时变量 + 语句序列
//! - 控制流显式化（? / @ 降级为 goto + switch）

use crate::diag::Span;
use std::fmt;

pub mod ty;
pub mod visitor;
pub mod printer;

use ty::HirType;

// ────────────────────────────────────────────────────────────────────────────────
// 模块级别结构
// ────────────────────────────────────────────────────────────────────────────────

/// HIR 模块（单个编译单元）
#[derive(Debug, Clone)]
pub struct HirModule {
    pub functions: Vec<HirFunction>,
    pub structs: Vec<HirStruct>,
    pub unions: Vec<HirUnion>,
    pub globals: Vec<HirGlobal>,
}

/// 函数定义
#[derive(Debug, Clone)]
pub struct HirFunction {
    pub name: String,
    pub params: Vec<HirParam>,
    pub ret_type: ty::HirType,
    /// 函数体（内置函数为 None）
    pub body: Option<HirBody>,
    pub span: Span,
}

/// 函数参数
#[derive(Debug, Clone)]
pub struct HirParam {
    pub name: String,
    pub ty: ty::HirType,
    pub span: Span,
}

/// 结构体定义
#[derive(Debug, Clone)]
pub struct HirStruct {
    pub name: String,
    pub fields: Vec<HirField>,
    pub span: Span,
}

/// 联合体定义
#[derive(Debug, Clone)]
pub struct HirUnion {
    pub name: String,
    pub variants: Vec<HirVariant>,
    pub span: Span,
}

/// 结构体字段
#[derive(Debug, Clone)]
pub struct HirField {
    pub name: String,
    pub ty: ty::HirType,
    pub span: Span,
}

/// 联合体变体
#[derive(Debug, Clone)]
pub struct HirVariant {
    pub name: String,
    pub payload: Option<ty::HirType>,
    pub span: Span,
}

/// 全局变量
#[derive(Debug, Clone)]
pub struct HirGlobal {
    pub name: String,
    pub ty: ty::HirType,
    pub init: Option<Const>,
    pub span: Span,
}

// ────────────────────────────────────────────────────────────────────────────────
// 函数体 CFG
// ────────────────────────────────────────────────────────────────────────────────

/// 函数体（CFG 基本块集合）
#[derive(Debug, Clone)]
pub struct HirBody {
    pub blocks: Vec<HirBlock>,
    pub locals: Vec<HirLocal>,      // 局部变量表（包括参数和临时变量）
    pub entry_block: BlockId,        // 入口块 ID
}

/// 基本块
#[derive(Debug, Clone)]
pub struct HirBlock {
    pub id: BlockId,
    pub stmts: Vec<HirStmt>,
    pub terminator: HirTerminator,
    pub span: Span,
}

/// 局部变量（栈分配）
#[derive(Debug, Clone)]
pub struct HirLocal {
    pub name: Option<String>,       // 临时变量无名字
    pub ty: ty::HirType,
    pub span: Span,
}

/// 基本块 ID（索引到 HirBody.blocks）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub usize);

/// 局部变量 ID（索引到 HirBody.locals）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub usize);

/// 结构体 ID（索引到 HirModule.structs）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructId(pub usize);

/// 联合体 ID（索引到 HirModule.unions）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnionId(pub usize);

/// 函数 ID（索引到 HirModule.functions）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncId(pub usize);

// ────────────────────────────────────────────────────────────────────────────────
// 语句与终结符
// ────────────────────────────────────────────────────────────────────────────────

/// HIR 语句（基本块内的原子操作）
#[derive(Debug, Clone)]
pub enum HirStmt {
    /// 赋值：lhs = rhs
    Assign {
        lhs: HirPlace,
        rhs: HirRvalue,
        span: Span,
    },

    /// 函数调用（可能有副作用）
    Call {
        dest: Option<HirPlace>,     // 返回值存储位置（void 函数为 None）
        func: HirOperand,            // 函数指针或直接函数名
        args: Vec<HirOperand>,
        span: Span,
    },

    /// 显式析构（defer 展开生成）
    Drop {
        place: HirPlace,
        span: Span,
    },
}

/// 基本块终结符（控制流跳转）
#[derive(Debug, Clone)]
pub enum HirTerminator {
    /// 无条件跳转
    Goto(BlockId),

    /// 函数返回
    Return(Option<HirOperand>),

    /// 条件分支（类似 LLVM switch）
    Switch {
        discr: HirOperand,                      // 判别式
        targets: Vec<(u64, BlockId)>,           // (值, 目标块)
        otherwise: BlockId,                      // 默认分支
    },

    /// 不可达（never 类型路径结束点）
    Unreachable,
}

// ────────────────────────────────────────────────────────────────────────────────
// 左值与右值
// ────────────────────────────────────────────────────────────────────────────────

/// 左值（Place）：可寻址的内存位置
#[derive(Debug, Clone)]
pub enum HirPlace {
    /// 局部变量
    Local(LocalId),

    /// 结构体字段访问：base.field
    Field {
        base: Box<HirPlace>,
        field: usize,               // 字段索引
    },

    /// 数组索引：base[index]
    Index {
        base: Box<HirPlace>,
        index: Box<HirOperand>,
    },

    /// 指针解引用：*ptr
    Deref(Box<HirPlace>),
}

/// 右值（Rvalue）：计算表达式
#[derive(Debug, Clone)]
pub enum HirRvalue {
    /// 使用操作数（读取值）
    Use(HirOperand),

    /// 二元运算
    BinaryOp {
        op: BinOp,
        lhs: HirOperand,
        rhs: HirOperand,
    },

    /// 一元运算
    UnaryOp {
        op: UnOp,
        operand: HirOperand,
    },

    /// 取引用：^x 或 own ^x
    Ref {
        place: HirPlace,
        owned: bool,
    },

    /// 指针解引用（读取）
    Deref(HirOperand),

    /// 聚合构造（结构体/联合体字面量）
    Aggregate {
        kind: AggregateKind,
        fields: Vec<HirOperand>,
    },

    /// 类型判别式（联合体 tag）
    Discriminant(HirPlace),

    /// 提取联合体 payload（假设已验证 tag）
    ExtractPayload {
        place: HirPlace,
        variant_index: usize,
    },

    /// 数组到切片的转换：[N]T -> []T
    /// 生成 {ptr, len} 结构，ptr 指向数组首元素，len 为数组长度
    ArrayToSlice {
        array: HirOperand,
        elem_ty: HirType,
        len: usize,
    },
}

/// 操作数（值的来源）
#[derive(Debug, Clone)]
pub enum HirOperand {
    /// 常量
    Const(Const),

    /// 左值（读取内存位置）
    Place(Box<HirPlace>),

    /// 函数引用
    FuncRef(FuncId),
}

// ────────────────────────────────────────────────────────────────────────────────
// 运算符与聚合类型
// ────────────────────────────────────────────────────────────────────────────────

/// 二元运算符
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add, Sub, Mul, Div, Rem,
    BitAnd, BitOr, BitXor, Shl, Shr,
    Eq, Ne, Lt, Le, Gt, Ge,
    LogicAnd, LogicOr,
}

/// 一元运算符
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,        // -x
    Not,        // !x
    BitNot,     // ~x
}

/// 聚合类型种类
#[derive(Debug, Clone)]
pub enum AggregateKind {
    Struct(StructId),
    Union(UnionId, usize),      // (联合体 ID, 变体索引)
    Array(ty::HirType, usize),  // (元素类型, 长度)
    /// 错误联合构造：(变体索引: 0=Ok, 1=Err, 声明的错误联合类型)
    ///
    /// 必须携带声明类型：payload 槽位大小由 ok/err 两侧的最大值决定，
    /// 不能从实际 payload 值反推，否则 `i32 ! str` 这类两侧尺寸不同的
    /// 联合会按较小一侧分配，导致越界写入与字段截断。
    ErrorUnion(usize, ty::HirType),
}

/// 常量值
#[derive(Debug, Clone)]
pub enum Const {
    Void,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
    Nil,
}

// ────────────────────────────────────────────────────────────────────────────────
// Display 实现（用于调试）
// ────────────────────────────────────────────────────────────────────────────────

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

impl fmt::Display for LocalId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "_{}", self.0)
    }
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*",
            BinOp::Div => "/", BinOp::Rem => "%",
            BinOp::BitAnd => "&", BinOp::BitOr => "|", BinOp::BitXor => "^",
            BinOp::Shl => "<<", BinOp::Shr => ">>",
            BinOp::Eq => "==", BinOp::Ne => "!=",
            BinOp::Lt => "<", BinOp::Le => "<=",
            BinOp::Gt => ">", BinOp::Ge => ">=",
            BinOp::LogicAnd => "&&", BinOp::LogicOr => "||",
        };
        write!(f, "{}", s)
    }
}

impl fmt::Display for UnOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            UnOp::Neg => "-",
            UnOp::Not => "!",
            UnOp::BitNot => "~",
        };
        write!(f, "{}", s)
    }
}
