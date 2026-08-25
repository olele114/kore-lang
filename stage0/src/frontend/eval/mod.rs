//! 编译期求值：AST 解释器，执行编译期绑定与常量表达式。
//!
//! 用于：
//! - `::` 编译期绑定的值计算
//! - 数组长度等常量表达式
//! - 编译期断言

pub mod env;
pub mod evaluator;
pub mod value;

pub use env::EvalEnv;
pub use evaluator::Evaluator;
pub use value::Value;
