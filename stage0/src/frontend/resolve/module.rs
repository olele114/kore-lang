//! 模块注册表与依赖管理。
//!
//! 实现 ADR 012 Phase 5：模块系统的核心数据结构。
//!
//! ## 职责
//! - 模块注册：文件路径 → ModuleId 映射
//! - 依赖图构建：跟踪模块间的 use 关系
//! - 循环依赖检测：DFS 检测环
//! - 拓扑排序：确定编译顺序

use crate::diag::Span;
use crate::frontend::ast::node::Module;
use crate::frontend::resolve::symbols::SymbolTable;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

/// 模块标识符。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId(pub usize);

/// 单个导入记录。
#[derive(Debug, Clone)]
pub struct Import {
    /// 导入的模块名（use a.b.c 中的 c）
    pub imported_name: String,
    /// use 语句的完整路径段
    pub segments: Vec<String>,
    /// use 语句的 span
    pub span: Span,
}

/// 模块信息。
#[derive(Debug)]
pub struct ModuleInfo {
    pub id: ModuleId,
    /// 模块名（文件名无扩展名）
    pub name: String,
    /// 文件路径
    pub path: PathBuf,
    /// 解析后的 AST
    pub ast: Module,
    /// 导入信息
    pub imports: Vec<Import>,
    /// 导出符号表（pub 标记的符号）
    pub exports: SymbolTable,
}

/// 模块注册表。
///
/// 管理所有已加载模块的元信息和依赖关系。
#[derive(Debug)]
pub struct ModuleRegistry {
    /// 文件路径 → ModuleId
    path_to_id: HashMap<PathBuf, ModuleId>,
    /// 模块名 → ModuleId（用于 use 语句查找）
    name_to_id: HashMap<String, ModuleId>,
    /// ModuleId → ModuleInfo
    modules: HashMap<ModuleId, ModuleInfo>,
    /// 依赖图：ModuleId → 依赖的模块列表
    dependencies: HashMap<ModuleId, Vec<ModuleId>>,
    /// 下一个可用的 ModuleId
    next_id: usize,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self {
            path_to_id: HashMap::new(),
            name_to_id: HashMap::new(),
            modules: HashMap::new(),
            dependencies: HashMap::new(),
            next_id: 0,
        }
    }

    /// 注册一个新模块。
    ///
    /// 返回分配的 ModuleId。如果路径已存在，返回现有 ID。
    pub fn register_module(
        &mut self,
        path: PathBuf,
        name: String,
        ast: Module,
        imports: Vec<Import>,
    ) -> ModuleId {
        // 检查是否已注册
        if let Some(&id) = self.path_to_id.get(&path) {
            return id;
        }

        let id = ModuleId(self.next_id);
        self.next_id += 1;

        let info = ModuleInfo {
            id,
            name: name.clone(),
            path: path.clone(),
            ast,
            imports,
            exports: SymbolTable::new(),
        };

        self.path_to_id.insert(path, id);
        self.name_to_id.insert(name, id);
        self.modules.insert(id, info);
        self.dependencies.insert(id, Vec::new());

        id
    }

    /// 根据模块名查找 ModuleId。
    pub fn find_module_by_name(&self, name: &str) -> Option<ModuleId> {
        self.name_to_id.get(name).copied()
    }

    /// 根据路径查找 ModuleId。
    pub fn find_module_by_path(&self, path: &PathBuf) -> Option<ModuleId> {
        self.path_to_id.get(path).copied()
    }

    /// 获取模块信息（不可变引用）。
    pub fn get_module(&self, id: ModuleId) -> Option<&ModuleInfo> {
        self.modules.get(&id)
    }

    /// 获取模块信息（可变引用）。
    pub fn get_module_mut(&mut self, id: ModuleId) -> Option<&mut ModuleInfo> {
        self.modules.get_mut(&id)
    }

    /// 添加依赖关系：from_module 依赖 to_module。
    pub fn add_dependency(&mut self, from_module: ModuleId, to_module: ModuleId) {
        self.dependencies
            .entry(from_module)
            .or_insert_with(Vec::new)
            .push(to_module);
    }

    /// 获取模块的导出符号表。
    pub fn get_exports(&self, id: ModuleId) -> Option<&SymbolTable> {
        self.modules.get(&id).map(|m| &m.exports)
    }

    /// 获取所有模块 ID。
    pub fn all_module_ids(&self) -> Vec<ModuleId> {
        self.modules.keys().copied().collect()
    }

    /// 检测循环依赖。
    ///
    /// 使用 DFS 算法检测依赖图中的环。
    /// 如果发现环，返回 Err 包含环中的模块 ID 列表。
    pub fn check_cycles(&self) -> Result<(), Vec<ModuleId>> {
        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();

        for &module_id in self.modules.keys() {
            if !visited.contains(&module_id) {
                if let Err(cycle) = self.dfs_cycle_check(module_id, &mut visiting, &mut visited) {
                    return Err(cycle);
                }
            }
        }

        Ok(())
    }

    /// DFS 循环依赖检测辅助函数。
    fn dfs_cycle_check(
        &self,
        module_id: ModuleId,
        visiting: &mut HashSet<ModuleId>,
        visited: &mut HashSet<ModuleId>,
    ) -> Result<(), Vec<ModuleId>> {
        if visiting.contains(&module_id) {
            // 发现环：重构环路径
            return Err(vec![module_id]);
        }

        if visited.contains(&module_id) {
            return Ok(());
        }

        visiting.insert(module_id);

        if let Some(deps) = self.dependencies.get(&module_id) {
            for &dep in deps {
                match self.dfs_cycle_check(dep, visiting, visited) {
                    Err(mut cycle) => {
                        // 如果当前模块在环中，停止传播
                        if cycle[0] == module_id {
                            return Err(cycle);
                        }
                        // 否则继续向上传播
                        cycle.push(module_id);
                        return Err(cycle);
                    }
                    Ok(()) => {}
                }
            }
        }

        visiting.remove(&module_id);
        visited.insert(module_id);
        Ok(())
    }

    /// 拓扑排序模块。
    ///
    /// 使用 Kahn 算法返回编译顺序：被依赖的模块在前。
    /// 依赖关系：如果 A 依赖 B（A → B），则 B 必须先编译。
    /// 如果有环，返回 Err。
    pub fn topological_sort(&self) -> Result<Vec<ModuleId>, ()> {
        // 计算入度（有多少模块依赖于我）
        let mut in_degree: HashMap<ModuleId, usize> = self.modules.keys().map(|&id| (id, 0)).collect();

        // dependencies[A] = [B, C] 表示 A 依赖 B 和 C
        // 所以 B 和 C 的入度应该增加
        for deps in self.dependencies.values() {
            for &to_module in deps {
                *in_degree.entry(to_module).or_insert(0) += 1;
            }
        }

        // 初始化队列：入度为 0 的模块（没有被任何模块依赖）
        let mut queue: VecDeque<ModuleId> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut sorted = Vec::new();

        while let Some(module_id) = queue.pop_front() {
            sorted.push(module_id);

            // 减少我依赖的模块的入度
            if let Some(deps) = self.dependencies.get(&module_id) {
                for &dep in deps {
                    if let Some(deg) = in_degree.get_mut(&dep) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(dep);
                        }
                    }
                }
            }
        }

        // 检查是否所有模块都处理完
        if sorted.len() == self.modules.len() {
            // 反转结果，使得被依赖的在前
            sorted.reverse();
            Ok(sorted)
        } else {
            Err(())
        }
    }

    /// 获取所有模块 ID。
    pub fn all_modules(&self) -> Vec<ModuleId> {
        self.modules.keys().copied().collect()
    }

    /// 模块数量。
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ===== 诊断辅助函数 =====

use crate::diag::{DiagLoc, DiagSink, Diagnostic, Severity, SubDiag};

/// 发射 E4006 错误：未定义的模块。
pub fn emit_undefined_module(sink: &mut DiagSink, module_name: &str, span: Span) {
    sink.emit(Diagnostic::error(
        4006,
        format!("未定义的模块 `{}`", module_name),
        DiagLoc::At(span),
    ));
}

/// 发射 E4007 错误：未定义的符号（跨模块访问）。
pub fn emit_undefined_symbol(
    sink: &mut DiagSink,
    symbol_name: &str,
    module_name: &str,
    span: Span,
) {
    sink.emit(Diagnostic::error(
        4007,
        format!("模块 `{}` 中未定义符号 `{}`", module_name, symbol_name),
        DiagLoc::At(span),
    ));
}

/// 发射 E4008 错误：私有符号。
pub fn emit_private_symbol(
    sink: &mut DiagSink,
    symbol_name: &str,
    module_name: &str,
    span: Span,
) {
    sink.emit(Diagnostic::error(
        4008,
        format!(
            "符号 `{}` 在模块 `{}` 中是私有的",
            symbol_name, module_name
        ),
        DiagLoc::At(span),
    ).child(SubDiag::new(
        Severity::Help,
        "提示：使用 `pub` 关键字使符号可导出",
    )));
}

/// 发射 E4009 错误：循环依赖。
pub fn emit_circular_dependency(sink: &mut DiagSink, cycle: &[ModuleId], registry: &ModuleRegistry) {
    let module_names: Vec<String> = cycle
        .iter()
        .filter_map(|id| registry.get_module(*id).map(|m| m.name.clone()))
        .collect();

    let cycle_str = module_names.join(" → ");
    sink.emit(Diagnostic::error(
        4009,
        format!("检测到循环依赖: {}", cycle_str),
        DiagLoc::None,
    ).child(SubDiag::new(
        Severity::Help,
        "提示：重新组织模块结构以消除循环引用",
    )));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::FileId;
    fn make_module(_name: &str) -> Module {
        Module {
            items: Vec::new(),
            span: Span::new(FileId(0), 0, 1),
        }
    }

    #[test]
    fn test_register_module() {
        let mut registry = ModuleRegistry::new();
        let path = PathBuf::from("test.kore");
        let ast = make_module("test");

        let id = registry.register_module(path.clone(), "test".into(), ast, Vec::new());
        assert_eq!(id, ModuleId(0));

        // 重复注册返回相同 ID
        let ast2 = make_module("test");
        let id2 = registry.register_module(path.clone(), "test".into(), ast2, Vec::new());
        assert_eq!(id, id2);
    }

    #[test]
    fn test_find_module() {
        let mut registry = ModuleRegistry::new();
        let path = PathBuf::from("test.kore");
        let ast = make_module("test");

        let id = registry.register_module(path.clone(), "test".into(), ast, Vec::new());

        assert_eq!(registry.find_module_by_name("test"), Some(id));
        assert_eq!(registry.find_module_by_path(&path), Some(id));
        assert_eq!(registry.find_module_by_name("nonexistent"), None);
    }

    #[test]
    fn test_no_cycle() {
        let mut registry = ModuleRegistry::new();

        let a_id = registry.register_module(
            PathBuf::from("a.kore"),
            "a".into(),
            make_module("a"),
            Vec::new(),
        );
        let b_id = registry.register_module(
            PathBuf::from("b.kore"),
            "b".into(),
            make_module("b"),
            Vec::new(),
        );

        // a → b
        registry.add_dependency(a_id, b_id);

        assert!(registry.check_cycles().is_ok());
    }

    #[test]
    fn test_detect_cycle() {
        let mut registry = ModuleRegistry::new();

        let a_id = registry.register_module(
            PathBuf::from("a.kore"),
            "a".into(),
            make_module("a"),
            Vec::new(),
        );
        let b_id = registry.register_module(
            PathBuf::from("b.kore"),
            "b".into(),
            make_module("b"),
            Vec::new(),
        );

        // a → b → a (环)
        registry.add_dependency(a_id, b_id);
        registry.add_dependency(b_id, a_id);

        assert!(registry.check_cycles().is_err());
    }

    #[test]
    fn test_topological_sort() {
        let mut registry = ModuleRegistry::new();

        let a_id = registry.register_module(
            PathBuf::from("a.kore"),
            "a".into(),
            make_module("a"),
            Vec::new(),
        );
        let b_id = registry.register_module(
            PathBuf::from("b.kore"),
            "b".into(),
            make_module("b"),
            Vec::new(),
        );
        let c_id = registry.register_module(
            PathBuf::from("c.kore"),
            "c".into(),
            make_module("c"),
            Vec::new(),
        );

        // a → b → c
        registry.add_dependency(a_id, b_id);
        registry.add_dependency(b_id, c_id);

        let sorted = registry.topological_sort().unwrap();

        // c 应该在 b 前面，b 在 a 前面（被依赖的在前）
        let c_pos = sorted.iter().position(|&id| id == c_id).unwrap();
        let b_pos = sorted.iter().position(|&id| id == b_id).unwrap();
        let a_pos = sorted.iter().position(|&id| id == a_id).unwrap();

        assert!(c_pos < b_pos);
        assert!(b_pos < a_pos);
    }
}
