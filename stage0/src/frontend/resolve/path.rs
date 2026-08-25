//! 路径解析器：将 use 路径解析为文件系统路径。
//!
//! 实现 ADR 004 和 ADR 007 的模块路径规则：
//! - 文件即模块：每个 `.kore` 文件是一个模块
//! - 路径解析：`use a.b.c` → 查找 `./a/b/c.kore`
//! - 相对路径：从当前文件目录开始

use std::path::{Path, PathBuf};

/// 路径解析器。
pub struct PathResolver {
    /// 当前模块的文件目录
    current_dir: PathBuf,
}

impl PathResolver {
    /// 创建新的路径解析器。
    ///
    /// # 参数
    /// - `current_file`: 当前模块的文件路径
    pub fn new(current_file: &Path) -> Self {
        let current_dir = current_file
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        Self { current_dir }
    }

    /// 解析 use 路径到文件系统路径。
    ///
    /// # 示例
    /// ```ignore
    /// // 当前文件：/project/main.kore
    /// // use std.io
    /// // 解析为：/project/std/io.kore
    /// ```
    ///
    /// # 参数
    /// - `segments`: use 路径的各个段（例如 ["std", "io"]）
    ///
    /// # 返回
    /// - `Ok(PathBuf)`: 解析成功的文件路径
    /// - `Err(PathError)`: 解析失败
    pub fn resolve_use_path(&self, segments: &[String]) -> Result<PathBuf, PathError> {
        if segments.is_empty() {
            return Err(PathError::EmptyPath);
        }

        let mut path = self.current_dir.clone();

        // segments[0..n-1] 作为目录
        for segment in &segments[..segments.len() - 1] {
            path.push(segment);
        }

        // segments[n-1] + ".kore" 作为文件名
        let file_name = format!("{}.kore", segments.last().unwrap());
        path.push(file_name);

        Ok(path)
    }

    /// 获取当前目录。
    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }
}

/// 路径解析错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// 空路径
    EmptyPath,
    /// 文件不存在
    FileNotFound(PathBuf),
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathError::EmptyPath => write!(f, "empty use path"),
            PathError::FileNotFound(path) => write!(f, "file not found: {}", path.display()),
        }
    }
}

impl std::error::Error for PathError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_simple_path() {
        let resolver = PathResolver::new(Path::new("/project/main.kore"));
        let result = resolver.resolve_use_path(&vec!["math".to_string()]).unwrap();
        assert_eq!(result, PathBuf::from("/project/math.kore"));
    }

    #[test]
    fn test_resolve_nested_path() {
        let resolver = PathResolver::new(Path::new("/project/main.kore"));
        let result = resolver
            .resolve_use_path(&vec!["std".to_string(), "io".to_string()])
            .unwrap();
        assert_eq!(result, PathBuf::from("/project/std/io.kore"));
    }

    #[test]
    fn test_resolve_deep_nested() {
        let resolver = PathResolver::new(Path::new("/project/src/main.kore"));
        let result = resolver
            .resolve_use_path(&vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
            ])
            .unwrap();
        assert_eq!(result, PathBuf::from("/project/src/a/b/c.kore"));
    }

    #[test]
    fn test_empty_path_error() {
        let resolver = PathResolver::new(Path::new("/project/main.kore"));
        let result = resolver.resolve_use_path(&vec![]);
        assert!(matches!(result, Err(PathError::EmptyPath)));
    }

    #[test]
    fn test_current_dir() {
        let resolver = PathResolver::new(Path::new("/project/src/main.kore"));
        assert_eq!(resolver.current_dir(), Path::new("/project/src"));
    }

    #[test]
    fn test_current_dir_root() {
        let resolver = PathResolver::new(Path::new("main.kore"));
        // "main.kore" 没有父目录，parent() 返回 None，所以变成空字符串
        assert_eq!(resolver.current_dir(), Path::new(""));
    }
}
