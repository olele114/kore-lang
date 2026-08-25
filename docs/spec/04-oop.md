# 对象模型

Kore 的 OOP 由三件东西组成：**类型自带方法**（封装）、**trait**（多态接口）、**嵌入**（复用）。没有实现继承。

## 1. 方法

`impl` 块给类型挂方法。接收者显式写出，`^self` 只读，`~^self` 可变 —— 与 `~`/`^` 的全局含义一致。

```kore
Counter :: { n u64 }

Counter :: impl {
  new()            Self => Self{n: 0}     -- 无接收者 = 静态方法
  get(^self)       u64  => self.n
  bump(~^self)     void => self.n += 1
  reset(~^self)    void => self.n = 0
}

~c := Counter.new()
c.bump()
c.get()
```

调用点自动取址：`c.bump()` 等价于 `Counter.bump(^c)`。

## 2. 封装

默认私有，`pub` 导出。字段与方法各自控制：

```kore
pub File :: {
  pub path str        -- 外部可读
  fd  i32             -- 模块私有
}
```

字段可变性由持有者的绑定决定，不由字段声明决定 —— 少一层规则要记。

## 3. trait

trait 描述行为，可带默认实现：

```kore
pub Writer :: trait {
  write(~^self, buf []u8) usz ! IoErr        -- 必须实现
  flush(~^self) void ! IoErr => .Ok(void)    -- 默认实现
  write_str(~^self, s str) usz ! IoErr => self.write(s.bytes())
}
```

实现是**外部的**（孤儿规则允许在类型或 trait 任一方所在模块实现）：

```kore
Serial :: impl Writer {
  write(~^self, buf []u8) usz ! IoErr => {
    @ buf => b { self.put(b) }
    .Ok(buf.len)
  }
}
```

## 4. 静态派发与 `dyn`

默认**静态派发**：泛型参数带 trait 约束，编译期单态化，无间接调用开销。

```kore
emit :: [W: Writer] (w ~^W, s str) void ! IoErr => w.write_str(s)!
```

需要运行期多态时才用 `dyn`，此时是"指针 + vtable"的胖指针：

```kore
sinks : []^dyn Writer = [^serial, ^console, ^log]
@ sinks => s { s.write_str("hi")! }
```

`dyn` 是显式的，因为内核里每一次间接跳转都是可见成本。vtable 布局稳定，可在 freestanding 下使用。

## 5. 嵌入

`use` 把另一个类型嵌入进来并自动转发其方法，替代实现继承：

```kore
Base :: { id u64 }
Base :: impl {
  id_of(^self) u64 => self.id
}

Conn :: {
  use Base            -- 嵌入，字段与方法都被提升
  sock i32
}

c := Conn{Base{id: 7}, 3}
c.id_of()             -- 转发到 Base.id_of(^c.Base)
```

冲突时必须显式限定（`c.Base.id_of()`），不做 C++ 那样的复杂解析。

## 6. 为什么没有实现继承

需求里的 OOP 由三部分完整覆盖：封装靠 `pub` 与模块，多态靠 trait，复用靠嵌入。省掉实现继承是有代价的取舍，理由是三条：

1. **脆弱基类** — 修改基类会静默改变派生类行为，内核代码里这是很难排查的一类 bug。
2. **对象布局不可预测** — 多重继承与虚基类让 `#packed` 结构无法与硬件寄存器精确对应。
3. **自举复杂度** — 继承层次的名字解析与 vtable 合并是编译器里最容易出错的部分之一，Kore0 要能被一个小编译器接受。

代价是深层类型层次要靠组合手写转发。嵌入把这部分成本降到"一行 `use`"。

