//! 作用域栈管理。
//!
//! ADR 004 规定的三层查找链：局部作用域（内→外）→ 模块级 `::` → 内建。
//! 本模块实现前两层：嵌套的局部作用域栈 + 模块级符号表。

use crate::diag::{DiagSink, Diagnostic, DiagLoc, Severity, Span};
use crate::frontend::resolve::symbols::SymbolId;
use std::collections::HashMap;

/// 单个作用域：名字 → SymbolId 映射。
#[derive(Debug, Default)]
pub struct Scope {
    /// 当前作用域中的绑定。
    bindings: HashMap<String, SymbolId>,
}

impl Scope {
    pub fn new() -> Self {
        Self::default()
    }

    /// 在当前作用域中查找名字。
    pub fn lookup(&self, name: &str) -> Option<SymbolId> {
        self.bindings.get(name).copied()
    }

    /// 在当前作用域中插入绑定。
    /// 返回 `Some(旧 SymbolId)` 如果名字已存在（重定义冲突）。
    pub fn insert(&mut self, name: String, id: SymbolId) -> Option<SymbolId> {
        self.bindings.insert(name, id)
    }

    /// 检查名字是否已在当前作用域中定义。
    pub fn contains(&self, name: &str) -> bool {
        self.bindings.contains_key(name)
    }
}

/// 作用域栈：管理嵌套的局部作用域。
///
/// ## 查找顺序（ADR 004 §32-42）
/// 1. 局部作用域栈（内→外）
/// 2. 模块级 `::` 绑定（`module_scope`）
/// 3. 内建（由调用者处理，此处不涉及）
///
/// ## 重定义检测（ADR 004 §51-67）
/// - 同一作用域内重定义：编译错误
/// - 内层作用域遮蔽外层：允许
#[derive(Debug)]
pub struct ScopeStack {
    /// 作用域栈，栈顶是最内层作用域。
    /// `scopes[0]` 是最外层的函数/块作用域（非模块级）。
    scopes: Vec<Scope>,

    /// 模块级 `::` 绑定（独立于局部作用域栈）。
    /// ADR 004 §10-19：`::` 绑定在编译时可见，需两趟扫描。
    module_scope: Scope,
}

impl ScopeStack {
    pub fn new() -> Self {
        Self {
            scopes: Vec::new(),
            module_scope: Scope::new(),
        }
    }

    /// 进入新作用域（函数体、块、循环等）。
    pub fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    /// 退出当前作用域。
    ///
    /// # Panics
    /// 如果作用域栈为空（编译器内部逻辑错误）。
    pub fn pop_scope(&mut self) {
        self.scopes.pop().expect("pop_scope on empty stack");
    }

    /// 在当前作用域中定义名字。
    ///
    /// ## 返回值
    /// - `Ok(())`: 成功插入
    /// - `Err(旧 SymbolId)`: 当前作用域中已存在该名字（重定义冲突）
    ///
    /// ## ADR 004 §51-67
    /// 同一作用域内重定义是编译错误，调用者需发射 E4003 诊断。
    pub fn define_local(&mut self, name: String, id: SymbolId) -> Result<(), SymbolId> {
        let current = self.scopes.last_mut().expect("define_local: no scope");
        match current.insert(name, id) {
            None => Ok(()),
            Some(old_id) => Err(old_id),
        }
    }

    /// 在模块级作用域中定义 `::` 绑定。
    ///
    /// ## 返回值
    /// - `Ok(())`: 成功插入
    /// - `Err(旧 SymbolId)`: 模块级已存在该名字（重定义冲突）
    ///
    /// ## ADR 004 §10-19
    /// 模块级 `::` 绑定需要两趟扫描：
    /// 1. 第一趟：收集所有 `::` 绑定的名字和 span
    /// 2. 第二趟：遍历函数体时可引用模块级名字
    pub fn define_module(&mut self, name: String, id: SymbolId) -> Result<(), SymbolId> {
        match self.module_scope.insert(name, id) {
            None => Ok(()),
            Some(old_id) => Err(old_id),
        }
    }

    /// 查找名字，按 ADR 004 规定的三层顺序。
    ///
    /// ## 查找顺序
    /// 1. 局部作用域栈（内→外）
    /// 2. 模块级 `::` 绑定
    /// 3. 内建（返回 `None`，由调用者检查内建表）
    ///
    /// ## ADR 004 §32-42
    /// 内层作用域遮蔽外层（包括模块级和内建）。
    pub fn lookup(&self, name: &str) -> Option<SymbolId> {
        // 1. 局部作用域栈（从内到外）
        for scope in self.scopes.iter().rev() {
            if let Some(id) = scope.lookup(name) {
                return Some(id);
            }
        }

        // 2. 模块级 `::` 绑定
        if let Some(id) = self.module_scope.lookup(name) {
            return Some(id);
        }

        // 3. 内建（返回 None，由调用者处理）
        None
    }

    /// 仅在模块级作用域中查找（用于 `::` 绑定的前向引用）。
    pub fn lookup_module(&self, name: &str) -> Option<SymbolId> {
        self.module_scope.lookup(name)
    }

    /// 检查当前是否在任何局部作用域中（用于判断是否在函数/块内）。
    pub fn in_local_scope(&self) -> bool {
        !self.scopes.is_empty()
    }

    /// 获取当前作用域深度（0 = 模块级，1+ = 局部作用域）。
    pub fn depth(&self) -> usize {
        self.scopes.len()
    }
}

impl Default for ScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

/// 辅助函数：发射重定义错误诊断（E4003）。
///
/// ADR 004 §51-67：同一作用域内重定义名字是编译错误。
pub fn emit_redefinition_error(
    sink: &mut DiagSink,
    name: &str,
    new_span: Span,
    old_span: Span,
) {
    sink.emit(
        Diagnostic::error(
            4003,
            format!("重定义名字 `{}`", name),
            DiagLoc::At(new_span),
        )
        .child(crate::diag::SubDiag::new(
            Severity::Note,
            "首次定义在此".to_string(),
        ).at(old_span)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_insert_and_lookup() {
        let mut scope = Scope::new();
        let id = SymbolId(42);

        assert_eq!(scope.lookup("x"), None);
        assert_eq!(scope.insert("x".to_string(), id), None);
        assert_eq!(scope.lookup("x"), Some(id));
    }

    #[test]
    fn scope_redefinition_detected() {
        let mut scope = Scope::new();
        let id1 = SymbolId(1);
        let id2 = SymbolId(2);

        assert_eq!(scope.insert("x".to_string(), id1), None);
        assert_eq!(scope.insert("x".to_string(), id2), Some(id1));
    }

    #[test]
    fn scope_stack_nested_shadowing() {
        let mut stack = ScopeStack::new();
        let id1 = SymbolId(1);
        let id2 = SymbolId(2);

        // 外层作用域定义 x
        stack.push_scope();
        assert!(stack.define_local("x".to_string(), id1).is_ok());
        assert_eq!(stack.lookup("x"), Some(id1));

        // 内层作用域遮蔽 x
        stack.push_scope();
        assert!(stack.define_local("x".to_string(), id2).is_ok());
        assert_eq!(stack.lookup("x"), Some(id2)); // 内层遮蔽

        // 退出内层作用域
        stack.pop_scope();
        assert_eq!(stack.lookup("x"), Some(id1)); // 恢复外层
    }

    #[test]
    fn scope_stack_same_scope_redefinition() {
        let mut stack = ScopeStack::new();
        let id1 = SymbolId(1);
        let id2 = SymbolId(2);

        stack.push_scope();
        assert!(stack.define_local("x".to_string(), id1).is_ok());
        assert_eq!(stack.define_local("x".to_string(), id2), Err(id1));
    }

    #[test]
    fn scope_stack_module_level_lookup() {
        let mut stack = ScopeStack::new();
        let id_module = SymbolId(10);
        let id_local = SymbolId(20);

        // 模块级定义 foo
        assert!(stack.define_module("foo".to_string(), id_module).is_ok());
        assert_eq!(stack.lookup("foo"), Some(id_module));

        // 局部作用域遮蔽模块级
        stack.push_scope();
        assert!(stack.define_local("foo".to_string(), id_local).is_ok());
        assert_eq!(stack.lookup("foo"), Some(id_local)); // 局部遮蔽

        stack.pop_scope();
        assert_eq!(stack.lookup("foo"), Some(id_module)); // 恢复模块级
    }

    #[test]
    fn scope_stack_lookup_order() {
        let mut stack = ScopeStack::new();
        let id_module = SymbolId(1);
        let id_outer = SymbolId(2);
        let id_inner = SymbolId(3);

        // 1. 模块级定义 x
        assert!(stack.define_module("x".to_string(), id_module).is_ok());

        // 2. 外层局部作用域定义 x
        stack.push_scope();
        assert!(stack.define_local("x".to_string(), id_outer).is_ok());

        // 3. 内层局部作用域定义 x
        stack.push_scope();
        assert!(stack.define_local("x".to_string(), id_inner).is_ok());

        // 查找应返回最内层
        assert_eq!(stack.lookup("x"), Some(id_inner));

        stack.pop_scope();
        assert_eq!(stack.lookup("x"), Some(id_outer));

        stack.pop_scope();
        assert_eq!(stack.lookup("x"), Some(id_module));
    }

    #[test]
    fn scope_stack_depth() {
        let mut stack = ScopeStack::new();
        assert_eq!(stack.depth(), 0);
        assert!(!stack.in_local_scope());

        stack.push_scope();
        assert_eq!(stack.depth(), 1);
        assert!(stack.in_local_scope());

        stack.push_scope();
        assert_eq!(stack.depth(), 2);

        stack.pop_scope();
        assert_eq!(stack.depth(), 1);

        stack.pop_scope();
        assert_eq!(stack.depth(), 0);
        assert!(!stack.in_local_scope());
    }

    #[test]
    fn module_scope_redefinition() {
        let mut stack = ScopeStack::new();
        let id1 = SymbolId(1);
        let id2 = SymbolId(2);

        assert!(stack.define_module("foo".to_string(), id1).is_ok());
        assert_eq!(stack.define_module("foo".to_string(), id2), Err(id1));
    }

    #[test]
    fn lookup_module_ignores_locals() {
        let mut stack = ScopeStack::new();
        let id_module = SymbolId(1);
        let id_local = SymbolId(2);

        stack.define_module("x".to_string(), id_module).unwrap();

        stack.push_scope();
        stack.define_local("x".to_string(), id_local).unwrap();

        // lookup 返回局部
        assert_eq!(stack.lookup("x"), Some(id_local));

        // lookup_module 仅返回模块级
        assert_eq!(stack.lookup_module("x"), Some(id_module));
    }

    #[test]
    fn scope_empty_lookup_returns_none() {
        let scope = Scope::new();
        assert_eq!(scope.lookup("nonexistent"), None);

        let stack = ScopeStack::new();
        assert_eq!(stack.lookup("nonexistent"), None);
        assert_eq!(stack.lookup_module("nonexistent"), None);
    }

    #[test]
    fn scope_stack_multiple_names() {
        let mut stack = ScopeStack::new();

        stack.push_scope();
        assert!(stack.define_local("x".to_string(), SymbolId(1)).is_ok());
        assert!(stack.define_local("y".to_string(), SymbolId(2)).is_ok());
        assert!(stack.define_local("z".to_string(), SymbolId(3)).is_ok());

        assert_eq!(stack.lookup("x"), Some(SymbolId(1)));
        assert_eq!(stack.lookup("y"), Some(SymbolId(2)));
        assert_eq!(stack.lookup("z"), Some(SymbolId(3)));
        assert_eq!(stack.lookup("w"), None);
    }
}
