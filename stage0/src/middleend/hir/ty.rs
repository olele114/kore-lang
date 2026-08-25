//! HIR 类型系统。
//!
//! HIR 类型是前端类型系统的简化版本：
//! - 无泛型类型参数
//! - 无 trait 约束
//! - 结构体/联合体通过 ID 引用（避免递归类型问题）
//! - 函数类型仅用于函数指针（Kore0 无闭包）

use super::{StructId, UnionId};
use std::fmt;

/// HIR 类型
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirType {
    /// 空类型（副作用构造的返回类型）
    Void,

    /// 底类型（发散控制流）
    Never,

    /// 布尔
    Bool,

    /// 字符串（编译期为字面量，运行期为胖指针 {ptr, len}）
    Str,

    /// 整数
    Int {
        width: u8,      // 8, 16, 32, 64
        signed: bool,
    },

    /// 浮点
    Float {
        width: u8,      // 32, 64
    },

    /// 指针
    Ptr {
        pointee: Box<HirType>,
        owned: bool,    // true = own ^T, false = ^T
    },

    /// 固定大小数组
    Array {
        elem: Box<HirType>,
        len: usize,
    },

    /// 动态大小切片（运行期为胖指针 {ptr, len}）
    Slice {
        elem: Box<HirType>,
    },

    /// 结构体（通过 ID 引用，避免递归）
    Struct(StructId),

    /// 联合体（通过 ID 引用）
    Union(UnionId),

    /// 错误联合 `T ! E`
    ErrUnion {
        ok: Box<HirType>,
        err: Box<HirType>,
    },

    /// 函数指针（Kore0 无闭包，仅直接函数）
    FnPtr {
        params: Vec<HirType>,
        ret: Box<HirType>,
    },
}

impl HirType {
    /// 类型是否是 void
    pub fn is_void(&self) -> bool {
        matches!(self, HirType::Void)
    }

    /// 类型是否是 never
    pub fn is_never(&self) -> bool {
        matches!(self, HirType::Never)
    }

    /// 类型是否是有符号整数
    pub fn is_signed_int(&self) -> bool {
        matches!(self, HirType::Int { signed: true, .. })
    }

    /// 类型是否是指针
    pub fn is_ptr(&self) -> bool {
        matches!(self, HirType::Ptr { .. })
    }

    /// 获取整数位宽（如果是整数类型）
    pub fn int_width(&self) -> Option<u8> {
        match self {
            HirType::Int { width, .. } => Some(*width),
            _ => None,
        }
    }

    /// 获取指针指向的类型
    pub fn pointee(&self) -> Option<&HirType> {
        match self {
            HirType::Ptr { pointee, .. } => Some(pointee),
            _ => None,
        }
    }

    /// 类型的大小（字节数，用于代码生成）
    /// 注意：结构体/联合体需要查询模块定义
    pub fn size_hint(&self) -> Option<usize> {
        match self {
            HirType::Void | HirType::Never => Some(0),
            HirType::Bool => Some(1),
            HirType::Str => Some(16),  // 胖指针 {ptr: 8, len: 8}
            HirType::Int { width, .. } => Some((*width / 8) as usize),
            HirType::Float { width } => Some((*width / 8) as usize),
            HirType::Ptr { .. } => Some(8),  // 假设 64 位平台
            HirType::Array { elem, len } => {
                elem.size_hint().map(|s| s * len)
            }
            HirType::Slice { .. } => Some(16),  // 胖指针 {ptr: 8, len: 8}
            HirType::ErrUnion { ok, err } => {
                // Tagged union: 1 字节 tag + max(ok_size, err_size)
                match (ok.size_hint(), err.size_hint()) {
                    (Some(ok_s), Some(err_s)) => Some(1 + ok_s.max(err_s)),
                    _ => None,
                }
            }
            HirType::Struct(_) | HirType::Union(_) => None,  // 需要查询定义
            HirType::FnPtr { .. } => Some(8),
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────────
// Display 实现（调试用）
// ────────────────────────────────────────────────────────────────────────────────

impl fmt::Display for HirType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HirType::Void => write!(f, "void"),
            HirType::Never => write!(f, "never"),
            HirType::Bool => write!(f, "bool"),
            HirType::Str => write!(f, "str"),
            HirType::Int { width, signed } => {
                write!(f, "{}{}", if *signed { "i" } else { "u" }, width)
            }
            HirType::Float { width } => write!(f, "f{}", width),
            HirType::Ptr { pointee, owned } => {
                if *owned {
                    write!(f, "own ^{}", pointee)
                } else {
                    write!(f, "^{}", pointee)
                }
            }
            HirType::Array { elem, len } => write!(f, "[{}; {}]", elem, len),
            HirType::Slice { elem } => write!(f, "[{}]", elem),
            HirType::ErrUnion { ok, err } => write!(f, "{} ! {}", ok, err),
            HirType::Struct(id) => write!(f, "struct#{}", id.0),
            HirType::Union(id) => write!(f, "union#{}", id.0),
            HirType::FnPtr { params, ret } => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", p)?;
                }
                write!(f, ") => {}", ret)
            }
        }
    }
}

/// 便捷构造函数
impl HirType {
    pub fn i32() -> Self {
        HirType::Int { width: 32, signed: true }
    }

    pub fn u32() -> Self {
        HirType::Int { width: 32, signed: false }
    }

    pub fn i64() -> Self {
        HirType::Int { width: 64, signed: true }
    }

    pub fn u64() -> Self {
        HirType::Int { width: 64, signed: false }
    }

    pub fn f32() -> Self {
        HirType::Float { width: 32 }
    }

    pub fn f64() -> Self {
        HirType::Float { width: 64 }
    }

    pub fn ptr(pointee: HirType) -> Self {
        HirType::Ptr {
            pointee: Box::new(pointee),
            owned: false,
        }
    }

    pub fn own_ptr(pointee: HirType) -> Self {
        HirType::Ptr {
            pointee: Box::new(pointee),
            owned: true,
        }
    }

    pub fn array(elem: HirType, len: usize) -> Self {
        HirType::Array {
            elem: Box::new(elem),
            len,
        }
    }

    pub fn slice(elem: HirType) -> Self {
        HirType::Slice {
            elem: Box::new(elem),
        }
    }

    pub fn err_union(ok: HirType, err: HirType) -> Self {
        HirType::ErrUnion {
            ok: Box::new(ok),
            err: Box::new(err),
        }
    }
}
