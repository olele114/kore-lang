# Kore0 子集边界确认

**日期**: 2026-08-08  
**状态**: 已确认

## 目的

本文档明确定义 Kore0 子集的范围，区分哪些语言特性属于 stage0 实现范围，哪些推迟到 stage1。

## Kore0 定义（来自 ADR 007）

### ✅ 包含的功能

- **函数**：顶层函数定义，参数、返回类型
- **结构体**：字段定义、初始化、字段访问
- **联合**：变体定义、模式匹配
- **指针**：借用指针 `^T`、所有指针 `own ^T`
- **数组**：固定大小数组、索引访问
- **基础类型**：整数、浮点、布尔、字符串
- **分支 `?`**：守卫、条件链、模式匹配三种形态
- **循环 `@`**：无限循环、条件循环
- **内存管理**：`own`、`defer`

### ❌ 明确排除的功能

- **trait 系统**：trait 定义、impl 块、trait 约束
- **泛型**：类型参数、单态化
- **编译期求值**（复杂形式）：只保留基础常量折叠
- **动态派发**：trait 对象、vtable
- **复杂类型推断**：需要 Hindley-Milner 的场景
- **闭包 `\`**：需要捕获环境，涉及闭包对象生成
- **管道 `|>`**：语法糖，依赖函数类型系统
- **范围表达式** `..` / `..=`：涉及迭代器协议

## 当前 stage0 实现状态

### 已实现的核心 Pass

1. **词法分析器** (`frontend/lexer/`)
   - 23 个关键字
   - 运算符和分隔符
   - 注释和字符串转义

2. **语法分析器** (`frontend/parser/`)
   - 递归下降解析
   - AST 构建
   - 错误恢复

3. **名称解析** (`frontend/resolve/`)
   - 符号表构建
   - 作用域管理
   - 前向引用处理

4. **类型检查** (`frontend/typecheck/`)
   - 类型推断
   - 类型一致性验证
   - 错误联合检查

5. **逃逸分析** (`frontend/escape/`)
   - 借用指针生命周期检查
   - 逃逸检测

6. **编译期求值** (`frontend/eval/`)
   - 常量折叠
   - 编译期绑定求值（基础）

### AST 支持的表达式（`frontend/ast/node.rs`）

```rust
pub enum Expr {
    // 字面量
    Int, Float, Str, Bool, Nil,
    
    // 路径与调用
    Path, Call, Field, Index,
    
    // 控制流
    Branch { scrutinee: Option<Box<Expr>>, arms },
    Loop { subject: Option<Box<Expr>>, body },
    
    // 运算
    Unary, Binary, Deref, Propagate,
    
    // 块与跳转
    Block, Ret, Stop, Skip, Jmp,
}
```

**关键观察**：
- ✅ `Branch` 支持三种形态（scrutinee 为 None 时是守卫/条件链）
- ✅ `Loop` 有 `subject` 字段，可表示条件循环
- ❌ 无 `Lambda` / `Closure` 变体
- ❌ 无 `Pipeline` 变体
- ❌ 无 `Range` 表达式变体

### 循环解析器实现（`frontend/parser/expr.rs:308-333`）

```rust
pub fn parse_loop(p: &mut Parser) -> Expr {
    let start = p.bump(); // 吃掉 @

    // @ { body } 无限循环
    if matches!(p.peek(), TokenKind::Punct("{")) {
        let body = parse_block(p);
        return Expr::Loop { subject: None, body: Box::new(body), span };
    }

    // @ cond { body } 条件循环
    let subject = parse_expr_bp(p, 10);
    let body = parse_block(p);
    
    Expr::Loop { subject: Some(Box::new(subject)), body: Box::new(body), span }
}
```

**关键观察**：
- ✅ 支持 `@ { body }`（无限循环）
- ✅ 支持 `@ cond { body }`（条件循环）
- ❌ 不支持 `@ 0..n => i { body }`（范围循环）
- ❌ 不支持 `@ items => it { body }`（迭代循环）

解析器注释写："三种形态：条件循环、范围循环、迭代"，但实际只实现了前两种的简化版。

### 测试覆盖（`tests/unit/parser.rs`）

```rust
#[test]
fn expr_loop_infinite() {
    let body = func_body("f :: () => @ { 1 }");
    assert!(matches!(body, Expr::Loop { subject: None, .. }));
}

#[test]
fn expr_loop_with_condition() {
    let body = func_body("f :: () => @ cond { 1 }");
    assert!(matches!(body, Expr::Loop { subject: Some(_), .. }));
}
```

**关键观察**：
- ✅ 测试覆盖了无限循环和条件循环
- ❌ 无范围循环测试
- ❌ 无迭代循环测试

## 边界确认结论

### Stage0 (Kore0) 应实现的功能

基于 ADR 007 的定义和当前实现，Kore0 的循环支持应限定为：

1. **无限循环**：`@ { body }`
2. **条件循环**：`@ cond { body }`

**不包括**：
- 范围循环 `@ 0..n => i { body }`（需要范围表达式和绑定变量）
- 迭代循环 `@ items => it { body }`（需要迭代器 trait）

### Stage1 (完整 Kore) 才实现的功能

1. **闭包** `\x => x * 2`
   - 理由：需要环境捕获、闭包对象生成、函数类型系统
   
2. **管道** `|>`
   - 理由：语法糖，依赖完善的函数类型系统和闭包

3. **范围循环** `@ 0..n => i { body }`
   - 理由：需要范围表达式类型、迭代器协议、绑定变量支持
   - 涉及 AST 扩展：`Loop` 需要 `binding` 字段

4. **迭代循环** `@ items => it { body }`
   - 理由：需要 `Iter` trait、trait 方法调用

## 实现建议

### Stage0 前端已完成

当前 stage0 前端对于 Kore0 子集已基本完成：

- ✅ 词法、语法、语义分析三大 pass
- ✅ 90% 测试覆盖率
- ✅ 性能基准测试框架
- ✅ CI 流水线

### Stage0 下一步：后端实现

根据 ADR 007 Phase 1 检查清单，当前应开始：

1. **LLVM IR 生成** (`backend/codegen/`)
   - 函数编译
   - 表达式求值
   - 控制流转换（分支、循环）
   - 内存管理（own、defer 的 LLVM 表示）

2. **链接器集成**
   - 调用 `lld` 生成可执行文件
   - 处理外部符号

### Stage1 扩展清单

在 stage0 完成后，stage1 需要添加（按优先级）：

1. **泛型与单态化** (P0)
2. **Trait 系统** (P0)
3. **范围表达式与迭代循环** (P1)
4. **闭包与捕获** (P1)
5. **管道语法糖** (P2)

## 参考文档

- `docs/adr/007-compiler-module-structure.md` — Kore0 子集定义
- `docs/spec/02-syntax.md` — 完整 Kore 语法规范
- `CONTEXT.md` — 词汇表和术语
- `stage0/src/frontend/ast/node.rs` — AST 定义
- `stage0/src/frontend/parser/expr.rs` — 表达式解析器
