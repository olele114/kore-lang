# ADR 008: 代码风格与命名约定

## 状态

已接受（2026-08-03）

## 背景

Kore 编译器包含两个实现阶段：
1. **stage0** — 用 Rust 实现的最小编译器（编译 Kore0 子集）
2. **stage1+** — 用 Kore 实现的完整编译器（自举后）

为保证代码库的一致性与可维护性，需要统一的命名约定、格式化规范与注释风格。由于 stage0 与 stage1+ 使用不同语言实现，需在「遵循各语言社区惯例」与「保持项目跨阶段一致性」之间取舍。

本 ADR 通过 15 个问题确定了完整的风格规范，优先级为：**Kore 语言自身的命名哲学 > 编译器领域惯例 > 各语言社区惯例**。

## 决策

### 1. 标识符命名

#### 大小写规则

**跨语言统一使用 Kore 风格**，stage0 (Rust) 与 stage1+ (Kore) 命名规则完全一致：

| 类别 | 规则 | 示例 |
|------|------|------|
| 类型（结构体、枚举、trait） | PascalCase | `AstNode`、`TokenKind`、`Allocator` |
| 函数、方法 | snake_case | `parse_expr`、`tokenize`、`type_check` |
| 变量、字段 | snake_case | `token_count`、`~total` |
| 常量（编译期绑定） | snake_case | `MAX_DEPTH :: 64` |
| 模块 | snake_case | `compiler.frontend.lexer` |

**理由**：自举编译器的代码会从 Rust 迁移到 Kore，统一命名风格避免重写时的机械重命名。Kore 的 `::` / `:` 双轨绑定已经区分了编译期与运行期，不需要靠大写标记常量。

#### 缩写规则

**允许行业标准缩写**，禁止项目特定发明：

**✅ 允许的缩写**：
- **编译器通用**：`AST`、`IR`、`CFG`、`SSA`、`ABI`、`ELF`
- **语法树节点**：`expr`、`stmt`、`decl`
- **类型系统**：`ty`（`type` 的标准简写）

**❌ 禁止的缩写**：
- `SymTab` → 应写 `SymbolTable`
- `TyChk` → 应写 `TypeChecker`
- `ResolvCtx` → 应写 `ResolveContext`

**理由**：编译器领域从业者对 AST/IR/expr 这些术语有共同认知，简写不损害可读性；但 `SymTab` 这类项目发明的缩写需要额外记忆，增加认知负担。

#### 枚举变体命名

**不重复类型名前缀**，依赖命名空间消歧：

```rust
// Rust (stage0)
enum Expr {
    Binary(Box<Expr>, BinOp, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Literal(Literal),
}
// 使用：Expr::Binary(...)
```

```kore
-- Kore (stage1+)
Expr :: .Binary(own ^Expr, BinOp, own ^Expr)
      | .Call(own ^Expr, [Expr])
      | .Literal(Literal)
-- 使用：Expr.Binary(...) 或模式匹配 .Binary(...)
```

**理由**：`Expr::ExprBinary` 重复冗余，类型名已经提供命名空间。Rust 与 Kore 的枚举语法已强制带命名空间前缀。

### 2. 方法命名

#### 访问器方法

**不使用 `get_` 前缀**：

```rust
// Rust
impl Token {
    fn kind(&self) -> TokenKind { self.kind }
    fn span(&self) -> Span { self.span }
}
```

```kore
-- Kore
impl Token {
    kind :: (self ^Self) TokenKind => self.kind
    span :: (self ^Self) Span => self.span
}
```

**修改方法规则**：
1. **优先返回可变引用**：`fn kind_mut(&mut self) -> &mut TokenKind`
2. **仅在与 getter 冲突或需要副作用时用 `set_`**：
   ```rust
   fn set_kind(&mut self, k: TokenKind) {
       self.kind = k;
       self.invalidate_cache();  // 副作用
   }
   ```

#### 布尔查询方法

**`is_` + `has_` 混合**：

| 前缀 | 语义 | 示例 |
|------|------|------|
| `is_` | 描述对象的**状态或属性** | `is_empty()`、`is_keyword()`、`is_mutable()` |
| `has_` | 描述对象**包含或拥有**某物 | `has_errors()`、`has_type_annotation()`、`has_next()` |

**区分示例**：
- `token.is_keyword()` — token 本身的性质
- `parser.has_errors()` — parser 持有的错误列表非空
- `node.is_mutable()` — 节点的可变标记
- `node.has_body()` — 节点包含 body 字段

当语义重叠时优先 `is_`（更短且更常见）。

#### 构造函数命名

**`new` + `from_*` + `with_*` 混合**：

```rust
// 主构造器
Lexer::new(input: &str) -> Self

// 类型转换构造器
Expr::from_literal(lit: Literal) -> Self
Type::from_primitive(prim: PrimitiveType) -> Self

// 可选配置（builder 模式）
Lexer::new(input).with_diagnostics(diag)
```

#### 迭代器方法命名

**泛型容器用标准名，领域对象用语义化名字**：

```rust
// 泛型容器 — Rust 标准
Vec<T>::iter()
Vec<T>::iter_mut()
Vec<T>::into_iter()

// 领域对象 — 语义化
Parser::tokens(&self) -> impl Iterator<Item = &Token>
AstNode::children(&self) -> impl Iterator<Item = &AstNode>
SymbolTable::symbols(&self) -> impl Iterator<Item = &Symbol>
```

### 3. 类型系统命名

#### 错误类型

**统一使用 `Err` 后缀**，与 Kore 标准库保持一致：

```rust
enum LexerErr { UnexpectedChar(u8), UnterminatedString }
enum ParseErr { ExpectedToken(TokenKind), InvalidExpr }
enum TypeErr { Mismatch(Type, Type), UnresolvedName(String) }
```

```kore
LexerErr :: .UnexpectedChar(u8) | .UnterminatedString
ParseErr :: .ExpectedToken(TokenKind) | .InvalidExpr
TypeErr :: .Mismatch(Type, Type) | .UnresolvedName(str)
```

**理由**：Kore 标准库已有 `IoErr`、`AllocErr`，编译器错误类型保持一致避免混乱。

#### 类型参数命名

**通用用单字母，特定角色用全称**：

```rust
// 通用类型参数
struct Vec<T> { ... }
enum Result<T, E> { ... }

// 标准角色（业界惯例）
struct HashMap<K, V> { ... }  // K/V = Key/Value
fn parse<I: Iterator>(iter: I) -> Result<Expr, E>

// 特定约束强的参数
fn allocate<Alloc: Allocator>(al: Alloc, size: usize) -> ...
fn parse<Parser: Parse>(p: Parser) -> Expr
```

```kore
-- Kore 对应（方括号泛型）
Vec[T] :: { ... }
HashMap[K, V] :: { ... }
parse[Parser: Parse] :: (p Parser) Expr => ...
```

#### 生命周期参数命名（仅 stage0）

**单一用 `'a`，多个用语义名**：

```rust
// 单一生命周期
fn foo<'a>(x: &'a str) -> &'a str

// 多个生命周期 — 用语义名表明来源
fn parse<'input, 'ctx>(
    input: &'input str,
    ctx: &'ctx mut Context,
) -> Result<Expr<'input>, ParseErr>
```

常用语义名：`'input`（源码）、`'arena`（分配区）、`'ctx`（上下文）、`'ast`（AST 存活期）

### 4. 可见性与私有符号

**私有与公开符号命名规则相同**，仅靠可见性关键字区分：

```rust
// Rust
pub struct Parser { ... }   // 公开
struct TokenCache { ... }   // 私有
```

```kore
-- Kore
pub Parser :: { ... }    -- 公开
TokenCache :: { ... }    -- 私有（无 pub）
```

不使用 `_` 前缀或 `internal` 子模块标记私有符号。

### 5. 注释与文档

#### 文档注释规则

**所有公开 API 必须有文档注释**，实现内部仅在逻辑非显然处加解释性注释。

**Rust (stage0)** — 使用标准文档注释：

```rust
/// Tokenizes the input source into a stream of tokens.
///
/// # Errors
/// Returns `LexerErr::UnexpectedChar` if an invalid byte is encountered.
///
/// # Example
/// ```
/// let tokens = tokenize("x := 42")?;
/// ```
pub fn tokenize(input: &str) -> Result<Vec<Token>, LexerErr> {
    // 跳过 BOM，因为某些编辑器会在 UTF-8 文件开头插入
    if input.starts_with('\u{FEFF}') { ... }
}
```

**Kore (stage1+)** — 使用提案语法：

```kore
--  普通行注释

--- 文档注释，附着于下一个声明
--- 支持 Markdown 格式
---
--- # 错误
--- 失败时返回 `LexerErr.UnexpectedChar`
---
--- # 示例
--- ```kore
--- tokens := tokenize("x := 42", al)!
--- ```
pub tokenize :: (src str, al ^Alloc) [Token] ! LexerErr => {
    -- 跳过 BOM
    ? src.starts_with("\u{FEFF}") => ...
}

--! 模块级文档注释，只允许出现在文件顶部
--! 描述整个模块的职责与依赖
```

**文档注释语法设计**：
- `---` 是 `--` 的自然延伸，解析器在词法阶段多看一个字符
- `--!` 借用 Rust `//!` 的语义（内层文档），区分「描述下一项」与「描述所属模块」
- 渲染工具（`koredoc`）与包管理器同期实现（Phase 4）

> **被 ADR 010 (Q2/Q3) 修订**：本节只定了 `--`、`---`、`--!` 三种形式。ADR 010 另加了 `--~`（测试注解）与 `--=`（契约断言），完整的记号阶梯见 ADR 010 Q3——五种形式都靠 `--` 之后的第三个字符区分。

### 6. 代码组织

#### 导入语句排列

**按来源分组 + 组内字母序**：

**Rust (stage0)** 分组顺序：
```rust
use std::collections::HashMap;
use std::fmt;

use inkwell::context::Context;
use inkwell::module::Module;

use crate::ast::Expr;
use crate::diag::Diagnostic;
use crate::frontend::lexer::Token;
```

**Kore (stage1+)** 分组顺序：
```kore
use core.mem
use core.slice

use alloc.vec

use std.io

use compiler.ast
use compiler.diag
use compiler.frontend.lexer
```

组顺序固定：
1. 标准库第一层（`std` / `core`）
2. 标准库第二层（`alloc`）
3. 外部依赖
4. 本项目模块

组内字母序，组间空行。

#### 函数长度与嵌套深度

**软限制 + 例外白名单**：

**默认目标**：
- 函数 ≤50 行
- 嵌套深度 ≤4 层

**超限行为**：
- 触发 **warning**（非 error）
- 提示考虑拆分，但由开发者判断是否必要

**豁免清单**（单层分派结构不计入限制）：
- `match` / `?` 的臂（每臂 ≤10 行时整体豁免）
- lexer 字符分派表（`? ch is { 'a'..='z' => ..., '0'..='9' => ... }`）
- parser 运算符优先级表
- codegen 指令选择表
- 大型 `enum` 的 `impl` 方法分派

**理由**：编译器的分派表是**数据驱动的平坦结构**，虽长但深度为 1，强行拆分降低可读性。真正需要限制的是**逻辑嵌套**（`? { @ { ? { ... } } }`）才是可读性杀手。

### 7. 格式化

#### 缩进与行宽

**4 空格缩进，100 列行宽**

```rust
// Rust 示例
fn parse_expr(
    input: &str,
    ctx: &mut Context,
) -> Result<Expr, ParseErr> {
    match token.kind {
        TokenKind::LParen => { ... }
        TokenKind::Ident(name) => { ... }
        _ => Err(ParseErr::UnexpectedToken(token)),
    }
}
```

```kore
-- Kore 示例
parse_expr :: (
    input str,
    ctx ~^Context,
) Expr ! ParseErr => {
    ? token.kind is {
        .LParen => { ... }
        .Ident(name) => { ... }
        _ => .Err(.UnexpectedToken(token))
    }
}
```

**理由**：
- **4 空格** — 视觉层次更清晰，编译器代码常有深层 AST 遍历，嵌套结构需易辨识
- **100 列** — 现代宽屏标准，Rust 的长类型签名（`Result<Vec<Token>, LexerErr>`）在 80 列下过于局促

#### 格式化工具

**stage0**：使用 `rustfmt`（Rust 官方工具），配置：
```toml
# rustfmt.toml
edition = "2021"
max_width = 100
tab_spaces = 4
```

**stage1+**：`korefmt` 与包管理器、LSP 同期实现（ADR 007 Phase 4），在此之前手动保持格式一致。

## 后果

### 正面

1. **跨阶段一致性** — stage0 与 stage1+ 命名规则统一，迁移时无需重命名
2. **降低认知负担** — 禁止项目特定缩写，只需记忆编译器领域标准术语
3. **文档强制性** — 公开 API 必须有文档注释，提升代码可维护性
4. **灵活的复杂度限制** — 分派表豁免避免机械拆分，保留编译器代码的自然结构
5. **工具链友好** — stage0 可立即使用 `rustfmt`，stage1+ 的 `korefmt` 有明确规范可遵循

### 负面

1. **Rust 社区惯例偏离** — stage0 不遵循 `rustfmt` 默认配置（类型名用 Kore 风格）
2. **手动格式化负担** — stage1+ 在 `korefmt` 实现前需人工保持 4 空格 / 100 列
3. **文档注释工作量** — 所有公开 API 必须写文档，增加初期开发时间（但长期收益大）
4. **软限制执行** — 函数长度 warning 可能被忽略，需 code review 补充

### 实现检查清单

**Phase 1 — stage0 (Rust) 工具配置**
- [ ] 添加 `rustfmt.toml`（100 列 / 4 空格）
- [ ] 添加 `clippy.toml`（配置函数长度 lint）
- [ ] CI 检查文档注释覆盖率

**Phase 2 — stage1 (Kore) 迁移**
- [ ] 从 Rust 迁移时保持命名风格一致
- [ ] 手动保持 4 空格缩进（在 `korefmt` 实现前）

**Phase 3 — Kore 文档注释实现**
- [ ] 词法分析器按 `--` 之后的第三个字符区分 `---`、`--!`、`--~`、`--=`（后两个见 ADR 010），其余归普通行注释 `--`
- [ ] 解析器将文档注释附着到 AST 节点
- [ ] `koredoc` 工具生成 HTML/Markdown 文档

**Phase 4 — 格式化工具**
- [ ] 实现 `korefmt`（遵循本 ADR 规则）
- [ ] IDE 插件集成自动格式化
- [ ] CI 强制格式检查

## 附录：命名速查表

| 场景 | 规则 | 示例 |
|------|------|------|
| 类型 | PascalCase | `AstNode`、`TokenKind` |
| 函数/变量 | snake_case | `parse_expr`、`token_count` |
| 错误类型 | `*Err` | `LexerErr`、`ParseErr` |
| 访问器 | 无 `get_` 前缀 | `kind()`、`span()` |
| 布尔查询 | `is_*` / `has_*` | `is_empty()`、`has_errors()` |
| 构造器 | `new` / `from_*` / `with_*` | `Lexer::new()`、`Expr::from_literal()` |
| 迭代器 | 泛型 `iter()`，领域语义化 | `Vec::iter()`、`Parser::tokens()` |
| 类型参数 | 通用单字母，特定全称 | `T`、`K/V`、`Allocator` |
| 枚举变体 | 无前缀 | `Expr::Binary` 而非 `Expr::ExprBinary` |
| 缩写 | 仅行业标准 | `AST` ✅、`SymTab` ❌ |

## 参考

- `docs/spec/01-overview.md` — Kore 语法与六个统一记号
- `docs/spec/02-syntax.md` — `::` vs `:` 绑定，`~` 可变标记
- `docs/adr/007-compiler-module-structure.md` — 编译器目录结构，stage0/stage1 定义
- Rust API Guidelines: https://rust-lang.github.io/api-guidelines/naming.html
