//! 编译期求值环境：存储编译期绑定的值。

use super::value::Value;
use std::collections::HashMap;

/// 编译期环境：作用域栈，每层存储名字到值的映射。
pub struct EvalEnv {
    scopes: Vec<HashMap<String, Value>>,
}

impl Default for EvalEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl EvalEnv {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    /// 进入新作用域。
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// 退出作用域。
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// 在当前作用域定义绑定。
    pub fn define(&mut self, name: String, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, value);
        }
    }

    /// 查找绑定（从最内层向外搜索）。
    pub fn lookup(&self, name: &str) -> Option<&Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(val) = scope.get(name) {
                return Some(val);
            }
        }
        None
    }

    /// 更新已有绑定（用于可变绑定赋值）。
    pub fn update(&mut self, name: &str, value: Value) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn define_and_lookup() {
        let mut env = EvalEnv::new();
        env.define("x".into(), Value::Int(42));
        assert_eq!(env.lookup("x"), Some(&Value::Int(42)));
        assert_eq!(env.lookup("y"), None);
    }

    #[test]
    fn inner_scope_shadows_outer() {
        let mut env = EvalEnv::new();
        env.define("x".into(), Value::Int(10));
        env.push_scope();
        env.define("x".into(), Value::Int(20));
        assert_eq!(env.lookup("x"), Some(&Value::Int(20)));
        env.pop_scope();
        assert_eq!(env.lookup("x"), Some(&Value::Int(10)));
    }

    #[test]
    fn update_modifies_existing_binding() {
        let mut env = EvalEnv::new();
        env.define("x".into(), Value::Int(10));
        assert!(env.update("x", Value::Int(20)));
        assert_eq!(env.lookup("x"), Some(&Value::Int(20)));
    }

    #[test]
    fn update_fails_for_undefined_name() {
        let mut env = EvalEnv::new();
        assert!(!env.update("x", Value::Int(20)));
    }
}
