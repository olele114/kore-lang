//! 类型表示。覆盖 Kore0 子集的所有类型。

use std::fmt;

/// Kore0 的类型。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    /// 基础整数类型（i32, u64, 等）。
    Int { signed: bool, width: u8 },
    /// 浮点类型（f32, f64）。
    Float { width: u8 },
    /// 布尔类型。
    Bool,
    /// 字符串类型。
    Str,
    /// void 类型（无值）。
    Void,
    /// never 类型（不返回）。
    Never,
    /// 借用指针 `^T`。
    Borrow(Box<Type>),
    /// 所有指针 `own ^T`。
    Own(Box<Type>),
    /// 数组 `[N]T`。
    Array { elem: Box<Type>, len: u64 },
    /// 切片 `[]T`。
    Slice { elem: Box<Type> },
    /// 错误联合 `T ! E`。
    ErrUnion { ok: Box<Type>, err: Box<Type> },
    /// 具名结构体。
    Struct(String),
    /// 具名联合。
    Union(String),
    /// 函数类型。
    Func {
        params: Vec<Type>,
        ret: Box<Type>,
        err: Option<Box<Type>>,
    },
    /// 未知类型（类型推断失败时的占位）。
    Unknown,
}

impl Type {
    /// 创建 i32 类型。
    pub fn i32() -> Self {
        Type::Int { signed: true, width: 32 }
    }

    /// 创建 i64 类型。
    pub fn i64() -> Self {
        Type::Int { signed: true, width: 64 }
    }

    /// 创建 u32 类型。
    pub fn u32() -> Self {
        Type::Int { signed: false, width: 32 }
    }

    /// 创建 u64 类型。
    pub fn u64() -> Self {
        Type::Int { signed: false, width: 64 }
    }

    /// 创建 f64 类型。
    pub fn f64() -> Self {
        Type::Float { width: 64 }
    }

    /// 是否是 never 类型。
    pub fn is_never(&self) -> bool {
        matches!(self, Type::Never)
    }

    /// 是否是整数类型。
    pub fn is_int(&self) -> bool {
        matches!(self, Type::Int { .. })
    }

    /// 是否是浮点类型。
    pub fn is_float(&self) -> bool {
        matches!(self, Type::Float { .. })
    }

    /// 是否是数值类型（整数或浮点）。
    pub fn is_numeric(&self) -> bool {
        self.is_int() || self.is_float()
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int { signed, width } => {
                write!(f, "{}{}", if *signed { "i" } else { "u" }, width)
            }
            Type::Float { width } => write!(f, "f{}", width),
            Type::Bool => write!(f, "bool"),
            Type::Str => write!(f, "str"),
            Type::Void => write!(f, "void"),
            Type::Never => write!(f, "never"),
            Type::Borrow(t) => write!(f, "^{}", t),
            Type::Own(t) => write!(f, "own ^{}", t),
            Type::Array { elem, len } => write!(f, "[{}]{}", len, elem),
            Type::Slice { elem } => write!(f, "[]{}", elem),
            Type::ErrUnion { ok, err } => write!(f, "{} ! {}", ok, err),
            Type::Struct(name) => write!(f, "{}", name),
            Type::Union(name) => write!(f, "{}", name),
            Type::Func { params, ret, err } => {
                write!(f, "(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", p)?;
                }
                write!(f, ") {}", ret)?;
                if let Some(e) = err {
                    write!(f, " ! {}", e)?;
                }
                Ok(())
            }
            Type::Unknown => write!(f, "<unknown>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_type_display() {
        assert_eq!(Type::i32().to_string(), "i32");
        assert_eq!(Type::u64().to_string(), "u64");
        assert_eq!(Type::f64().to_string(), "f64");
        assert_eq!(Type::Bool.to_string(), "bool");
        assert_eq!(Type::Str.to_string(), "str");
        assert_eq!(Type::Void.to_string(), "void");
    }

    #[test]
    fn pointer_type_display() {
        assert_eq!(Type::Borrow(Box::new(Type::i32())).to_string(), "^i32");
        assert_eq!(Type::Own(Box::new(Type::Str)).to_string(), "own ^str");
    }

    #[test]
    fn array_type_display() {
        let arr = Type::Array { elem: Box::new(Type::i32()), len: 10 };
        assert_eq!(arr.to_string(), "[10]i32");
    }

    #[test]
    fn type_predicates() {
        assert!(Type::i32().is_int());
        assert!(Type::f64().is_float());
        assert!(Type::i32().is_numeric());
        assert!(Type::f64().is_numeric());
        assert!(!Type::Bool.is_numeric());
        assert!(Type::Never.is_never());
    }

    #[test]
    fn never_type_display() {
        assert_eq!(Type::Never.to_string(), "never");
        assert!(!Type::Void.is_never());
        assert!(!Type::i32().is_never());
    }

    #[test]
    fn error_union_type_display() {
        let eu = Type::ErrUnion {
            ok: Box::new(Type::i32()),
            err: Box::new(Type::Str),
        };
        assert_eq!(eu.to_string(), "i32 ! str");
    }

    #[test]
    fn struct_union_type_display() {
        assert_eq!(Type::Struct("Point".to_string()).to_string(), "Point");
        assert_eq!(Type::Union("Result".to_string()).to_string(), "Result");
    }

    #[test]
    fn func_type_display() {
        let f1 = Type::Func {
            params: vec![Type::i32(), Type::Str],
            ret: Box::new(Type::Bool),
            err: None,
        };
        assert_eq!(f1.to_string(), "(i32, str) bool");

        let f2 = Type::Func {
            params: vec![],
            ret: Box::new(Type::Void),
            err: Some(Box::new(Type::Str)),
        };
        assert_eq!(f2.to_string(), "() void ! str");
    }

    #[test]
    fn unknown_type_display() {
        assert_eq!(Type::Unknown.to_string(), "<unknown>");
    }

    #[test]
    fn integer_type_helpers() {
        assert_eq!(Type::i32(), Type::Int { signed: true, width: 32 });
        assert_eq!(Type::i64(), Type::Int { signed: true, width: 64 });
        assert_eq!(Type::u32(), Type::Int { signed: false, width: 32 });
        assert_eq!(Type::u64(), Type::Int { signed: false, width: 64 });
    }

    #[test]
    fn float_type_helpers() {
        assert_eq!(Type::f64(), Type::Float { width: 64 });
        assert!(Type::f64().is_float());
        assert!(!Type::f64().is_int());
    }

    #[test]
    fn type_equality() {
        assert_eq!(Type::i32(), Type::i32());
        assert_ne!(Type::i32(), Type::u32());
        assert_ne!(Type::i32(), Type::i64());
        assert_eq!(Type::Bool, Type::Bool);
        assert_ne!(Type::Void, Type::Never);
    }

    #[test]
    fn complex_nested_types() {
        let t1 = Type::Array {
            elem: Box::new(Type::Borrow(Box::new(Type::i32()))),
            len: 5,
        };
        assert_eq!(t1.to_string(), "[5]^i32");

        let t2 = Type::Own(Box::new(Type::Array {
            elem: Box::new(Type::Str),
            len: 100,
        }));
        assert_eq!(t2.to_string(), "own ^[100]str");
    }
}
