# 底层能力

面向内核、驱动、VM 的针对性设计。

## 1. 属性

`#name` 前缀属性，不是宏，由编译器直接识别。

```kore
#packed      -- 无填充
#align(64)   -- 指定对齐
#inline      -- 强制内联
#noinline
#naked       -- 无 prologue/epilogue，用于中断入口
#section(".text.boot")
#noreturn
#cold / #hot -- 分支布局提示
#abi("sysv") -- 调用约定
```

## 2. 位精确的硬件结构

任意宽度整数 + `#packed` 让寄存器布局直接可写，`vol` 标记易变访问：

```kore
Cr0 :: #packed {
  pe u1     -- protected mode enable
  mp u1
  em u1
  ts u1
  _  u27
  pg u1     -- paging
}

UartRegs :: #packed {
  dr   vol u32
  rsr  vol u32
  _    [4]u32
  fr   vol u32
}

UART_BASE :: 0x1000_0000       -- 编译期绑定到寄存器基址

putc :: (c u8) void => unsafe {
  uart := UART_BASE as ~^UartRegs   -- 整数转指针，要 unsafe
  @ uart.fr & 0x20 != 0 { }         -- 忙等 TX FIFO
  uart.dr = c as u32                -- vol 字段，不会被优化掉或重排
}
```

编译器保证 `vol` 字段的访问既不合并也不重排，位域读写生成单次宽度正确的 load/store。

## 3. 内联汇编

```kore
outb :: (port u16, val u8) void => asm {
  "outb %[v], %[p]"
  :: [v] "a"(val), [p] "Nd"(port)
  :  "memory"
}

rdtsc :: () u64 => asm {
  "rdtsc"
  : [lo] "={eax}"(u32), [hi] "={edx}"(u32)
} |> \(lo, hi) => (hi as u64 << 32) | lo as u64
```

## 4. 保证尾调用 `jmp`

`jmp` 是语义保证，不是优化提示。编译器必须把它生成为直接跳转，不压帧：

```kore
Expr :: .Lit(i64) | .Var(str) | .If(own ^Expr, own ^Expr, own ^Expr) | .App(own ^Expr, own ^Expr)

eval :: (env ^Env, expr ^Expr) i64 ! EvalErr => ? expr^ is {
  .Lit(n)        => n
  .Var(s)        => env.get(s)!
  .If(c, t, f)   => {
    next := ? eval(env, c)! is { 0 => f ; _ => t }
    jmp eval(env, next)    -- 保证不增长栈
  }
  .App(fn, arg)  => jmp apply(env, fn, arg)
}
```

VM dispatch loop 里，所有 `jmp` 到下一条指令的分发都能维持 O(1) 栈深。`jmp` 只能是函数体最后的表达式，类型必须与当前函数返回类型一致。

## 5. 原子操作

原子操作通过内置 trait `Atomic[T]` 暴露，而不是语言关键字：

```kore
~cnt : Atomic[u64] = .new(0)
cnt.add(1, .SeqCst)
old := cnt.swap(0, .AcqRel)
? cnt.cmp_xchg(0, 1, .Acquire, .Relaxed) is { .Ok(_) => ... ; .Err(v) => ... }
```

内存序枚举：`.Relaxed .Acquire .Release .AcqRel .SeqCst`。freestanding 下直接生成 `lock xadd`、`cmpxchg` 等指令，不需要 libc。

## 6. 编译期块

`::{ }` 内部在编译期求值，可以生成类型、数组、查表：

```kore
SIN_TABLE :: ::{
  ~t : [256]f32
  @ 0..256 => i { t[i] = sin(i as f32 * TAU / 256.0) }
  t                -- 块值即绑定的值
}
```

编译期块可调用任何标记为 `#comptime` 的函数，无副作用（不分配堆内存、不做 IO）。

## 7. freestanding 模式

编译目标声明 `target.freestanding = true` 时：

- 不链接 libc 和任何宿主库
- 入口是 `#naked` + `#noreturn` 的手写汇编跳板，或者用户声明的 `_start`
- 栈展开元数据不生成（`panic` 只能调用用户注册的 `panic_handler`）
- `std.mem.page` 对接用户提供的物理帧分配器

```kore
#naked #noreturn
#section(".text.reset")
_start :: () never => asm {
  "bl kernel_main"
  :: : "lr"
}

#noreturn
kernel_main :: () never => {
  arch.init_bss()
  arch.init_stack()
  main_loop()
  @ { }
}
```

