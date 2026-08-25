//! 不逃逸检查的上下文：追踪绑定的所有权状态与作用域深度。

use crate::diag::Span;

/// 绑定的所有权状态。
#[derive(Debug, Clone)]
pub enum OwnershipState {
    /// 可用。
    Live,
    /// 已移动，记录首次移动的位置（用于诊断提示）。
    Moved(Span),
}

/// 绑定的种类。
#[derive(Debug, Clone, PartialEq)]
pub enum BindingKind {
    /// own ^T 类型的绑定，赋值即移动。
    Own,
    /// ^T 类型的借用绑定，不能逃逸。
    Borrow,
    /// 普通绑定，不受检查约束。
    Plain,
}

/// 一个绑定的完整信息。
#[derive(Debug, Clone)]
pub struct BindingInfo {
    pub kind: BindingKind,
    pub state: OwnershipState,
    /// 定义该绑定时的作用域深度（0 = 函数参数层）。
    pub depth: usize,
}

impl BindingInfo {
    pub fn own(depth: usize) -> Self {
        Self { kind: BindingKind::Own, state: OwnershipState::Live, depth }
    }

    pub fn borrow(depth: usize) -> Self {
        Self { kind: BindingKind::Borrow, state: OwnershipState::Live, depth }
    }

    pub fn plain(depth: usize) -> Self {
        Self { kind: BindingKind::Plain, state: OwnershipState::Live, depth }
    }

    pub fn is_moved(&self) -> bool {
        matches!(self.state, OwnershipState::Moved(_))
    }
}

/// 不逃逸检查上下文：作用域栈，每层是一组绑定。
pub struct EscapeContext {
    /// 作用域栈：每个元素是（名字, 绑定信息）的列表。
    scopes: Vec<Vec<(String, BindingInfo)>>,
}

impl Default for EscapeContext {
    fn default() -> Self {
        Self::new()
    }
}

impl EscapeContext {
    pub fn new() -> Self {
        Self { scopes: vec![Vec::new()] }
    }

    /// 当前作用域深度（0-indexed）。
    pub fn depth(&self) -> usize {
        self.scopes.len().saturating_sub(1)
    }

    /// 进入新作用域。
    pub fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    /// 退出作用域。
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// 在当前作用域定义绑定。
    pub fn define(&mut self, name: String, info: BindingInfo) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.push((name, info));
        }
    }

    /// 查找绑定（从最内层向外搜索）。
    pub fn lookup(&self, name: &str) -> Option<&BindingInfo> {
        for scope in self.scopes.iter().rev() {
            for (n, info) in scope.iter().rev() {
                if n == name {
                    return Some(info);
                }
            }
        }
        None
    }

    /// 查找绑定（可变引用，用于更新状态）。
    pub fn lookup_mut(&mut self, name: &str) -> Option<&mut BindingInfo> {
        for scope in self.scopes.iter_mut().rev() {
            for (n, info) in scope.iter_mut().rev() {
                if n == name {
                    return Some(info);
                }
            }
        }
        None
    }

    /// 将某个 own 绑定标记为已移动。
    pub fn mark_moved(&mut self, name: &str, move_site: Span) {
        if let Some(info) = self.lookup_mut(name)
            && info.kind == BindingKind::Own
        {
            info.state = OwnershipState::Moved(move_site);
        }
    }

    /// 对当前所有 own 绑定的所有权状态做快照（用于 branch 前保存）。
    pub fn snapshot_moves(&self) -> Vec<(String, OwnershipState)> {
        let mut snap = Vec::new();
        for scope in &self.scopes {
            for (name, info) in scope {
                if info.kind == BindingKind::Own {
                    snap.push((name.clone(), info.state.clone()));
                }
            }
        }
        snap
    }

    /// 将 own 绑定的状态恢复到快照（仅更新快照中存在的绑定）。
    pub fn restore_snapshot(&mut self, snap: &[(String, OwnershipState)]) {
        for (name, state) in snap {
            if let Some(info) = self.lookup_mut(name) {
                info.state = state.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{FileId, Span};

    fn s() -> Span { Span::new(FileId(0), 0, 0) }

    #[test]
    fn define_and_lookup() {
        let mut ctx = EscapeContext::new();
        ctx.define("x".into(), BindingInfo::own(0));
        assert!(matches!(ctx.lookup("x").unwrap().kind, BindingKind::Own));
        assert!(ctx.lookup("y").is_none());
    }

    #[test]
    fn mark_moved_transitions_state() {
        let mut ctx = EscapeContext::new();
        ctx.define("x".into(), BindingInfo::own(0));
        ctx.mark_moved("x", s());
        assert!(ctx.lookup("x").unwrap().is_moved());
    }

    #[test]
    fn inner_scope_shadows_outer() {
        let mut ctx = EscapeContext::new();
        ctx.define("x".into(), BindingInfo::own(0));
        ctx.push_scope();
        ctx.define("x".into(), BindingInfo::borrow(1));
        assert_eq!(ctx.lookup("x").unwrap().kind, BindingKind::Borrow);
        ctx.pop_scope();
        assert_eq!(ctx.lookup("x").unwrap().kind, BindingKind::Own);
    }

    #[test]
    fn depth_tracks_scope_nesting() {
        let mut ctx = EscapeContext::new();
        assert_eq!(ctx.depth(), 0);
        ctx.push_scope();
        assert_eq!(ctx.depth(), 1);
        ctx.push_scope();
        assert_eq!(ctx.depth(), 2);
        ctx.pop_scope();
        assert_eq!(ctx.depth(), 1);
    }
}
