//! ICE 报告路径。ADR 009 Q15：固定缓冲区写 stderr，不分配、不经 `Diag`。
//!
//! 不分配是硬约束而非风格偏好：OOM 是 ICE 的成因之一，报告 OOM 的路径
//! 本身不能依赖分配。所以这里既不用 `format!` 也不用 `String`。

use std::io::Write;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const ISSUE_HINT: &str = "请在 https://github.com/kore-lang/kore/issues 提交 issue";

/// 固定容量的栈上写入器。溢出即截断，不增长。
pub struct FixedBuf<const N: usize> {
    buf: [u8; N],
    len: usize,
    /// 是否发生过截断。用于测试与末尾省略号。
    truncated: bool,
}

impl<const N: usize> FixedBuf<N> {
    pub fn new() -> Self {
        FixedBuf {
            buf: [0u8; N],
            len: 0,
            truncated: false,
        }
    }

    pub fn push_str(&mut self, s: &str) {
        let room = N - self.len;
        let bytes = s.as_bytes();
        if bytes.len() > room {
            // 截断到字符边界，避免写出半个 UTF-8 序列。
            let mut cut = room;
            while cut > 0 && !s.is_char_boundary(cut) {
                cut -= 1;
            }
            self.buf[self.len..self.len + cut].copy_from_slice(&bytes[..cut]);
            self.len += cut;
            self.truncated = true;
        } else {
            self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
            self.len += bytes.len();
        }
    }

    /// 十进制写入无符号整数，不经 `format!`。
    pub fn push_u32(&mut self, mut n: u32) {
        let mut tmp = [0u8; 10];
        let mut i = tmp.len();
        loop {
            i -= 1;
            tmp[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        // tmp[i..] 全是 ASCII 数字，转换必定成功。
        self.push_str(core::str::from_utf8(&tmp[i..]).unwrap());
    }

    pub fn as_str(&self) -> &str {
        // 只通过 push_str / push_u32 写入，且截断对齐字符边界。
        core::str::from_utf8(&self.buf[..self.len]).unwrap()
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<const N: usize> Default for FixedBuf<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// ICE 报告的缓冲区容量。够放位置、版本与提示，不够则截断而非分配。
const ICE_BUF: usize = 1024;

/// 组装 ICE 报告文本。分离出来是为了可测——`report` 只负责写 stderr。
pub fn format_ice(location: &str, msg: &str) -> FixedBuf<ICE_BUF> {
    let mut b = FixedBuf::<ICE_BUF>::new();
    b.push_str("error: internal compiler error: ");
    b.push_str(msg);
    b.push_str("\n  位置: ");
    b.push_str(location);
    b.push_str("\n  版本: kore-stage0 ");
    b.push_str(VERSION);
    b.push_str("\n  ");
    b.push_str(ISSUE_HINT);
    b.push_str("\n");
    b
}

/// 写 ICE 报告到 stderr。忽略写入错误：此时已无从报告。
pub fn report(location: &str, msg: &str) {
    let b = format_ice(location, msg);
    let _ = std::io::stderr().write_all(b.as_str().as_bytes());
    let _ = std::io::stderr().flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_location_version_and_hint() {
        let b = format_ice("src/frontend/parser.rs:120", "unexpected token in block");
        let s = b.as_str();
        assert!(s.contains("internal compiler error"));
        assert!(s.contains("src/frontend/parser.rs:120"));
        assert!(s.contains(VERSION));
        assert!(s.contains("issues"));
    }

    #[test]
    fn truncates_instead_of_growing() {
        let mut b = FixedBuf::<16>::new();
        b.push_str("0123456789abcdef_overflow");
        assert_eq!(b.len(), 16);
        assert!(b.truncated());
    }

    #[test]
    fn truncation_respects_char_boundary() {
        let mut b = FixedBuf::<8>::new();
        // 每个汉字 3 字节，8 字节放不下第三个，必须切在 6 字节处。
        b.push_str("中文字符");
        assert_eq!(b.len(), 6);
        assert_eq!(b.as_str(), "中文");
    }

    #[test]
    fn push_u32_writes_decimal() {
        let mut b = FixedBuf::<32>::new();
        b.push_u32(0);
        b.push_str(" ");
        b.push_u32(4294967295);
        assert_eq!(b.as_str(), "0 4294967295");
    }
}
