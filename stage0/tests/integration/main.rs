//! 集成测试入口。
//!
//! 集成测试验证多个模块协作时的行为，例如 lexer + parser、
//! parser + resolve 等跨模块 pipeline。
//!
//! 每个子模块对应一个测试场景或主题。

mod cli_args;
mod codegen_diag;
mod contract;
mod lexer_parser;
mod parser_smoke;
mod stderr_output;
mod string_support;
mod verify_annotations;
mod pass_pipeline;
