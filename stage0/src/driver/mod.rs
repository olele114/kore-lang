//! Driver 协调模块。ADR 007 Q13：driver 协调 frontend/middleend/backend。
//!
//! 当前包含前端 pass 序列、测试验证器与契约断言检查点。

pub mod contract;
pub mod pipeline;
pub mod test_verifier;

pub use contract::{ContractAssertion, extract_contract_assertions, verify_contract_assertions};
pub use pipeline::{FrontendOutput, Stage, run_frontend, run_frontend_with_registry};
pub use test_verifier::{TestResult, verify_test_annotations};
