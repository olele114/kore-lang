# ADR 007: 编译器模块组织与文件结构

## 状态

已接受（2026-08-02）

## 背景

Kore 语言的设计目标之一是**自举编译器**的可行性。编译器的模块组织直接影响：
1. 自举时的复杂度（循环依赖会阻塞分阶段实现）
2. 代码可读性（自举编译器要解析自己）
3. 构建策略（文件系统即模块系统 vs 需要构建工具）
4. 测试组织（单元测试 vs 集成测试的位置）

本 ADR 通过 22 个问题确定了完整的编译器架构、模块依赖规则、标准库分层、后端策略、自举路径与实现语言选择。

## 决策

### 1. 模块系统基础规则

#### Q1-Q3: 文件与模块的映射

**一个文件 = 一个模块**，文件扩展名为 `.kore`，模块路径即文件系统路径。

```
core/mem.kore       → 模块路径 core.mem
std/io/file.kore    → 模块路径 std.io.file
```

初期不引入包管理器与版本系统，直接用文件系统组织代码。后期构建工具成熟后可引入 workspace 概念，但模块解析规则不变。

#### Q4-Q5: 构建工具策略

**分阶段**：
- **前期**（stage0/stage1）— 手写 `build.sh` 或 `Makefile`，直接调用编译器，文件列表显式
- **后期**（标准库与工具链完善后）— 用 Kore 实现构建工具，支持增量编译、缓存、并行构建

迁移过程**不改变语法**，只是从手动列举文件变为自动依赖分析。

#### Q6: 循环依赖

**禁止循环依赖**，模块依赖图必须是 DAG。

编译器在 `resolve` 阶段检测循环，报编译错误。理由：
- 自举编译器实现简单（拓扑排序即可）
- 强制模块职责清晰，避免"大泥球"架构
- Kore 的 `::` 编译期绑定顺序无关，只需文件间无环即可

### 2. 标准库分层

#### Q7-Q9: 三层架构

```
core/     — freestanding，零依赖，不分配，不做 IO
  ├─ mem.kore      (指针操作、对齐、布局)
  ├─ ops.kore      (算术、位运算 trait)
  ├─ cmp.kore      (相等、排序 trait)
  └─ iter.kore     (迭代器 trait)

alloc/    — 依赖 core + ^Alloc 参数，实现数据结构
  ├─ vec.kore
  ├─ hashmap.kore
  ├─ string.kore
  └─ arena.kore

std/      — 依赖 alloc + 操作系统，提供 IO、线程、文件系统
  ├─ io/
  │   ├─ file.kore
  │   └─ stream.kore
  ├─ thread.kore
  └─ env.kore
```

**模块路径必须反映层次**：
- ✅ `use alloc.hashmap` 
- ❌ `use std.hashmap`（`hashmap` 不在 `std` 层）

**分层规则**：
- `core` 可用于裸机、内核、WASM，零运行时
- `alloc` 需要传入 `^Alloc`，但不假设操作系统
- `std` 假设 POSIX/Win32，提供高层封装

### 3. 编译器目录结构

#### Q10-Q13: 顶层组织

```
compiler/
  ├─ diag/          # 诊断系统（顶层，避免循环依赖）
  ├─ frontend/      # 词法、语法、语义分析
  ├─ middleend/     # IR 优化与变换
  ├─ backend/       # 代码生成
  └─ driver/        # 主协调器
```

**frontend → middleend → backend** 是单向依赖，`driver` 协调整个流程。`diag` 放顶层是因为所有 pass 都需要报告错误，放在 `driver` 或 `frontend` 会造成依赖倒置。

#### Q14-Q15: 后端策略

**自举前**：用 LLVM 后端（通过 Rust 的 `inkwell` 绑定）生成机器码，快速实现可用编译器。

**自举后**：实现原生后端，不依赖外部工具链：

```
backend/
  ├─ llvm/          # 仅用于 stage0 → stage1
  └─ native/        # 自研后端（stage1 之后）
      ├─ common/    # 指令选择、寄存器分配通用框架
      ├─ bare/      # 跨架构裸机支持（启动代码、中断向量）
      ├─ x86_64/    # x86-64 指令编码与 ABI
      ├─ aarch64/   # AArch64（包括 Android）
      └─ elf/       # ELF 文件格式生成
```

**`bare/` 不是第三个架构**，而是 `x86_64` 和 `aarch64` 共享的裸机抽象（页表、GDT、异常处理框架）。

链接策略：
- LLVM 后端内联调用 `llc` + `lld`
- 原生后端直接生成 ELF，不依赖外部链接器

#### Q16-Q20: Frontend 细节

```
frontend/
  ├─ lexer/
  │   ├─ token.kore       # Token 定义
  │   ├─ lexer.kore       # 词法分析器
  │   ├─ keywords.kore    # 23 个关键字表
  │   ├─ token_test.kore
  │   └─ lexer_test.kore
  ├─ parser/
  │   ├─ parser.kore      # 递归下降解析器
  │   ├─ expr.kore        # 表达式解析
  │   ├─ stmt.kore        # 块与绑定
  │   ├─ decl.kore        # 顶层声明
  │   └─ parser_test.kore
  ├─ ast/
  │   ├─ node.kore        # AST 节点定义
  │   ├─ visitor.kore     # 遍历器 trait
  │   └─ printer.kore     # 调试打印
  ├─ resolve/
  │   ├─ scope.kore       # 作用域栈
  │   ├─ resolver.kore    # 名字解析
  │   ├─ symbols.kore     # 符号表（resolve pass 拥有）
  │   └─ import.kore      # use 语句处理
  ├─ typecheck/
  │   ├─ checker.kore     # 类型检查主逻辑
  │   ├─ unify.kore       # 类型统一
  │   ├─ infer.kore       # 类型推断
  │   └─ traits.kore      # trait 解析
  ├─ borrow/
  │   ├─ move.kore        # own 移动检查
  │   └─ escape.kore      # 借用不逃逸检查
  └─ eval/
      ├─ evaluator.kore   # 编译期求值器（AST 解释器）
      ├─ builtin.kore     # 编译期内置函数
      └─ const.kore       # 常量折叠
```

**关键决策**：
- **符号表归属** (Q19)：`resolve` pass 拥有 `SymbolTable`，后续 pass 接收 `^SymbolTable` 借用指针
- **编译期求值位置** (Q16)：在 `frontend/eval/`，因为操作 AST 而非 IR，在类型检查期间执行
- **数据结构拆分** (Q20)：符号表按职责拆分为 `scope.kore`（作用域栈）、`symbols.kore`（符号定义与查询）、`import.kore`（use 处理）

#### Q21: 测试文件位置

**混合方式**：

1. **单元测试** — 与实现文件同目录，命名 `*_test.kore`：
   ```
   frontend/lexer/token_test.kore      # 紧邻 token.kore
   frontend/parser/parser_test.kore
   ```
   - 验证单个模块的函数与数据结构
   - 修改实现时立刻看到对应测试
   - 测试即文档，紧邻实现更易理解

2. **集成测试** — 独立 `tests/` 目录，按功能分类：
   ```
   tests/
     ├─ pipeline/         # 端到端编译流程
     │   ├─ simple_program.kore
     │   └─ error_recovery.kore
     ├─ codegen/          # 代码生成正确性
     │   ├─ llvm_output.kore
     │   └─ native_x64.kore
     └─ diagnostics/      # 错误信息质量
         └─ error_messages.kore
   ```
   - 跨多个模块，验证完整流程
   - 需要构造源文件、调用多个 pass、检查最终输出
   - 不属于任何单一模块，独立组织更清晰

### 4. 自举策略

#### Q22: stage0 实现语言

**Rust**

```
┌─────────────┐
│   stage0    │  Rust 实现，编译 Kore0 子集 → LLVM IR
│  (Rust +    │  - 无 trait、无泛型、无编译期求值
│   inkwell)  │  - 只需实现最小编译器
└──────┬──────┘
       │ 编译
       ↓
┌─────────────┐
│   stage1    │  用 Kore0 写的编译器（可自编译）
│ (Kore0 源码)│  - 实现完整 Kore 语法
└──────┬──────┘
       │ 编译
       ↓
┌─────────────┐
│   stage2    │  stage1 编译完整编译器源码 S 的产物
│             │  - 经自举后端产出，与 stage1 字节不同（正常）
└──────┬──────┘
       │ 编译同一份源码 S
       ↓
┌─────────────┐
│   stage3    │  判据：stage2 == stage3 逐字节相同
│ (自举闭合)  │  - 闭合后 stage0 归档，只维护 Kore 版本
└─────────────┘
```

> **被 ADR 010 (Q4) 修订**：本图原只有三格，stage2 一格写「验证：stage1 编译自己，输出与 stage1 一致」，并把该格标为「自举完成」。该不动点不可满足——stage1 的二进制由 stage0 经 LLVM 后端产出，stage2 的二进制由 stage1 经自举后端产出，两个不同的编译器编译同一份源码没有理由产出相同字节。正确的不动点在下一跳：**stage2 与 stage3 逐字节相同**（stage2 编译源码 S 得 stage3）。这与 `CONTEXT.md` 词汇表已写的判据一致，本图是不同步的一方。ADR 010 第 2 节给出完整链条与理由。

**选择 Rust 的理由**：
1. **类型安全** — `enum` + 模式匹配天然适合 AST/IR，`Result<T, E>` 与 Kore 语义一致
2. **LLVM 绑定成熟** — `inkwell` 可直接生成 IR，避免文本拼接
3. **错误处理强制** — `Option`/`Result` 减少 stage0 自身 bug
4. **生态成熟** — 编译器领域主流（rustc、swc），易吸引贡献者
5. **生命周期短** — 编译慢、二进制大的劣势被摊销（只需开发一次）

**取舍接受**：
- 首次构建慢（~分钟级）→ stage0 冷启动频率低
- 二进制大（~MB 级）→ 不影响 stage1/stage2 体积

**Kore0 子集定义**：
- 有：函数、结构体、联合、指针、数组、基础类型、`?`/`@`、`own`/`defer`
- 无：trait、泛型、编译期求值、动态派发、复杂类型推断

stage1 用 Kore0 实现后，可以编译完整 Kore（包括自己）。

> **被 ADR 010 (Q4) 修订**：原文此处写「此时自举完成」。stage1 能编译自己只是自举链条的中段，不是终点——判据是 **stage2 与 stage3 逐字节相同**，见下方 stage 图的修订说明。

## 后果

### 正面

1. **自举路径清晰** — stage0 (Rust) → stage1 (Kore0) → stage2 → stage3 (字节相等判据)，每阶段目标明确
2. **依赖关系简单** — 单向 DAG，无循环，编译顺序可静态确定
3. **标准库分层** — `core`/`alloc`/`std` 让裸机与应用层代码共享基础抽象
4. **测试组织合理** — 单元测试紧邻实现，集成测试独立分类，职责清晰
5. **后端可替换** — LLVM 用于快速启动，原生后端用于自举后零依赖

### 负面

1. **初期需维护两份代码** — stage0 (Rust) 与 stage1 (Kore0) 在自举前并存
2. **标准库路径迁移** — 现有代码若写 `use std.hashmap` 需改为 `use alloc.hashmap`
3. **测试文件混排** — 单元测试与实现同目录，目录文件数翻倍（但文件名后缀 `_test` 易区分）
4. **LLVM 依赖** — stage0 需要 LLVM 工具链，增加构建环境要求（但仅限自举前）

### 实现里程碑

**Phase 1 — stage0 (Rust)**
- [ ] 词法分析器（23 关键字 + 6 记号）
- [ ] 递归下降解析器（Kore0 语法子集）
- [ ] 名字解析与符号表
- [ ] 类型检查（无泛型、无 trait）
- [ ] LLVM IR 生成（通过 `inkwell`）
- [ ] 链接器集成（调用 `lld`）

**Phase 2 — stage1 (Kore0)**
- [ ] 用 Kore0 重写 stage0 的所有模块
- [ ] 添加 trait 系统（静态派发）
- [ ] 添加泛型（单态化）
- [ ] 添加编译期求值器
- [ ] stage1 编译完整编译器源码，产出 stage2（字节相等判据在 Phase 3，需原生后端就位后比对 stage2 与 stage3）

**Phase 3 — 原生后端**
- [ ] x86-64 指令选择与寄存器分配
- [ ] ELF 文件生成
- [ ] AArch64 后端
- [ ] 裸机目标支持（`bare/`）
- [ ] 移除 LLVM 依赖

**Phase 4 — 工具链完善**
- [ ] 构建工具（依赖分析、增量编译）
- [ ] 包管理器
- [ ] LSP 服务器（IDE 支持）
- [ ] 调试器集成

## 参考

- `docs/spec/01-overview.md` — 六个统一记号，23 关键字
- `docs/spec/02-syntax.md` — `::` vs `:` 的区分
- `docs/adr/002-expression-statement-boundary.md` — `never` 类型与块类型规则
- `docs/adr/004-scope-and-binding.md` — 名字解析规则，`use` 只能在顶层
- `docs/spec/05-memory.md` — `own ^T`、`defer`、显式分配器
