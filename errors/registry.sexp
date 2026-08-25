;; kore-errors v1
;;
;; 错误码登记表。ADR 009 定义：本表是唯一真源，码号不复用、不重编号，
;; 退役条目保留并把 status 改成 retired。
;;
;; 号段：E1xxx 词法 / E2xxx 语法 / E3xxx 名字解析 / E4xxx 类型
;;       E5xxx 内存与所有权 / E6xxx 编译期求值 / E7xxx 代码生成
;;       E9xxx driver/IO/链接
;;
;; Diagnostic.code 是 u16，上限 65535，所以禁止五位码号。
;;
;; 每条：(error (code N) (status active|retired) (msg "…") (explain "…"))

(error
  (code 9001)
  (status active)
  (msg "无法读取源文件")
  (explain "driver 打开源文件失败。常见原因是路径拼写错误、文件不存在，或当前用户对该路径没有读权限。检查路径后重试。"))

(error
  (code 9002)
  (status active)
  (msg "源文件不是有效的 UTF-8")
  (explain "Kore 源文件必须是 UTF-8 编码。该文件含有无法按 UTF-8 解码的字节序列。用编辑器另存为 UTF-8，或检查文件是否其实是二进制。"))

(error
  (code 4001)
  (status active)
  (msg "类型不匹配")
  (explain "期望的类型与实际得到的类型不一致。Kore 没有隐式数值转换，宽窄不同的整数类型之间也需要显式 as。诊断正文会给出期望类型与实际类型。"))

(error
  (code 5001)
  (status active)
  (msg "移动后使用")
  (explain "own ^T 绑定的所有权已经转移，之后不能再使用这个名字。所有权在赋值给新绑定、作为实参传给函数时转移。Kore 的检查是流敏感但保守的：不确定是否已移动时按已移动处理。如果需要在移动后继续访问，改用借用指针 ^T，或在移动前先取一份副本。"))

(error
  (code 5002)
  (status active)
  (msg "借用指针逃逸到堆")
  (explain "借用指针 ^T 不拥有所指对象，不能存入比它来源更长寿的位置。把栈上对象的借用写进结构体字段（该结构体可能在堆上、活得更久）会产生悬垂指针，因此被拒绝。改为存入 own ^T 转移所有权，或让被借用的对象活得至少和容器一样久。"))

(error
  (code 7002)
  (status active)
  (msg "代码生成失败")
  (explain "后端把 HIR 翻译成 LLVM IR 时失败。这通常不是源码问题，而是 stage0 后端尚未覆盖该构造，或 HIR 中存在后端无法处理的形状。诊断正文会给出后端报告的具体原因。用 --debug-trace 可以看到失败前生成的 IR 与降级过程。"))

(error
  (code 5003)
  (status active)
  (msg "借用指针逃逸到返回值")
  (explain "不能返回指向函数局部对象的借用指针：函数返回后该对象已销毁，指针悬垂。返回 own ^T 转移所有权，或返回由参数传入的借用（参数的借用与调用者同寿，可以返回）。"))
