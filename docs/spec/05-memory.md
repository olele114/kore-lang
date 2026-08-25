# 内存与错误

## 1. 没有 GC，分配器是显式值

任何会分配的操作都必须拿到一个 `^Alloc`。这是内核开发的硬需求：early boot 只有 bump 分配器，中断上下文根本不能分配。

```kore
pub Alloc :: trait {
  alloc(~^self, sz usz, align usz) []u8 ! OOM
  free(~^self, mem []u8) void
  resize(~^self, mem []u8, nsz usz) []u8 ! OOM
}
```

标准库不持有全局分配器。签名里看不到 `^Alloc`，就保证不分配：

```kore
fmt_int  :: (buf ~[]u8, v i64) []u8            -- 不分配，写进调用者缓冲
to_str   :: (v i64, al ^Alloc) str ! OOM       -- 分配，显式
```

内置分配器：`mem.page`（页分配）、`mem.bump`（线性，freestanding 友好）、`mem.arena`（一次性释放）、`mem.fixed`（栈上固定缓冲）、`mem.dbg`（记录泄漏与越界）。

## 2. 所有权

Kore 用一套比 Rust 轻的模型：**唯一所有权 + 移动检查**，不做完整的借用生命周期推断。

```kore
own ^T        -- 唯一所有指针，作用域结束自动 drop，赋值即移动
^T            -- 借用指针，不拥有，不负责释放
~^T           -- 可变借用指针
```

编译器强制的检查有两条，都是流敏感但不需要生命周期变量：

1. **移动后不可用** — 移动过的 `own` 绑定再次使用是编译错误。
2. **不逃逸** — 借用指针不能存入比其来源更长寿的位置（栈上取址不能 return、不能写入堆对象字段）。

```kore
open :: (path str, al ^Alloc) own ^File ! IoErr => ...

use_it :: (al ^Alloc) void ! IoErr => {
  f := open("/x", al)!      -- f: own ^File
  defer f.close()           -- 作用域结束时执行
  read_all(f)!              -- 传 ^File，借用，不移动
}
```

第 2 条检查是保守的：拿不准时报错，用 `unsafe` 显式豁免。这个取舍是有意的 —— 把不逃逸检查扩展成完整的静态别名与生命周期分析会让自举编译器的复杂度翻倍。

## 3. 错误联合 `T ! E`

错误是值，不是控制流。`T ! E` 表示"成功给出 T，失败给出 E"：

```kore
IoErr :: .NotFound | .Perm | .Busy
read :: (f ^File, buf ~[]u8) usz ! IoErr => ...
```

三种消费方式：

```kore
n := read(f, buf)!          -- 传播：失败则原样返回给调用者
n := read(f, buf)!!         -- panic：失败即终止，用于"不可能失败"处
n := read(f, buf) ?! 0      -- 兜底：失败则取默认值

? read(f, buf) is {          -- 显式处理
  .Ok(n)       => use(n)
  .Err(.Busy)  => retry()
  .Err(e)      => log(e)
}
```

`T ! E` 的成功侧有两种写法，语义相同：裸 `T` 值自动提升为成功，也可以显式写 `.Ok(v)`。返回位置通常写裸值，`?` 的匹配形态里必须写 `.Ok(...)`，因为要与 `.Err(...)` 一起构成穷尽的臂。

```kore
read :: (f ^File, buf ~[]u8) usz ! IoErr => {
  ? not f.readable => ret .Err(.Perm)
  raw_read(f, buf)!    -- 裸值，等价于 .Ok(n)
}
```

后缀 `!` 只能出现在返回类型也是错误联合的函数里，错误集合由编译器推断求并，不必手写转换。用 `!` 而不是 `?`，因为 `?` 已被分支占用。

没有异常、没有栈展开、没有 `errno`。错误路径与正常路径在同一个返回值里，freestanding 下零成本。

## 4. `defer`

`defer` 把表达式挂到当前作用域退出时执行，逆序运行。它覆盖 RAII 覆盖不到的清理（fd、锁、中断屏蔽）：

```kore
lock :: (~^self) void => {
  self.mtx.acquire()
  defer self.mtx.release()      -- 无论从哪条路径退出都执行
  self.mutate()!                -- 传播时也会执行
}
```

`defer` 在 `stop`、`skip`、`ret`、传播 `!` 上都会触发；panic 时按配置决定是否运行。

## 5. 三条规则的合力

| 机制 | 负责 |
| --- | --- |
| `own ^T` + 移动检查 | 堆对象的生命周期，自动 `drop` |
| 不逃逸检查 | 借用指针不悬垂 |
| `defer` | 非内存资源的确定性释放 |

这套组合刻意比 Rust 弱：拿不到"无数据竞争"的静态保证，换来的是编译器实现量小一个数量级，以及不需要在签名里写生命周期。

