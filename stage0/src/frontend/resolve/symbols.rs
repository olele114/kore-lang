//! 符号表。ADR 007 第 161–163 行：`resolve` 拥有 `SymbolTable`，后续 pass
//! 只拿 `&SymbolTable`。所以这里只提供不可变查询 + 建表期的插入。

use crate::diag::Span;

/// 符号编号。名字消解后所有引用都指向 `SymbolId`，不再按字符串查。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u32);

/// 符号的种类。Kore0 子集里只有这几类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Func,
    Struct,
    Union,
    /// 结构体字段或联合变体。
    Member,
    Param,
    /// 局部绑定。
    Local,
    /// 模块（通过 use 语句导入）。
    Module(crate::frontend::resolve::module::ModuleId),
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// 声明处。重复定义的诊断要同时指向两处。
    pub span: Span,
    /// 是否带可变标记 `~`。
    pub is_mut: bool,
    /// 是否为公开符号（pub 标记）。
    pub is_public: bool,
}

#[derive(Debug, Default)]
pub struct SymbolTable {
    syms: Vec<Symbol>,
}

impl SymbolTable {
    pub fn new() -> Self {
        SymbolTable { syms: Vec::new() }
    }

    /// 登记一个符号。同名不在这里拦——同名合法与否取决于作用域，由
    /// `scope` 判定。
    pub fn insert(&mut self, sym: Symbol) -> SymbolId {
        let id = SymbolId(self.syms.len() as u32);
        self.syms.push(sym);
        id
    }

    pub fn get(&self, id: SymbolId) -> Option<&Symbol> {
        self.syms.get(id.0 as usize)
    }

    /// 按名字查找符号（用于查找导出符号）。
    pub fn lookup(&self, name: &str) -> Option<SymbolId> {
        self.syms
            .iter()
            .enumerate()
            .find(|(_, s)| s.name == name)
            .map(|(i, _)| SymbolId(i as u32))
    }

    pub fn len(&self) -> usize {
        self.syms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.syms.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (SymbolId, &Symbol)> {
        self.syms
            .iter()
            .enumerate()
            .map(|(i, s)| (SymbolId(i as u32), s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::FileId;

    fn sym(name: &str, kind: SymbolKind) -> Symbol {
        Symbol {
            name: name.into(),
            kind,
            span: Span::new(FileId(0), 0, 1),
            is_mut: false,
            is_public: false,
        }
    }

    #[test]
    fn ids_are_dense_and_stable() {
        let mut t = SymbolTable::new();
        let a = t.insert(sym("a", SymbolKind::Func));
        let b = t.insert(sym("b", SymbolKind::Local));
        assert_eq!(a, SymbolId(0));
        assert_eq!(b, SymbolId(1));
        assert_eq!(t.get(a).unwrap().name, "a");
        assert_eq!(t.get(b).unwrap().kind, SymbolKind::Local);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn same_name_can_be_inserted_twice() {
        // 遮蔽合法性由 scope 判定，表本身不拦。
        let mut t = SymbolTable::new();
        t.insert(sym("x", SymbolKind::Local));
        t.insert(sym("x", SymbolKind::Local));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn out_of_range_id_is_none() {
        let t = SymbolTable::new();
        assert!(t.get(SymbolId(0)).is_none());
        assert!(t.is_empty());
    }
}
