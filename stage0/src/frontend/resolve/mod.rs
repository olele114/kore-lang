//! 名字消解。ADR 007 §161–184：将所有标识符绑定到 `SymbolId`，
//! 为后续 pass 提供只读符号表。

mod builder;
pub mod module;
pub mod path;
pub mod scope;
pub mod symbols;

pub use builder::Resolver;
pub use module::{Import, ModuleId, ModuleInfo, ModuleRegistry};
pub use path::{PathError, PathResolver};
pub use scope::{Scope, ScopeStack, emit_redefinition_error};
pub use symbols::{Symbol, SymbolId, SymbolKind, SymbolTable};
