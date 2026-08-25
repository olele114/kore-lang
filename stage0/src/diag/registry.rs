//! 错误码登记表。读仓库根的 errors/registry.sexp，支撑 --explain <code>。
//!
//! 格式是 S 表达式而非 TOML：stage1 用 Kore0 重写时，几十行 Kore 就能
//! 读回 S 表达式，而 TOML 会把一个 TOML 解析器塞进 Kore0。

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeStatus {
    Active,
    Retired,
}

#[derive(Debug, Clone)]
pub struct CodeEntry {
    pub code: u16,
    pub status: CodeStatus,
    pub msg: String,
    pub explain: String,
}

#[derive(Debug, Default)]
pub struct Registry {
    entries: BTreeMap<u16, CodeEntry>,
}

#[derive(Debug)]
pub enum RegistryError {
    BadHeader,
    Malformed(String),
    DuplicateCode(u16),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::BadHeader => write!(f, "缺少 `;; kore-errors v1` 头"),
            RegistryError::Malformed(s) => write!(f, "条目格式错误：{s}"),
            RegistryError::DuplicateCode(c) => write!(f, "码号重复：E{c:04}"),
        }
    }
}

impl Registry {
    /// 解析登记表文本。
    ///
    /// 只认三种语法元素：`;;` 注释、`(error …)` 条目、条目内的
    /// `(key value)` 对。value 是裸词或双引号字符串。
    pub fn parse(src: &str) -> Result<Registry, RegistryError> {
        if !src.trim_start().starts_with(";; kore-errors v1") {
            return Err(RegistryError::BadHeader);
        }

        let mut reg = Registry::default();
        for form in split_forms(src) {
            let entry = parse_entry(&form)?;
            if reg.entries.contains_key(&entry.code) {
                return Err(RegistryError::DuplicateCode(entry.code));
            }
            reg.entries.insert(entry.code, entry);
        }
        Ok(reg)
    }

    pub fn get(&self, code: u16) -> Option<&CodeEntry> {
        self.entries.get(&code)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &CodeEntry> {
        self.entries.values()
    }
}

/// 去掉 `;;` 注释后，按顶层括号切出每个 `(error …)` 表单。
fn split_forms(src: &str) -> Vec<String> {
    let mut forms = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    let mut in_str = false;

    for line in src.lines() {
        let line = if in_str { line } else { strip_comment(line) };
        for ch in line.chars() {
            match ch {
                '"' => {
                    in_str = !in_str;
                    if depth > 0 {
                        cur.push(ch);
                    }
                }
                '(' if !in_str => {
                    depth += 1;
                    cur.push(ch);
                }
                ')' if !in_str => {
                    cur.push(ch);
                    depth -= 1;
                    if depth == 0 {
                        forms.push(std::mem::take(&mut cur));
                    }
                }
                _ if depth > 0 => cur.push(ch),
                _ => {}
            }
        }
        if depth > 0 {
            cur.push('\n');
        }
    }
    forms
}

fn strip_comment(line: &str) -> &str {
    match line.find(";;") {
        Some(i) => &line[..i],
        None => line,
    }
}

fn parse_entry(form: &str) -> Result<CodeEntry, RegistryError> {
    let body = form
        .trim()
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| RegistryError::Malformed("非括号表单".into()))?;
    let body = body.trim();
    let rest = body
        .strip_prefix("error")
        .ok_or_else(|| RegistryError::Malformed("顶层表单不是 error".into()))?;

    let mut code = None;
    let mut status = None;
    let mut msg = None;
    let mut explain = None;

    for (key, value) in split_pairs(rest) {
        match key.as_str() {
            "code" => {
                let n: u32 = value
                    .trim()
                    .parse()
                    .map_err(|_| RegistryError::Malformed(format!("码号不是整数：{value}")))?;
                if n > u16::MAX as u32 {
                    return Err(RegistryError::Malformed(format!("码号超出 u16：{n}")));
                }
                code = Some(n as u16);
            }
            "status" => {
                status = Some(match value.trim() {
                    "active" => CodeStatus::Active,
                    "retired" => CodeStatus::Retired,
                    other => {
                        return Err(RegistryError::Malformed(format!("未知 status：{other}")));
                    }
                });
            }
            "msg" => msg = Some(value),
            "explain" => explain = Some(value),
            other => return Err(RegistryError::Malformed(format!("未知字段：{other}"))),
        }
    }

    Ok(CodeEntry {
        code: code.ok_or_else(|| RegistryError::Malformed("缺 code".into()))?,
        status: status.ok_or_else(|| RegistryError::Malformed("缺 status".into()))?,
        msg: msg.ok_or_else(|| RegistryError::Malformed("缺 msg".into()))?,
        explain: explain.ok_or_else(|| RegistryError::Malformed("缺 explain".into()))?,
    })
}

/// 从 `(key value) (key "value")…` 中切出键值对。
fn split_pairs(src: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] != '(' {
            i += 1;
            continue;
        }
        i += 1;
        let mut key = String::new();
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != ')' {
            key.push(chars[i]);
            i += 1;
        }
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < chars.len() && chars[i] == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                value.push(chars[i]);
                i += 1;
            }
            i += 1;
        } else {
            while i < chars.len() && chars[i] != ')' {
                value.push(chars[i]);
                i += 1;
            }
        }
        while i < chars.len() && chars[i] != ')' {
            i += 1;
        }
        i += 1;
        pairs.push((key, value.trim().to_string()));
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = concat!(
        ";; kore-errors v1\n",
        ";; 注释行\n",
        "(error (code 9001) (status active) (msg \"无法读取源文件\") (explain \"长文本\"))\n",
        "(error\n  (code 4001)\n  (status retired)\n  (msg \"m\")\n  (explain \"e\"))\n",
    );

    #[test]
    fn parses_seed_entries() {
        let reg = Registry::parse(SRC).expect("应能解析");
        assert_eq!(reg.len(), 2);
        let e = reg.get(9001).unwrap();
        assert_eq!(e.msg, "无法读取源文件");
        assert_eq!(e.status, CodeStatus::Active);
        assert_eq!(reg.get(4001).unwrap().status, CodeStatus::Retired);
    }

    #[test]
    fn rejects_missing_header() {
        assert!(matches!(
            Registry::parse("(error (code 1) (status active) (msg \"m\") (explain \"e\"))"),
            Err(RegistryError::BadHeader)
        ));
    }

    #[test]
    fn rejects_duplicate_code() {
        let src = format!("{SRC}(error (code 9001) (status active) (msg \"x\") (explain \"y\"))\n");
        assert!(matches!(
            Registry::parse(&src),
            Err(RegistryError::DuplicateCode(9001))
        ));
    }

    #[test]
    fn rejects_five_digit_code() {
        let src =
            ";; kore-errors v1\n(error (code 70000) (status active) (msg \"m\") (explain \"e\"))";
        assert!(matches!(
            Registry::parse(src),
            Err(RegistryError::Malformed(_))
        ));
    }

    #[test]
    fn real_registry_file_parses() {
        // 不断言条目总数：登记表只增不减（ADR 009），锁死数字会让每次
        // 新增错误码都要改这个测试。改为逐码存在性断言。
        let src = include_str!("../../../errors/registry.sexp");
        let reg = Registry::parse(src).expect("仓库登记表应可解析");
        for code in [9001, 9002, 4001, 5001, 5002, 5003] {
            assert!(reg.get(code).is_some(), "登记表缺少 E{code:04}");
        }
        assert!(reg.len() >= 6);
    }
}
