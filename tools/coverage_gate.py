#!/usr/bin/env python3
"""覆盖率门禁（S9-02）。

输入：`cargo llvm-cov --lcov --output-path lcov.info` 的报告。
门限：整体行覆盖 ≥ 70%；`src/domain/**` 行覆盖 ≥ 90%。

用法：
    cargo llvm-cov --all-targets --lcov --output-path lcov.info -- --skip large_full_text_find_is_bounded
    python tools/coverage_gate.py lcov.info [--overall 70] [--domain 90]

排除项及理由（与 docs/testing.md §排除清单一致）：
- `src/bin/**`：架构校验工具本身，不随应用分发。
- `tests/` 与 `#[cfg(test)]`：lcov 报告只含 src/ 下被编译的生产代码，
  cfg(test) 天然不在内，无需额外处理。

lcov 报告格式：`SF:<路径>` 开头一个记录块，块内 `LF:` 总行数、
`LH:` 命中行数。本脚本只做行覆盖（line coverage）——分支覆盖在
Rust 的 llvm-cov 报告里噪声大（泛型单态化），不作为门禁指标。
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path, PurePosixPath


def parse_lcov(path: Path) -> dict[str, tuple[int, int]]:
    """返回 {文件路径: (命中行数, 总行数)}。"""
    files: dict[str, tuple[int, int]] = {}
    current: str | None = None
    lf = lh = 0
    with path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.rstrip("\n")
            if line.startswith("SF:"):
                current = line[3:].replace("\\", "/")
                lf = lh = 0
            elif line.startswith("LF:"):
                lf = int(line[3:])
            elif line.startswith("LH:"):
                lh = int(line[3:])
            elif line == "end_of_record":
                if current is not None:
                    files[current] = (lh, lf)
                current = None
    return files


def normalize(posix_path: str) -> str:
    """取 `src/` 起的相对路径；无 `src/` 时原样返回（小写盘符差异不影响匹配）。"""
    idx = posix_path.find("src/")
    return posix_path[idx:] if idx >= 0 else posix_path


def is_excluded(posix_path: str) -> bool:
    """排除清单（理由见模块 docstring 与 docs/testing.md）。"""
    rel = normalize(posix_path)
    parts = PurePosixPath(rel).parts
    return "src" in parts[:1] and "bin" in parts


def is_domain(posix_path: str) -> bool:
    """领域层判定：`src/domain/**`。"""
    return normalize(posix_path).startswith("src/domain/")


def aggregate(files: dict[str, tuple[int, int]]) -> tuple[float, float]:
    """返回（整体覆盖率, 领域层覆盖率），单位 %。"""
    total_hit = total_lf = 0
    domain_hit = domain_lf = 0
    for path, (hit, lf) in files.items():
        if is_excluded(path) or lf == 0:
            continue
        total_hit += hit
        total_lf += lf
        if is_domain(path):
            domain_hit += hit
            domain_lf += lf
    overall = 100.0 * total_hit / total_lf if total_lf else 0.0
    domain = 100.0 * domain_hit / domain_lf if domain_lf else 0.0
    return overall, domain


def main() -> int:
    parser = argparse.ArgumentParser(description="rustmark 覆盖率门禁")
    parser.add_argument("lcov", type=Path, help="llvm-cov 的 lcov 报告路径")
    parser.add_argument("--overall", type=float, default=70.0)
    parser.add_argument("--domain", type=float, default=90.0)
    args = parser.parse_args()

    files = parse_lcov(args.lcov)
    if not files:
        print("门禁失败：报告为空或格式不符", file=sys.stderr)
        return 2

    overall, domain = aggregate(files)
    print(f"整体行覆盖：{overall:.2f}%（门限 {args.overall}%）")
    print(f"领域层行覆盖：{domain:.2f}%（门限 {args.domain}%）")

    # 2% 缓冲来自阶段 9 风险表：合法重构的小幅回落不应立刻红，
    # 但缓冲区会打 WARN，提示该补测试了。
    ok = True
    if overall < args.overall:
        if overall < args.overall - 2.0:
            print(f"::error::整体覆盖率 {overall:.2f}% 低于门限 {args.overall}%")
            ok = False
        else:
            print(f"::warning::整体覆盖率 {overall:.2f}% 落入门限缓冲区")
    if domain < args.domain:
        if domain < args.domain - 2.0:
            print(f"::error::领域层覆盖率 {domain:.2f}% 低于门限 {args.domain}%")
            ok = False
        else:
            print(f"::warning::领域层覆盖率 {domain:.2f}% 落入门限缓冲区")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
