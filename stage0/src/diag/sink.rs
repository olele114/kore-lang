//! 诊断汇集器。ADR 009：sink 只累积与计数，不做渲染。

use super::diagnostic::{Diagnostic, Severity};

pub struct DiagSink {
    diags: Vec<Diagnostic>,
    /// 错误计数。ADR 009 的 pass 闸门读这个字段。
    /// 注意：--error-limit 的节流不改变它。
    err_count: u32,
    warn_count: u32,
    /// --error-limit。None 表示不节流。
    error_limit: Option<u32>,
    /// 被节流掉的错误条数，用于末尾提示。
    suppressed: u32,
}

impl DiagSink {
    pub fn new() -> Self {
        DiagSink {
            diags: Vec::new(),
            err_count: 0,
            warn_count: 0,
            error_limit: None,
            suppressed: 0,
        }
    }

    pub fn with_error_limit(limit: Option<u32>) -> Self {
        let mut s = Self::new();
        s.error_limit = limit;
        s
    }

    /// 记录一条诊断。
    ///
    /// 两件事必须分开：计数总是发生（否则闸门会漏），存储可能被
    /// --error-limit 节流掉。级联抑制第二层在这里按 (code, loc) 归并。
    pub fn emit(&mut self, diag: Diagnostic) {
        match diag.severity {
            Severity::Error => self.err_count += 1,
            Severity::Warning => self.warn_count += 1,
            _ => {}
        }

        if let Some(existing) = self
            .diags
            .iter_mut()
            .find(|d| d.dedup_key() == diag.dedup_key())
        {
            existing.occurrences += 1;
            return;
        }

        if diag.severity == Severity::Error
            && let Some(limit) = self.error_limit
            && self.stored_errors() >= limit
        {
            self.suppressed += 1;
            return;
        }

        self.diags.push(diag);
    }

    fn stored_errors(&self) -> u32 {
        self.diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count() as u32
    }

    /// ADR 009 的闸门判据：有错就不进下一个 pass。
    pub fn has_errors(&self) -> bool {
        self.err_count > 0
    }

    pub fn err_count(&self) -> u32 {
        self.err_count
    }

    pub fn warn_count(&self) -> u32 {
        self.warn_count
    }

    pub fn suppressed(&self) -> u32 {
        self.suppressed
    }

    /// 取出诊断，按 (file, lo) 排序。ADR 009 要求排序发生在编译结束时，
    /// 而不是在产生顺序上。
    pub fn finish(mut self) -> Vec<Diagnostic> {
        self.diags.sort_by_key(|d| d.loc.sort_key());
        self.diags
    }

    /// 不消耗 sink 的只读视图，供中途检视。
    pub fn peek(&self) -> &[Diagnostic] {
        &self.diags
    }
}

impl Default for DiagSink {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::super::diagnostic::{DiagLoc, FileId, Span};
    use super::*;

    fn at(file: u32, lo: u32) -> DiagLoc {
        DiagLoc::At(Span::new(FileId(file), lo, lo + 1))
    }

    #[test]
    fn throttling_does_not_change_err_count() {
        let mut sink = DiagSink::with_error_limit(Some(2));
        for i in 0..10 {
            sink.emit(Diagnostic::error(2001, "语法错误", at(0, i)));
        }
        // 计数是全量的，闸门看到的是真实错误数。
        assert_eq!(sink.err_count(), 10);
        // 存储被节流到上限。
        assert_eq!(sink.peek().len(), 2);
        assert_eq!(sink.suppressed(), 8);
    }

    #[test]
    fn dedup_counts_occurrences() {
        let mut sink = DiagSink::new();
        for _ in 0..4 {
            sink.emit(Diagnostic::error(4001, "类型不匹配", at(1, 100)));
        }
        assert_eq!(sink.peek().len(), 1);
        assert_eq!(sink.peek()[0].occurrences, 4);
        // 去重不吞计数。
        assert_eq!(sink.err_count(), 4);
    }

    #[test]
    fn finish_sorts_by_file_then_offset() {
        let mut sink = DiagSink::new();
        sink.emit(Diagnostic::error(1001, "c", at(2, 5)));
        sink.emit(Diagnostic::error(1002, "a", at(0, 90)));
        sink.emit(Diagnostic::error(1003, "b", at(0, 10)));
        let out = sink.finish();
        let keys: Vec<(u32, u32)> = out.iter().map(|d| d.loc.sort_key()).collect();
        assert_eq!(keys, vec![(0, 10), (0, 90), (2, 5)]);
    }

    #[test]
    fn warnings_are_not_throttled_by_error_limit() {
        let mut sink = DiagSink::with_error_limit(Some(1));
        sink.emit(Diagnostic::error(2001, "e", at(0, 0)));
        sink.emit(Diagnostic::warning(2002, "w", at(0, 1)));
        sink.emit(Diagnostic::warning(2003, "w", at(0, 2)));
        assert_eq!(sink.peek().len(), 3);
        assert_eq!(sink.warn_count(), 2);
        assert!(sink.has_errors());
    }
}
