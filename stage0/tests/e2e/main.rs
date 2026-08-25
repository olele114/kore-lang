//! 端到端测试入口。
//!
//! E2E 测试验证完整的编译流程：从源码文件到最终产物。
//! 使用 `--~` 注解标记预期诊断，用 test_verifier 验证。
//!
//! 每个子模块对应一类端到端场景。

mod arrays;
mod cmdline_args;
mod error_handling;
mod escape_check;
mod executable;
mod file_io;
mod full_pipeline;
mod llvm_codegen;
mod module_system;
mod print_output;
mod smoke;
mod stderr_output;
mod string_codegen;
mod union_payload_regression;
mod union_types;
mod warning_annotations;
mod warnings;
