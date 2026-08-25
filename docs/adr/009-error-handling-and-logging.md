# ADR 009: 编译器错误处理与日志规范

## 状态

已接受（2026-08-04）

## 背景

ADR 007 定下了编译器的模块结构与 pipeline，ADR 008 定下了代码风格。但「编译器自身如何报错、如何被观测」还是空白，而这套规范会渗透到几乎每个 pass 的函数签名里，必须在大规模实现开始前定死。

需要回答的问题分四类：

1. **对外诊断** — 用户看到的编译错误长什么样、怎么传递、什么时候停下来
2. **对内故障** — 编译器自己出 bug（ICE）、断言失败、分配失败时怎么办
3. **可观测性** — 开发编译器的人怎么看中间产物、怎么测性能
4. **契约稳定性** — 错误码一旦发布就是公共接口，怎么管

本 ADR 通过 25 个问题确定完整规范。裁决优先级沿用 `docs/spec/01-overview.md` 的「可预测性优先于便利性」，并补一条编译器特有的：**编译器的正确性优先于编译器的速度**。

## 决策

### 原则

1. **双通道分离** — 「用户代码有问题」与「编译器有问题」是两条独立路径，不共用类型、不共用输出机制。
2. **诊断是数据，不是字符串** — 诊断以结构体形式产出并累积，渲染是最后一步的独立关注点。
3. **计数与渲染分离** — 任何输出选项都不得改变编译流程。
4. **转储优先于日志** — 编译器的主要可观测性手段是结构化中间产物，不是过程性日志。
5. **无决策余地的失败不走可失败返回** — `T ! E` 用于调用方有决策余地的场合;否则 panic。

### 1. 对外诊断

#### 双通道（Q1–Q2）

| 场景 | 机制 |
| --- | --- |
| pass 内部「这一步无法继续，调用方有决策余地」 | `T ! E` 可失败返回 |
| 「用户代码有问题，需要报告给用户」 | 显式传递的 `~^Diag` sink |

`~^Diag` 显式传参，不用全局单例——与 `^Alloc` 的显式传递风格一致，且并行编译不需要加锁。

#### 数据结构（Q3–Q5, Q22）

```kore
Severity :: .Error | .Warning | .Note | .Help

DiagLoc :: .None | .File(FileId) | .At(Span)

Span :: { file FileId, lo u32, hi u32 }        -- 12 字节

Diagnostic :: {
    severity Severity,
    code     u16,
    msg      str,
    loc      DiagLoc,
    children [SubDiag],
}
```

`severity` 与 `code` 是**独立字段**，码里不含严重级。`error[E4001]` 里的 `error` 字样来自 `severity`。这个解耦是为了让同一诊断在不同 lint 级别下改变严重性——`unused_variable` 默认 warning，`-D unused` 下是 error，同一个问题、同一个码。若把 `W` 烧进码里，级别提升就得换码，用户的 `#allow` 标注会失效。

`DiagLoc` 三档对应三种渲染形态，这是 `Span?` 表达不了的：

```
error[E9001]: cannot read `src/foo.kore`: permission denied

error[E9002]: `src/foo.kore` is not valid UTF-8
  --> src/foo.kore

error[E4001]: expected `i32`, found `str`
  --> src/main.kore:14:9
   |
14 |     take_int(name)
   |              ^^^^ this is `str`
```

不用「合成 Span」（`lo: 0, hi: 0` + 保留 `FileId::NONE`）的理由：渲染器无法从类型上区分「真的指向第 0 字节」和「其实没有位置」，只能靠约定，而约定破裂的后果是去切一个未加载的源文件。用和类型换成穷尽匹配能检查的东西，正是 `?...is` 存在的意义。

不把 IO / 链接 / 参数错误分离到独立 `DriverErr` 通道的理由：「文件读不到」必须能通过 JSON 渠道到达 IDE，也必须计入 `err_count` 以触发 pass 门禁——文件没读进来，后续 pass 一步都不该跑。分离出去就得在 driver 里再写一套并行的计数与门禁。

`SubDiag.span` 保持 `Span?`：子诊断只有「指向另一处」和「纯文字」两种，不需要文件级那一档。

#### 门禁（Q6）

`err_count > 0` 时不进入下一个 pass。pass 内部继续收集诊断以便一次报告多个问题;pass 之间硬性阻断，避免在已知错误的 AST 上做后续推理。

#### 渲染与输出格式（Q7）

`--error-format=human|json|short`。sink 只负责累积与计数，三个 renderer 独立消费同一份 `[Diagnostic]`。JSON 输出带 `"version": 1` 作为演化锚点。

#### 诊断顺序（Q14）

诊断在编译结束时统一按 `(file, lo)` 排序后输出，不按产生顺序。理由：并行编译下产生顺序不确定，而快照测试与用户体验都要求稳定顺序。

#### 级联抑制（Q25）

三层机制叠加：

1. **`.Error` 毒化类型** — 出错处产出 `.Error`，它和任意类型统一成功且不再报错。把「这里已经报过了」编码进类型系统，而不是靠调用方到处判断。没有它，一个未定义的 `foo` 会在它出现的每个表达式、每次统一、每个字段访问上各报一次。
2. **按 `(code, loc)` 去重** — 保留出现次数，渲染成 `(此错误在 7 个实例化中出现)`。
3. **`--error-limit=N`** — 默认 100，`0` 表示不限;超限后打印 `error: too many errors, stopping`。

去重在别的语言里是锦上添花，**在 Kore 里是刚需**：泛型单态化与编译期求值会系统性复制诊断，一个泛型函数体里的错误会在每个实例化点重复一次。去重键用 `(code, loc)` 而非整条消息，因为消息里可能带实例化的具体类型（`expected i32, found str` vs `expected u8, found str`），文本不同但根因是泛型体里的同一行。

**限流发生在 sink 层，且不得影响 `err_count`。** 若超限丢弃的诊断也不计数，`--error-limit` 就会改变 pass 门禁行为——一个纯输出选项影响编译流程，可预测性原则不接受。限流对所有 renderer 统一生效，不为 JSON 开特例，否则 IDE 与终端看到的 `err_count` 会不一致。

不采用 pass 内 fail-fast 的理由：stage1 自编译一次不便宜，每次只拿一个错误意味着修 N 个错误要跑 N 遍完整编译。

### 2. 对内故障

#### ICE（Q15）

ICE 路径用**固定缓冲区**写 stderr，不经过 `Diag`、不分配。理由：OOM 是 ICE 的成因之一，报告 OOM 的路径本身不能依赖分配。输出包含 panic 位置、编译器版本、以及提交 issue 的提示。

#### 断言三档（Q17）

| 名字 | debug | release | 触发时 |
| --- | --- | --- | --- |
| `assert(cond, msg)` | 检查 | **保留检查** | panic |
| `debug_assert(cond, msg)` | 检查 | 移除 | debug panic / release 无操作 |
| `unreachable(msg)` | 检查 | 移除并告知优化器 | debug panic / release **UB** |

`unreachable` 不是 `debug_assert(false)` 的别名——它向优化器断言该分支不存在，这个信息换来边界检查消除与更紧凑的跳转表。穷尽匹配也需要一个标准写法表达「这个变体不可能到达」。

两条约束：`unreachable` 只允许出现在 `unsafe` 块内（release 下是 UB，危险必须在调用点可见）;编译器自身统一用保留检查的 `assert`，只在实测证明是热路径处才降级为 `debug_assert`——编译器的正确性优先于它的速度，ICE 永远好过静默产出错误代码。

#### 分配失败（Q23）

| 分配场景 | 策略 |
| --- | --- |
| AST 节点、符号表条目、类型表条目（小而密集，规模不可预知） | die-on-failure，panic |
| 读取整个源文件、mmap 目标文件（规模由输入直接决定且事先 `stat` 可知） | `T ! E`，走正常诊断 |

判据：**分配规模是否由输入直接决定且事先可知。**

不做全程 `!` 传播的理由：编译是全有或全无的批处理，OOM 时产不出部分结果，唯一正确动作是退出。成千上万个 `!` 汇聚到 driver 也只是打一行错误退出，可观察行为与 panic 一致,中间那一万个 `!` 没换来任何决策能力。这违背「`T ! E` 用于调用方有决策余地的场合」。

**边界（必读）：这是编译器这个应用的策略，不是 `alloc` 层的策略。** 标准库 `alloc` 保持 `T ! AllocErr` 可失败，内核与嵌入式必须能处理分配失败;编译器只是在自己代码里用 die-on-failure 包装它。一个是设施，一个是使用者的政策。

LSP 场景的顾虑不足以推翻：语言服务器按进程隔离部署是常规做法，编辑器会重启崩掉的 LSP 进程。不做 `--oom-behavior` 运行时开关：OOM 行为影响的是整个代码库的写法（是否到处写 `!`），编译期就该定死。

### 3. 可观测性

#### 转储为主，日志为辅（Q18）

| 需求 | 手段 |
| --- | --- |
| 某个 pass 的输出对不对 | `--emit=<stage>` |
| 自举一致性验证 | 转储逐字节比对 |
| 快照测试 | 转储进 `tests/` |
| 类型推断 / borrow 检查的推理链 | `~^Log` + `trace`/`debug` |

`~^Log` 只出现在 `typecheck/` 与 `borrow/` 的签名里，`trace`/`debug` 两档在 release 下经编译期求值整体消失，连参数都不求值。其余模块不带 `~^Log`——这是有意的边界，避免它像 `~^Diag` 那样蔓延全局。

不用全局 logger：在显式传递 `^Alloc` / `~^Diag` 的体系里是异物;更实际的是日志一旦全局可写，并行编译就要加锁，而锁会扰乱它本要观测的时序。

#### 转储格式（Q19）

阶段：`tokens`、`ast`、`resolved`、`typed`、`ir`、`llvm-ir`、`asm`、`obj`。统一 S-表达式：

```
;; kore-dump v1 stage=ast file=src/main.kore
(fn main
  (params)
  (ret void)
  (block
    (bind x (int 42))
    (call io.out.print (str "hi"))))
```

三条约束：

- **默认剔除 Span，`--emit-spans` 才带。** 这是快照测试可用的前提。源文件插一个空行会改变所有后续字节偏移，若 Span 在默认输出里，整个快照会因结构零改动的编辑而失效。剔掉后快照只对结构敏感,Span 正确性由少数专门用例覆盖。
- **头部带格式版本号**，与 JSON `"version": 1` 同一考虑。
- **S-表达式而非 JSON**：自举阶段可以用几十行 Kore 把转储解析回来做 AST → dump → AST 的 round-trip 检查;同样的事用 JSON 要先写一个完整 JSON 解析器。

#### 计时与统计（Q20）

`--time-passes` 给墙钟时间（driver 已是唯一 pass 编排点，实现十几行）;`--stats` 给计数指标：

| 指标 | 用途 |
| --- | --- |
| token 数 / AST 节点数 | 输入规模基线 |
| **编译期求值步数** | 编译期无限循环 / 失控递归的直接观测 |
| **单态化实例数** | 泛型组合爆炸的直接观测 |
| unify 次数 / 类型变量数 | 类型推断退化的观测 |
| 生成指令数 | 后端输出规模 |

加粗项针对 Kore 特有风险：这门语言把泛型与代码生成全压在编译期求值上，「编译突然变慢」最可能的根因是某个编译期循环或单态化展开失控，而墙钟时间只能定位到 `eval` 或 `middleend` 这一层，说不出是哪个实例展开了两万次。编译期求值步数还是将来加求值步数上限（防编译期死循环）的现成基础设施。

统计数据内部以键值对集合表示，非硬编码打印字符串——将来加 `--stats-format=json` 是加一个 renderer。不做 pass 内部层级 profiler：逐层插桩侵入性过强且插桩本身扰动被测数据，细看内部用外部 `perf` 采样更准。

#### 通道与退出码（Q21）

| 通道 | 内容 |
| --- | --- |
| stderr | 诊断、ICE、`--time-passes`、`--stats` |
| stdout | 仅 `--emit=<stage>` 未指定 `-o` 时的产物 |

| 退出码 | 含义 |
| --- | --- |
| `0` | 成功 |
| `1` | 编译错误 |
| `2` | 用法错误 |
| `101` | ICE |

**决定通道的是「这条输出是否为下游程序要消费的数据」，不是「它是否为错误」。** 这条排除了「统计走 stdout」的方案——统计语义上不算错误，但它会和 `--emit` 产物抢同一通道，而「看编译慢在哪同时 dump AST」是完全正常的组合。

`101` 而非 `3` 是为了和 Rust 对齐，stage0 就是 Rust 写的。

两条边界：`--emit` 请求多个阶段时禁止走 stdout，必须指定输出目录（两份 S-表达式混在一个流里没法拆，让下游靠解析格式头分流是把复杂度推给消费方）;**`--time-passes` / `--stats` 必须在编译结束后一次性打印**，不能边跑边打——它们与诊断共用 stderr，边打会插在诊断中间。这个时机与 Q14 的「结束时统一排序诊断」重合。

### 4. 错误码契约（Q24）

| 段 | 归属 |
| --- | --- |
| `E1xxx` | 词法 |
| `E2xxx` | 语法 |
| `E3xxx` | 名字解析 |
| `E4xxx` | 类型 |
| `E5xxx` | 内存与所有权 |
| `E6xxx` | 编译期求值 |
| `E7xxx` | 代码生成 |
| `E9xxx` | driver / IO / 链接 |

签入仓库的注册表文件是唯一真相源，每条带 `code` / `status`（active \| retired）/ 短消息 / `--explain` 长文。三个作用：并行开发撞号在 merge 阶段暴露;`--explain E4001` 有权威出处;删除的诊断留墓碑且码**永不复用、永不重编号**——否则用户搜到的旧文档会指向语义完全不同的新错误，比没文档更糟。

检查从 typecheck 搬到 borrow 时**码保持不动**：码标识用户看到的问题，不是编译器的目录结构。

`code u16` 上限 65535，四位分段最多 9999 个码，余量充足;这也意味着**不得设计五位码**，那会在第七个桶之后溢出。

## 后果

### 正面

1. **签名污染可控** — `~^Diag` 全局传递，但 `~^Log` 限于两个模块，`AllocErr` 只出现在规模已知的分配点。三者的蔓延范围都是有意划定的。
2. **快照测试可靠** — 默认剔 Span + 统一排序 + S-表达式，让快照只对结构敏感。
3. **输出选项不影响流程** — 限流在 sink 层、计数与渲染分离，`--error-limit` / `--error-format` / `--stats` 都不改变编译行为。
4. **级联抑制对泛型有效** — 毒化类型 + `(code, loc)` 去重正面处理了单态化复制诊断的问题。
5. **自举友好** — 退出码分四档让脚本能区分处理;S-表达式让 round-trip 检查用几十行 Kore 就能写。

### 负面

1. **`~^Diag` 出现在几乎所有 pass 签名里**，这是拒绝全局单例的代价。
2. **注册表需要人工维护**，加诊断多一步手续。
3. **`unreachable` 限于 `unsafe` 块**，穷尽匹配里写「不可能到达」比别的语言啰嗦。
4. **无 pass 内部细粒度计时**，深入分析要退回外部 `perf`。
5. **`assert` 在 release 保留**，编译器自身放弃了一部分速度。

### 取舍权衡

整套规范的取舍集中在一处：**宁可让签名变长、手续变多，也不引入全局可变状态或让输出选项影响流程。** 代价是写编译器代码时要多传几个参数;换来的是并行编译不需要加锁、快照测试稳定、以及「同样的输入永远得到同样的诊断」这个可预测性承诺。

## 实现检查清单

- [ ] `Diagnostic` / `DiagLoc` / `Severity` / `SubDiag` 定义，`Span` 保持 12 字节
- [ ] `~^Diag` sink：累积 + `err_count`，不做渲染
- [ ] 三个 renderer（human / json / short）消费同一份 `[Diagnostic]`，JSON 带 `"version": 1`
- [ ] pass 门禁：`err_count > 0` 阻断下一个 pass
- [ ] 诊断按 `(file, lo)` 在编译结束时统一排序
- [ ] `.Error` 毒化类型参与统一且不再报错
- [ ] `(code, loc)` 去重并保留出现计数
- [ ] `--error-limit` 在 sink 层限流，**不影响 `err_count`**，对所有 renderer 一致
- [ ] ICE 用固定缓冲区写 stderr，不分配、不经 `Diag`
- [ ] `assert` release 保留;`debug_assert` release 移除;`unreachable` 仅限 `unsafe` 块
- [ ] 小而密集的分配 die-on-failure;规模已知的分配返回 `T ! E`
- [ ] `alloc` 层保持 `T ! AllocErr`（不受编译器策略影响）
- [ ] `--emit=<stage>` 八个阶段，S-表达式 + 版本头，默认剔除 Span
- [ ] `--emit-spans` 开启 Span 输出
- [ ] `--emit` 多阶段时拒绝 stdout
- [ ] `~^Log` 仅 `typecheck/` 与 `borrow/`，`trace`/`debug` release 编译期消失
- [ ] `--time-passes` / `--stats` 走 stderr 且编译结束后一次性打印
- [ ] `--stats` 内部为键值对集合;含编译期求值步数与单态化实例数
- [ ] 退出码 `0` / `1` / `2` / `101`
- [ ] 错误码注册表文件签入，含 `status: active|retired`，码不复用
- [ ] `--explain <code>` 从注册表读长文

## 参考

- `docs/spec/01-overview.md` — 「可预测性优先于便利性」，六个统一记号
- `docs/adr/002-expression-statement-boundary.md` — `never`、`void`、发散构造
- `docs/adr/003-type-system-foundation.md` — `T ! E`、`T?`、和类型
- `docs/adr/007-compiler-module-structure.md` — pipeline 与 `diag` 模块位置、stage0/stage1 自举策略
- `docs/adr/008-code-style-and-naming.md` — 跨 stage 命名统一
