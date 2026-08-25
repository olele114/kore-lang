# Kore Language

自举编译器实现，使用多阶段引导策略。

## 项目结构

- `stage0/` - Rust 实现的引导编译器
- `docs/` - 架构决策记录（ADR）和设计文档
- `scripts/` - 构建和测试工具

## 构建

```bash
./build.sh  # 60 秒构建预算强制执行
```

或直接使用 Cargo：

```bash
cargo build --release --package kore-stage0
```

## 测试

```bash
# 运行所有测试
cargo test --workspace

# 运行覆盖率检查（需要 cargo-llvm-cov）
cargo llvm-cov --workspace --fail-under-lines 90

# 运行性能基准测试
cargo bench --package kore-stage0
```

## CI/CD

项目使用 GitHub Actions 进行持续集成：

- **Tests**: 单元测试、集成测试、格式化检查、Clippy
- **Coverage**: 代码覆盖率检查（≥90% 目标）
- **Benchmarks**: 性能回归检测（>20% 退化阈值）
- **Weekly Fuzzing**: 每周运行模糊测试

## 质量指标

- **测试覆盖率**: ≥90% (stage0)
- **构建时间**: ≤60 秒
- **性能退化阈值**: 20%

## 许可证

待定
