//! HIR 优化 Pass 框架。
//!
//! 提供 Pass trait 和 PassManager，用于在 HIR 上执行优化转换。
//! stage0 实现简单优化以减少生成的 LLVM IR 体积，提升代码质量。

use crate::middleend::hir::{HirBody, HirModule};

pub mod dead_code;
pub mod const_fold;
pub mod dead_store;

/// Pass trait - 所有优化 pass 必须实现此接口
pub trait Pass {
    /// Pass 名称（用于调试和日志）
    fn name(&self) -> &str;

    /// 在函数体上运行 pass，返回是否做了修改
    fn run_on_body(&mut self, body: &mut HirBody) -> bool;
}

/// Pass 管理器 - 按顺序执行多个 pass
pub struct PassManager {
    passes: Vec<Box<dyn Pass>>,
    max_iterations: usize,
}

impl PassManager {
    /// 创建新的 PassManager
    pub fn new() -> Self {
        Self {
            passes: Vec::new(),
            max_iterations: 3,  // 最多迭代 3 次（避免无限循环）
        }
    }

    /// 添加 pass
    pub fn add_pass(&mut self, pass: Box<dyn Pass>) {
        self.passes.push(pass);
    }

    /// 在整个模块上运行所有 pass
    pub fn run_on_module(&mut self, module: &mut HirModule) {
        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                self.run_on_body(body);
            }
        }
    }

    /// 在函数体上运行所有 pass（迭代直到不动点）
    fn run_on_body(&mut self, body: &mut HirBody) {
        for _ in 0..self.max_iterations {
            let mut changed = false;

            for pass in &mut self.passes {
                if pass.run_on_body(body) {
                    changed = true;
                }
            }

            // 如果没有任何改变，提前退出
            if !changed {
                break;
            }
        }
    }
}

impl Default for PassManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 创建 stage0 默认优化管道
pub fn default_pipeline() -> PassManager {
    let mut pm = PassManager::new();

    // Pass 顺序：死代码消除 -> 常量折叠 -> 无用赋值消除
    pm.add_pass(Box::new(dead_code::DeadCodeElimination));
    pm.add_pass(Box::new(const_fold::ConstantFolding));
    pm.add_pass(Box::new(dead_store::DeadStoreElimination));

    pm
}
