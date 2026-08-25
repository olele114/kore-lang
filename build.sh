#!/usr/bin/env bash
# Kore stage0 构建脚本
# 强制执行 60 秒构建预算（ADR 010）

set -euo pipefail

echo "==> Building kore-stage0 (60s budget)"

cd "$(dirname "$0")"

# 使用 timeout 强制执行时间限制
if timeout 60s cargo build --release --manifest-path stage0/Cargo.toml; then
    echo "✓ Build completed within 60s budget"
    exit 0
else
    EXIT_CODE=$?
    if [ $EXIT_CODE -eq 124 ]; then
        echo "✗ Build exceeded 60s budget" >&2
        exit 1
    else
        echo "✗ Build failed with exit code $EXIT_CODE" >&2
        exit $EXIT_CODE
    fi
fi
