# 完整示例

## 1. 泛型动态数组

```kore
use std.mem { Alloc, OOM }

pub Vec :: [T] {
  buf ~[]T
  len usz
  al  ^Alloc
}

pub Vec :: [T] impl {
  new(al ^Alloc) Self => Self{buf: [], len: 0, al}

  items(^self) []T => self.buf[..self.len]

  push(~^self, v T) void ! OOM => {
    ? self.len == self.buf.len => self.grow()!
    self.buf[self.len] = v
    self.len += 1
  }

  pop(~^self) T? => ? {
    self.len == 0 => nil
    _ => {
      self.len -= 1
      self.buf[self.len]
    }
  }

  grow(~^self) void ! OOM => {
    ncap := ? { self.buf.len == 0 => 8 ; _ => self.buf.len * 2 }
    self.buf = self.al.resize(self.buf, ncap)!
  }

  drop(~^self) void => self.al.free(self.buf)
}
```

`drop` 是编译器识别的名字：`own ^Vec[T]` 出作用域时自动调用。

## 2. AST 解释器

展示联合、穷尽匹配、错误联合、保证尾调用。

```kore
Expr :: .Num(i64)
      | .Var(str)
      | .Add(own ^Expr, own ^Expr)
      | .If(own ^Expr, own ^Expr, own ^Expr)

EvalErr :: .UnboundVar(str) | .TypeErr

Env :: { parent ^Env?, keys []str, vals []i64 }

Env :: impl {
  get(^self, name str) i64 ! EvalErr => {
    @ 0..self.keys.len => i {
      ? self.keys[i] == name => ret self.vals[i]
    }
    ? self.parent is {
      nil  => .Err(.UnboundVar(name))
      p    => jmp p.get(name)         -- 保证尾调用遍历环境链
    }
  }
}

eval :: (env ^Env, e ^Expr) i64 ! EvalErr => ? e^ is {
  .Num(n)        => n
  .Var(s)        => env.get(s)!
  .Add(l, r)     => eval(env, l)! + eval(env, r)!
  .If(c, t, f)   => {
    branch := ? eval(env, c)! is {
      0    => f
      _    => t
    }
    jmp eval(env, branch)             -- 尾调用，不增长栈
  }
}
```


## 3. freestanding 串口驱动

展示 `#packed`、`vol`、内联汇编、`#naked`/`#noreturn`，无 libc 依赖。

```kore
-- 直接映射 PL011 UART 寄存器布局

UartRegs :: #packed {
  dr   vol u32   -- 数据寄存器（读：接收，写：发送）
  rsr  vol u32   -- 接收状态 / 错误清除
  _    [4]u32    -- 保留
  fr   vol u32   -- 标志寄存器（bit5 = TX FIFO 满）
}

pub Uart :: { regs ~^UartRegs }

Uart :: impl {
  -- 用物理地址构造驱动句柄；整数转指针必须 unsafe
  at(addr usz) Self => unsafe { Self{ regs: addr as ~^UartRegs } }

  putc(^self, c u8) void => {
    @ self.regs.fr & 0x20 != 0 { }   -- 忙等：TX FIFO 满则自旋
    self.regs.dr = c as u32
  }

  puts(^self, s str) void => {
    @ s.bytes() => b { self.putc(b) }
  }
}

-- 裸函数：无 prologue/epilogue，用作中断/复位入口
#naked #noreturn
#section(".text.boot")
_start :: () never => asm {
  "bl kore_main"
  :: : "lr"
}

#noreturn
kore_main :: () never => {
  uart := Uart.at(0x1000_0000)
  uart.puts("boot ok\r\n")
  @ { }    -- 无限循环，不返回
}
```

`vol` 保证每次字段访问都生成真实 load/store，编译器不得合并或重排。

## 4. 函数式管道

展示闭包、`|>`、`fold`，以及块作为值的惯用法。

```kore
use std.io

-- 计算切片中所有正整数的平方和

sum_pos_squares :: (xs []i64) i64 =>
  xs
    |> filter(\x => x > 0)
    |> map(\x => x * x)
    |> fold(0i64, \acc, x => acc + x)

-- 带标注的版本，展示中间结果
describe :: (xs []i64) void ! io.Err => {
  pos   := xs |> filter(\x => x > 0)
  total := pos |> fold(0i64, \acc, x => acc + x)
  count := pos |> fold(0i64, \acc, _ => acc + 1)
  avg   := ? { count == 0 => 0i64 ; _ => total / count }
  io.out.print("count={} total={} avg={}\n", count, total, avg)!
}
```

`|>` 把左侧值注入右侧调用的第一个参数位置；链式写法与逐步绑定语义等价。
`filter` / `map` 返回惰性迭代器，由 `fold` 驱动求值，整条管道不产生中间分配 ——
所以 `pos` 没有 `.len`，`describe` 里两次 `fold` 各自遍历一遍。

