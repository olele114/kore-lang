#!/usr/bin/env python3
"""
基准测试对比脚本

比较两次 Criterion 基准测试结果，检测性能退化。
退化超过 20% 时退出码为 1（失败）。
"""

import sys
import json
from pathlib import Path
from typing import Dict, Tuple

REGRESSION_THRESHOLD = 0.20  # 20% 退化阈值


def load_criterion_estimates(path: Path) -> Dict[str, float]:
    """加载 Criterion estimates.json 文件"""
    with open(path) as f:
        data = json.load(f)
    
    # Criterion 格式：{"mean": {"point_estimate": <value>, ...}, ...}
    if "mean" in data and "point_estimate" in data["mean"]:
        return {"mean": data["mean"]["point_estimate"]}
    
    return {}


def compare_benchmarks(baseline_path: Path, current_path: Path) -> Tuple[bool, str]:
    """
    比较两个基准测试结果
    
    返回: (是否通过, 报告文本)
    """
    baseline = load_criterion_estimates(baseline_path)
    current = load_criterion_estimates(current_path)
    
    if not baseline or not current:
        return False, "错误: 无法加载基准测试数据"
    
    baseline_mean = baseline.get("mean", 0)
    current_mean = current.get("mean", 0)
    
    if baseline_mean == 0:
        return False, "错误: baseline 均值为 0"
    
    change_ratio = (current_mean - baseline_mean) / baseline_mean
    change_pct = change_ratio * 100
    
    report = []
    report.append("=" * 60)
    report.append("基准测试对比报告")
    report.append("=" * 60)
    report.append(f"Baseline: {baseline_mean:.2f} ns")
    report.append(f"Current:  {current_mean:.2f} ns")
    report.append(f"变化:     {change_pct:+.2f}%")
    report.append("")
    
    passed = True
    
    if change_ratio > REGRESSION_THRESHOLD:
        report.append(f"❌ 性能退化超过阈值 ({REGRESSION_THRESHOLD*100:.0f}%)")
        passed = False
    elif change_ratio > 0:
        report.append(f"⚠️  轻微性能下降 ({change_pct:.2f}%)")
    elif change_ratio < -0.05:
        report.append(f"✅ 性能提升 ({-change_pct:.2f}%)")
    else:
        report.append("✅ 性能保持稳定")
    
    report.append("=" * 60)
    
    return passed, "\n".join(report)


def main():
    if len(sys.argv) != 3:
        print("用法: compare_benches.py <baseline.json> <current.json>")
        sys.exit(2)
    
    baseline_path = Path(sys.argv[1])
    current_path = Path(sys.argv[2])
    
    if not baseline_path.exists():
        print(f"错误: baseline 文件不存在: {baseline_path}")
        sys.exit(2)
    
    if not current_path.exists():
        print(f"错误: current 文件不存在: {current_path}")
        sys.exit(2)
    
    passed, report = compare_benchmarks(baseline_path, current_path)
    
    print(report)
    
    sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()
