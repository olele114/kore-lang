# 测试分层说明

## 结构

```
tests/
├── unit/          # 单元测试（当前随代码在 src/ 内用 #[cfg(test)])
├── integration/   # 集成测试（跨模块协作）
├── e2e/           # 端到端测试（完整编译流程）
└── determinism/   # 确定性测试（便宜代理：重复编译比对）
```

## 分层原则

### 单元测试（Unit）
- **位置**：`src/` 各模块内 `#[cfg(test)] mod tests`
- **范围**：单个函数、单个类型、单个模块
- **特征**：快速、隔离、无 I/O
- **示例**：`lexer::tests::tokenize_yields_eof_without_panicking`

### 集成测试（Integration）
- **位置**：`tests/integration/`
- **范围**：多模块协作，验证接口契约
- **特征**：调用 pub API，模拟真实使用场景
- **示例**：`lexer_parser.rs` —— tokenize → parse_module pipeline

### 端到端测试（E2E）
- **位置**：`tests/e2e/`
- **范围**：完整编译流程，源码到产物
- **特征**：使用 `--~` 注解 + `verify_test_annotations`
- **示例**：`smoke.rs` —— 验证已知错误产生预期诊断

### 确定性测试（Determinism）
- **位置**：`tests/determinism/`
- **范围**：便宜代理（同一输入编译两次，逐字节比对）
- **特征**：捕获非确定性 bug（HashMap 遍历顺序、随机数等）
- **示例**：`mod.rs` —— 验证 AST 输出的可重现性

## Cargo 配置

```toml
[[test]]
name = "integration"
path = "tests/integration/main.rs"

[[test]]
name = "e2e"
path = "tests/e2e/main.rs"

[[test]]
name = "determinism"
path = "tests/determinism/mod.rs"
```

## 运行

```bash
cargo test               # 全部测试
cargo test --lib         # 仅单元测试
cargo test --test integration
cargo test --test e2e
cargo test --test determinism
```
