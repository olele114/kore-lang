//! 诊断数据结构。逐字对应 ADR 009 第 43–56 行的 Kore 声明。

/// 文件编号。driver 维护 FileId → 路径的映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

/// 源码区间。ADR 009 要求 12 字节，见本文件末尾的尺寸测试。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    pub file: FileId,
    pub lo: u32,
    pub hi: u32,
}

impl Span {
    pub fn new(file: FileId, lo: u32, hi: u32) -> Self {
        Span { file, lo, hi }
    }

    /// 扩展到另一个 span，合并两者的范围。
    pub fn extend(self, other: Span) -> Self {
        debug_assert_eq!(self.file, other.file);
        Span {
            file: self.file,
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }
}

/// 严重性。与错误码是两个独立字段——码号本身不带严重性字母。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
            Severity::Help => "help",
        }
    }
}

/// 诊断位置。三态：无位置、只知文件、精确到区间。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagLoc {
    None,
    File(FileId),
    At(Span),
}

impl DiagLoc {
    /// 排序键。ADR 009 要求编译结束时按 (file, lo) 排序输出。
    /// 无位置的诊断排在最后。
    pub fn sort_key(&self) -> (u32, u32) {
        match self {
            DiagLoc::None => (u32::MAX, u32::MAX),
            DiagLoc::File(f) => (f.0, 0),
            DiagLoc::At(s) => (s.file.0, s.lo),
        }
    }

    pub fn file(&self) -> Option<FileId> {
        match self {
            DiagLoc::None => None,
            DiagLoc::File(f) => Some(*f),
            DiagLoc::At(s) => Some(s.file),
        }
    }
}

/// 子诊断。span 是可选的：附注可以不指向具体位置。
#[derive(Debug, Clone)]
pub struct SubDiag {
    pub severity: Severity,
    pub msg: String,
    pub span: Option<Span>,
}

impl SubDiag {
    pub fn new(severity: Severity, msg: impl Into<String>) -> Self {
        SubDiag {
            severity,
            msg: msg.into(),
            span: None,
        }
    }

    pub fn at(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: u16,
    pub msg: String,
    pub loc: DiagLoc,
    pub children: Vec<SubDiag>,
    /// 去重后的出现次数。首次记录为 1，后续同 (code, loc) 只累加此计数。
    pub occurrences: u32,
}

impl Diagnostic {
    pub fn new(severity: Severity, code: u16, msg: impl Into<String>, loc: DiagLoc) -> Self {
        Diagnostic {
            severity,
            code,
            msg: msg.into(),
            loc,
            children: Vec::new(),
            occurrences: 1,
        }
    }

    pub fn error(code: u16, msg: impl Into<String>, loc: DiagLoc) -> Self {
        Self::new(Severity::Error, code, msg, loc)
    }

    pub fn warning(code: u16, msg: impl Into<String>, loc: DiagLoc) -> Self {
        Self::new(Severity::Warning, code, msg, loc)
    }

    pub fn child(mut self, sub: SubDiag) -> Self {
        self.children.push(sub);
        self
    }

    /// 去重键。ADR 009 的级联抑制第二层按 (code, loc) 归并。
    pub fn dedup_key(&self) -> (u16, (u32, u32)) {
        (self.code, self.loc.sort_key())
    }

    /// 格式化码号。根据严重性添加前缀：Error=E, Warning=W, Note/Help=I。
    pub fn code_str(&self) -> String {
        let prefix = match self.severity {
            Severity::Error => "E",
            Severity::Warning => "W",
            Severity::Note | Severity::Help => "I",
        };
        format!("{}{:04}", prefix, self.code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_is_twelve_bytes() {
        assert_eq!(core::mem::size_of::<Span>(), 12);
    }

    #[test]
    fn code_is_four_digits() {
        let err = Diagnostic::error(4001, "类型不匹配", DiagLoc::None);
        assert_eq!(err.code_str(), "E4001");

        let warn = Diagnostic::warning(3001, "未使用的变量", DiagLoc::None);
        assert_eq!(warn.code_str(), "W3001");
    }

    #[test]
    fn no_location_sorts_last() {
        let a = DiagLoc::At(Span::new(FileId(9), 0, 1));
        assert!(DiagLoc::None.sort_key() > a.sort_key());
    }
}
