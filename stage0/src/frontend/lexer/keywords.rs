//! 关键字全表。逐条对应 `docs/spec/01-overview.md` 第 3 节的 23 个关键字。
//!
//! `true` `false` `nil` `self` `Self` 不在此表：规范把它们定为字面量与内置
//! 名字，不计入关键字，所以它们走标识符路径。

/// 关键字全表，按规范的分组顺序排列。顺序即分组，改动时对照规范表。
pub const KEYWORDS: [&str; 23] = [
    // 声明
    "pub", "impl", "trait", "use",
    // 类型
    "own", "vol", "dyn", "as", "is",
    // 控制
    "ret", "jmp", "stop", "skip", "defer",
    // 逻辑
    "and", "or", "not",
    // 位运算
    "xor", "inv", "rol", "ror",
    // 其他
    "asm", "unsafe",
];

/// 判定一个标识符形状的词是否为关键字。
///
/// 线性扫描而非哈希表：23 个词的线性扫描比建表快，且 stage1 要把这段机械
/// 重写成 Kore0，Kore0 没有哈希表。
pub fn is_keyword(word: &str) -> bool {
    let mut i = 0;
    while i < KEYWORDS.len() {
        if KEYWORDS[i] == word {
            return true;
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_exactly_23_keywords() {
        // 规范 01-overview.md 第 3 节：「共 23 个」。数量变化必须先改规范。
        assert_eq!(KEYWORDS.len(), 23);
    }

    #[test]
    fn table_has_no_duplicates() {
        for (i, &kw_i) in KEYWORDS.iter().enumerate() {
            for &kw_j in KEYWORDS.iter().skip(i + 1) {
                assert_ne!(kw_i, kw_j, "重复关键字：{}", kw_i);
            }
        }
    }

    #[test]
    fn literals_and_builtin_names_are_not_keywords() {
        for w in ["true", "false", "nil", "self", "Self"] {
            assert!(!is_keyword(w), "{w} 是字面量或内置名字，不该进关键字表");
        }
    }

    #[test]
    fn absent_words_from_other_languages_are_not_keywords() {
        // Kore 靠 `?`/`@` 吞掉整族关键字，这些词不该存在。
        for w in ["if", "else", "match", "switch", "for", "while", "enum", "const", "mut", "fn"] {
            assert!(!is_keyword(w), "{w} 不该是 Kore 关键字");
        }
    }

    #[test]
    fn every_table_entry_is_recognized() {
        for w in KEYWORDS {
            assert!(is_keyword(w));
        }
    }
}
