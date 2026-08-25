#!/usr/bin/env bash
# stage0 的构建入口。ADR 007 Q4-Q5：前期手写脚本直接调编译器，文件列表显式。
#
# stage0 的宿主是 Rust，所以这里的「显式」体现在环境上而不是文件列表上——
# cargo 已经知道要编哪些文件，本脚本负责把 inkwell 需要的 LLVM 前缀钉死，
# 免得每个人各自记一遍那个环境变量。
#
# 自举闭合后 stage0 归档，这个脚本随之作废；stage1 会有自己的 build.sh，
# 那一份才真的显式列文件。

set -euo pipefail

# Termux 的 LLVM 装在 usr 前缀下。允许外部覆盖，便于换机器。
: "${LLVM_SYS_211_PREFIX:=/data/data/com.termux/files/usr}"
export LLVM_SYS_211_PREFIX

cd "$(dirname "$0")"

usage() {
    cat <<'EOF'
用法: build.sh [子命令]

  build   编译 korec（默认）
  test    跑第一层测试，并报告墙钟耗时
  check   只做类型检查，不生成产物
  cov     跑覆盖率，摘要输出；加 --gate 时低于目标即失败
  clean   清掉 target/
  all     check + test + build

环境:
  LLVM_SYS_211_PREFIX  LLVM 21 的安装前缀，默认 Termux 的 usr
  LLVM_COV             llvm-cov 路径，默认取 PATH 上的
  LLVM_PROFDATA        llvm-profdata 路径，默认取 PATH 上的
EOF
}

# ADR 010 Q12：第一层预算 60 秒，参考移动设备。超出只告警，不判失败。
LAYER1_BUDGET_SECS=60

# 覆盖率目标：stage0 是引导编译器，错在这里下游全错，所以卡得比 stage1+ 高。
COV_LINE_TARGET=90

# cargo-llvm-cov 默认从 rustup 的 llvm-tools 组件里找 llvm-cov/llvm-profdata。
# 这台机器的 rustc 是源码编译的，没有 rustup，也就没有那个组件，
# 必须手动指到系统 LLVM。rustc 内嵌的 LLVM 与系统 llvm 同为 21.1.8，
# profraw 格式一致，所以可以这么混用；换机器时先核对两边版本号。
run_cov() {
    : "${LLVM_COV:=$(command -v llvm-cov)}"
    : "${LLVM_PROFDATA:=$(command -v llvm-profdata)}"
    export LLVM_COV LLVM_PROFDATA

    if [ -z "$LLVM_COV" ] || [ -z "$LLVM_PROFDATA" ]; then
        echo "找不到 llvm-cov / llvm-profdata，装 llvm 包或显式设这两个环境变量。" >&2
        exit 2
    fi

    if [ "${1:-}" = "--gate" ]; then
        cargo llvm-cov --summary-only --fail-under-lines "$COV_LINE_TARGET"
    else
        cargo llvm-cov --summary-only
    fi
}

run_tests() {
    local start elapsed
    start=$(date +%s)
    cargo test
    elapsed=$(( $(date +%s) - start ))
    echo "第一层墙钟: ${elapsed}s / 预算 ${LAYER1_BUDGET_SECS}s"
    if [ "$elapsed" -gt "$LAYER1_BUDGET_SECS" ]; then
        echo "告警: 第一层超出预算，考虑把慢用例挪到第二层。这不是用例失败。" >&2
    fi
}

case "${1:-build}" in
    build) cargo build ;;
    test)  run_tests ;;
    check) cargo check --all-targets ;;
    cov)   run_cov "${2:-}" ;;
    clean) cargo clean ;;
    all)   cargo check --all-targets && run_tests && cargo build ;;
    -h|--help|help) usage ;;
    *) echo "未知子命令: $1" >&2; usage >&2; exit 2 ;;
esac
