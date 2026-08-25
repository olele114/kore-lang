#!/usr/bin/env python3
"""
基准测试对比脚本（ADR 010）

比较 Criterion 生成的基准结果，检测性能退化。
退化阈值：20%（避免噪声误报）
"""

import json
import sys
from pathlib import Path
from typing import Dict, Any

def parse_criterion_json(path: Path) -> Dict[str, Any]:
    """解析 Criterion JSON 输出"""
    with open(path) as f:
        return json.load(f)

def extract_estimates(data: Dict[str, Any]) -> Dict[str, float]:
    """提取每个基准的时间估算（纳秒）"""
    results = {}
    
    # Criterion JSON 格式：每个基准是一个 entry
    for bench_name, bench_data in data.items():
        if isinstance(bench_data, dict) and "mean" in bench_data:
            # mean 是 {point_estimate: ns, confidence_interval: ...}
            results[bench_name] = bench_data["mean"]["point_estimate"]
    
    return results

def compare_results(baseline: Dict[str, float], current: Dict[str, float], threshold: float = 0.20) -> tuple[bool, str]:
    """比较基准结果，返回 (是否通过, 报告文本)"""
    report_lines = ["# 基准测试对比报告\n"]
    all_pass = True
    
    for bench_name in sorted(baseline.keys()):
        if bench_name not in current:
            report_lines.append(f"⚠️  {bench_name}: 缺失（仅在 baseline 中）")
            continue
        
        base_time = baseline[bench_name]
        curr_time = current[bench_name]
        
        # 计算变化百分比
        if base_time == 0:
            pct_change = 0.0
        else:
            pct_change = (curr_time - base_time) / base_time
        
        # 判断是否退化
        if pct_change > threshold:
            status = "❌ FAIL"
            all_pass = False
        elif pct_change > 0.05:
            status = "⚠️  WARN"
        elif pct_change < -0.05:
            status = "✅ FASTER"
        else:
            status = "✅ PASS"
        
        report_lines.append(
            f"{status}  {bench_name}: {base_time/1e6:.2f}ms → {curr_time/1e6:.2f}ms "
            f"({pct_change:+.1%})"
        )
    
    # 检查新增的基准
    for bench_name in current.keys():
        if bench_name not in baseline:
            report_lines.append(f"✨ {bench_name}: 新增 ({current[bench_name]/1e6:.2f}ms)")
    
    report = "\n".join(report_lines)
    return all_pass, report

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
    
    try:
        baseline_data = parse_criterion_json(baseline_path)
        current_data = parse_criterion_json(current_path)
        
        baseline_times = extract_estimates(baseline_data)
        current_times = extract_estimates(current_data)
        
        passed, report = compare_results(baseline_times, current_times)
        
        print(report)
        
        if passed:
            print("\n✅ 所有基准测试通过！")
            sys.exit(0)
        else:
            print("\n❌ 检测到性能退化 (>20%)")
            sys.exit(1)
    
    except Exception as e:
        print(f"错误: {e}")
        sys.exit(2)

if __name__ == "__main__":
    main()
