# ADR 011: 中端 IR 设计

## 状态

提议中（2026-08-08）

## 背景

stage0 前端已完成（词法、语法、resolve、typecheck、escape、eval），需要设计中端（middleend）IR 层，作为 AST 到 LLVM IR 的桥接。关键问题：

1. **IR 形式**：选择 HIR（高级 IR）还是 MIR（中级 IR）？
2. **类型系统**：IR 是否需要独立的类型表示？
3. **降级策略**：哪些 AST 构造在 IR 层展开（如 `defer`、`?` 错误传播）？
4. **CFG 表示**：是否在 IR 层构建显式控制流图？
5. **优化职责**：中端做哪些优化，哪些交给 LLVM？

当前约束：
- **Kore0 子集**：无泛型、无 trait、无闭包
- **stage0 目标**：快速实现可用编译器，优化留给 stage1
- **后端策略**：先用 LLVM 后端（inkwell），后续实现原生后端

## 决策

### 原则

1. **单层 IR** — stage0 只实现一个 IR 层（HIR），直接映射到 LLVM IR，避免多层转换开销
2. **类型擦除** — IR 中保留类型信息用于验证，但简化为扁平类型（原始类型 + 指针 + 聚合）
3. **显式控制流** — `?` 分支、`@` 循环降级为显式 goto + label，便于代码生成
4. **defer 展开** — 在 IR 生成阶段展开 `defer` 为清理代码插入
5. **最小优化** — stage0 不做优化，依赖 LLVM 优化 pass（`-O2`）

### IR 设计

#### 核心结构

```rust
// stage0/src/middleend/hir/mod.rs

pub struct HirModule {
    pub functions: Vec<HirFunction>,
    pub structs: Vec<HirStruct>,
    pub unions: Vec<HirUnion>,
    pub globals: Vec<HirGlobal>,
}

pub struct HirFunction {
    pub name: String,
    pub params: Vec<HirParam>,
    pub ret_type: HirType,
    pub body: HirBody,
    pub span: Span,
}

pub struct HirBody {
    pub blocks: Vec<HirBlock>,      // CFG 基本块
    pub entry_block: BlockId,        // 入口块 ID
}

pub struct HirBlock {
    pub id: BlockId,
    pub stmts: Vec<HirStmt>,
    pub terminator: HirTerminator,   // 块终结符
}

pub enum HirStmt {
    Assign { lhs: HirPlace, rhs: HirRvalue },
    Call { dest: Option<HirPlace>, func: HirOperand, args: Vec<HirOperand> },
    Drop { place: HirPlace },        // 显式析构
}

pub enum HirTerminator {
    Goto(BlockId),
    Return(Option<HirOperand>),
    Switch { discr: HirOperand, targets: Vec<(u64, BlockId)>, otherwise: BlockId },
    Unreachable,
}
```

**关键设计点**：
- **CFG 基本块**：每个 `HirBlock` 对应一个基本块，终结符强制控制流显式化
- **Place vs Operand**：区分左值（Place）和右值（Operand），便于借用检查结果表示
- **显式 Drop**：`defer` 和作用域结束时插入 `Drop` 语句

#### 类型表示

```rust
// stage0/src/middleend/hir/ty.rs

pub enum HirType {
    Void,
    Never,
    Bool,
    Int { width: u8, signed: bool },   // i32, u64 等
    Float { width: u8 },                // f32, f64
    Ptr { pointee: Box<HirType>, owned: bool }, // ^T, own ^T
    Array { elem: Box<HirType>, len: usize },
    Struct { id: StructId },
    Union { id: UnionId },
}
```

**简化点**：
- 无函数类型（Kore0 无闭包）
- 无 trait 对象
- 结构体/联合体通过 ID 引用，避免递归类型问题

#### 表达式降级

```rust
pub enum HirRvalue {
    Use(HirOperand),
    BinaryOp { op: BinOp, lhs: HirOperand, rhs: HirOperand },
    UnaryOp { op: UnOp, operand: HirOperand },
    Ref { place: HirPlace, owned: bool },  // 取引用 ^x
    Deref(HirOperand),
    Aggregate { kind: AggregateKind, fields: Vec<HirOperand> }, // 结构体字面量
}

pub enum HirOperand {
    Const(Const),
    Place(HirPlace),
}

pub enum HirPlace {
    Local(LocalId),
    Field { base: Box<HirPlace>, field: usize },
    Index { base: Box<HirPlace>, index: HirOperand },
    Deref(Box<HirPlace>),
}
```

**要点**：
- 嵌套表达式拆分为临时变量 + 语句序列
- 短路逻辑 `&&` / `||` 降级为条件跳转

### 降级策略

#### 1. 分支 `?` 降级

AST 形式：
```kore
result :: Result<i32, ErrCode> = f()
? result => val { use(val) }
```

HIR 形式：
```
bb0:
  %0 = call f()
  %1 = discriminant %0
  switch %1, [0 -> bb1, 1 -> bb2]

bb1:  // Ok 分支
  %val = extract %0, field 0
  call use(%val)
  goto bb3

bb2:  // Err 分支
  %err = extract %0, field 1
  return %err

bb3:
  ...
```

#### 2. 循环 `@` 降级

AST 形式：
```kore
@ cond {
  ? done => skip
  work()
}
```

HIR 形式：
```
bb0:
  goto bb_loop

bb_loop:
  %cond = load cond
  switch %cond, [true -> bb_body, false -> bb_exit]

bb_body:
  %done = load done
  switch %done, [true -> bb_loop, false -> bb_work]  // skip -> 回到 bb_loop

bb_work:
  call work()
  goto bb_loop

bb_exit:
  ...
```

**关键点**：
- `skip` → `goto loop_header`
- `stop expr` → 赋值到 loop result + `goto loop_exit`

#### 3. `defer` 展开

AST 形式：
```kore
{
  ptr := alloc(size)
  defer free(ptr)
  ? fail => ret Err(code)
  use(ptr)
}
```

HIR 形式：
```
bb0:
  %ptr = call alloc(%size)
  %fail = load fail
  switch %fail, [true -> bb_cleanup, false -> bb_use]

bb_cleanup:
  call free(%ptr)
  %code = load code
  return Err(%code)

bb_use:
  call use(%ptr)
  call free(%ptr)  // 正常退出时也调用清理
  goto bb_next
```

**实现策略**：
- 每个块维护 `defer` 栈
- 块退出时（return / stop / 作用域结束）插入栈中清理代码
- 遇到 `?` 提前返回时，同样触发清理

### 文件结构

```
stage0/src/middleend/
  ├─ hir/
  │   ├─ mod.rs          # HIR 核心数据结构
  │   ├─ ty.rs           # HIR 类型系统
  │   ├─ visitor.rs      # HIR 遍历器（用于验证和转换）
  │   └─ printer.rs      # HIR 调试打印（类似 MIR dump）
  ├─ lower/
  │   ├─ mod.rs          # AST → HIR 降级主入口
  │   ├─ expr.rs         # 表达式降级（拆分嵌套、生成临时变量）
  │   ├─ control.rs      # 控制流降级（分支、循环、defer）
  │   ├─ pattern.rs      # 模式降级为决策树（Kore0 仅简单模式）
  │   └─ scope.rs        # 作用域管理（defer 栈、清理代码插入）
  ├─ validate/
  │   ├─ mod.rs          # HIR 验证器（检查 CFG 完整性）
  │   └─ cfg.rs          # 控制流图验证（无悬空块、终结符完整）
  └─ pass/
      └─ dead_code.rs    # 简单的死代码消除（可选，stage0 可跳过）
```

### 模块职责

| 模块 | 职责 | 依赖 |
|------|------|------|
| `hir/` | 定义 HIR 数据结构和类型系统 | 无（自洽） |
| `lower/` | AST → HIR 转换，控制流显式化 | `frontend/ast`, `hir/` |
| `validate/` | HIR 正确性验证（CFG、类型） | `hir/` |
| `pass/` | 可选优化 pass（stage0 不启用） | `hir/` |

**依赖方向**：
```
frontend/ast  →  middleend/lower  →  middleend/hir  →  backend/llvm
                                        ↓
                               middleend/validate
```

### 与 LLVM IR 的映射

| HIR 构造 | LLVM IR |
|----------|---------|
| `HirBlock` | BasicBlock |
| `HirStmt::Assign` | Store / InsertValue |
| `HirStmt::Call` | Call |
| `HirTerminator::Goto` | Br（无条件跳转） |
| `HirTerminator::Switch` | Switch |
| `HirTerminator::Return` | Ret |
| `HirPlace::Local` | Alloca（栈变量） |
| `HirRvalue::BinaryOp` | Add / Sub / Mul 等 |
| `HirType::Ptr` | Pointer Type |
| `HirType::Struct` | StructType（named） |

**设计理念**：HIR 尽量接近 LLVM IR 语义，减少后端翻译工作。

### Stage0 实现范围

**包含**：
- ✅ AST → HIR 降级（表达式、控制流、defer）
- ✅ HIR 类型系统
- ✅ CFG 验证
- ✅ HIR 调试打印

**排除**（留给 stage1）：
- ❌ 泛型单态化（Kore0 无泛型）
- ❌ Trait 方法派发
- ❌ 复杂模式匹配优化（决策树 vs 回溯）
- ❌ 中端优化 pass（内联、常量传播）

## 后果

### 正面

1. **清晰的抽象层次**：AST（语法） → HIR（控制流） → LLVM IR（机器模型）
2. **便于验证**：显式 CFG 让控制流错误易于检测
3. **可测试性**：HIR 可独立测试（不依赖 LLVM），可序列化为文本格式
4. **后端无关**：未来实现原生后端时，只需替换 `backend/llvm`，复用 HIR
5. **错误定位准确**：HIR 保留 Span 信息，LLVM 报错可回溯到源码

### 负面

1. **额外转换开销**：AST → HIR → LLVM IR 两次转换（但 stage0 编译速度非瓶颈）
2. **维护成本**：新增 HIR 数据结构需与 AST 和 LLVM IR 同步
3. **内存占用**：HIR 拆分嵌套表达式会生成大量临时变量（但编译后释放）

### 实施里程碑

**Phase 1 — HIR 定义与降级**（预估 2 周）
- [ ] 定义 `hir/mod.rs`、`hir/ty.rs` 核心结构
- [ ] 实现 `lower/expr.rs`（表达式拆分、临时变量生成）
- [ ] 实现 `lower/control.rs`（分支、循环降级）
- [ ] 单元测试：验证简单函数降级正确性

**Phase 2 — Defer 与清理**（预估 1 周）
- [ ] 实现 `lower/scope.rs`（defer 栈管理）
- [ ] 在所有退出点插入清理代码
- [ ] 测试：嵌套 defer、提前返回场景

**Phase 3 — HIR 验证与调试**（预估 3 天）
- [ ] 实现 `validate/cfg.rs`（检查 CFG 完整性）
- [ ] 实现 `hir/printer.rs`（类似 `rustc -Z dump-mir`）
- [ ] 集成测试：端到端 AST → HIR 转换

**Phase 4 — 后端集成**（在 backend 设计后）
- [ ] HIR → LLVM IR 翻译器（`backend/llvm/codegen.rs`）
- [ ] 测试：生成可执行文件，验证运行时行为

**总工作量**：约 20-25 天（单人全职）

## 参考

- `docs/adr/007-compiler-module-structure.md` — 编译器模块结构
- `docs/adr/002-expression-statement-boundary.md` — 控制流类型规则
- `docs/spec/02-syntax.md` — Kore 语法规范
- Rust MIR 设计：https://rustc-dev-guide.rust-lang.org/mir/
- LLVM IR 参考：https://llvm.org/docs/LangRef.html

## 后续迭代

完成 stage0 后，stage1 可增强中端：

1. **泛型单态化** — 在 HIR 生成前展开类型参数
2. **Trait 派发** — 静态派发降级为直接函数调用
3. **优化 pass** — 内联、死代码消除、常量传播
4. **借用检查集成** — 在 HIR 层做更精确的生命周期验证

当前设计聚焦 stage0 最小需求，为自举提供坚实基础。
