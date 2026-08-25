//! 编译期值：AST 解释器执行结果的表示。

use crate::diag::Span;
use crate::frontend::ast::TypeExpr;

/// 编译期求值结果。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// 整数（不区分 i32/i64/u32/u64，统一为 i64 表示）
    Int(i64),
    /// 浮点数
    Float(f64),
    /// 布尔值
    Bool(bool),
    /// 字符串
    Str(String),
    /// 类型值（用于 `Vec3 :: {x, y, z f32}` 这种绑定）
    Type(Box<TypeExpr>),
    /// 函数（编译期不能执行，但可以作为值传递）
    Func { name: String, span: Span },
    /// 单元值
    Unit,
    /// 求值失败（用于错误恢复）
    Error,
}

impl Value {
    /// 是否为真值（用于条件判断）
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(0) => false,
            Value::Unit | Value::Error => false,
            _ => true,
        }
    }

    /// 尝试转为整数
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// 尝试转为布尔值
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// 类型名称（用于错误消息）
    pub fn type_name(&self) -> &str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::Str(_) => "str",
            Value::Type(_) => "type",
            Value::Func { .. } => "func",
            Value::Unit => "void",
            Value::Error => "<error>",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::FileId;

    fn sp() -> Span {
        Span::new(FileId(0), 0, 0)
    }

    #[test]
    fn int_is_truthy_except_zero() {
        assert!(Value::Int(1).is_truthy());
        assert!(Value::Int(-1).is_truthy());
        assert!(!Value::Int(0).is_truthy());
    }

    #[test]
    fn bool_truthy_matches_value() {
        assert!(Value::Bool(true).is_truthy());
        assert!(!Value::Bool(false).is_truthy());
    }

    #[test]
    fn unit_and_error_are_falsy() {
        assert!(!Value::Unit.is_truthy());
        assert!(!Value::Error.is_truthy());
    }

    #[test]
    fn non_zero_values_are_truthy() {
        assert!(Value::Float(1.0).is_truthy());
        assert!(Value::Str("x".into()).is_truthy());
        assert!(Value::Func { name: "f".into(), span: sp() }.is_truthy());
    }

    #[test]
    fn as_int_extracts_integer() {
        assert_eq!(Value::Int(42).as_int(), Some(42));
        assert_eq!(Value::Bool(true).as_int(), None);
    }

    #[test]
    fn as_bool_extracts_boolean() {
        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert_eq!(Value::Int(1).as_bool(), None);
    }

    #[test]
    fn type_name_returns_correct_string() {
        assert_eq!(Value::Int(0).type_name(), "int");
        assert_eq!(Value::Float(0.0).type_name(), "float");
        assert_eq!(Value::Bool(true).type_name(), "bool");
        assert_eq!(Value::Str("".into()).type_name(), "str");
        assert_eq!(Value::Unit.type_name(), "void");
        assert_eq!(Value::Error.type_name(), "<error>");
        assert_eq!(Value::Func { name: "f".into(), span: sp() }.type_name(), "func");
    }

    #[test]
    fn value_equality_works() {
        assert_eq!(Value::Int(42), Value::Int(42));
        assert_ne!(Value::Int(42), Value::Int(43));
        assert_eq!(Value::Bool(true), Value::Bool(true));
        assert_eq!(Value::Str("x".into()), Value::Str("x".into()));
        assert_eq!(Value::Unit, Value::Unit);
        assert_eq!(Value::Error, Value::Error);
    }

    #[test]
    fn value_clone_works() {
        let v = Value::Int(42);
        let cloned = v.clone();
        assert_eq!(v, cloned);
    }
}
