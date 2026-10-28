#!/usr/bin/env python3
"""接入成本度量（V11-66）。

为 core 结构治理（docs/archive/architecture-reviews/core-refactor-tasklist-2026-10-24.md 的 D 组触发条件、
V11-60/62 排序）提供数据，回答两个问题：

1. **每提交的触及面**：平均「文件数 / 逻辑模块数 / workspace crate 数 /
   测试行变更数」——衡量「单功能平均要动多少地方」的维护与接入成本；
2. **目录级提交频率**：哪些模块最热（变更频率是模块归属判据四问之一，
   见 docs/spec/architecture-constraints.md「模块归属判据」）。

口径（与评审报告对齐的说明；变更口径需同步两边注释）：

- 窗口：最近 N 个月（`--months`，默认 12），自今天往前推 365×N/12 天；
  起止日期打印在表头，保证基线可复现。
- **不含 merge 提交**：merge 本身不携带文件变更（`--name-only` 下为空），
  计入只会稀释均值。
- **逻辑模块路径合并**：D-3 阶段 4a（`2f4fd07`，2026-10-21）把四层搬进
  `crates/rustmark-core/src/`；`src/<层>/**` 与 `crates/rustmark-core/src/<层>/**`
  是同一模块的前后两址，**合并计数**。`presentation/views` 未搬离 root，
  两址本就相同。
- **crate 触及数**：`crates/rustmark-core/**` 记 core；`src/**`、`tests/**`、
  `build.rs`、根 `Cargo.toml` / `Cargo.lock` 记 root；docs / tools / ci /
  scripts / assets 不属任何 crate（记 0）。
- **测试行变更**：`tests/` 下与 src 内文件名含 `test` 的文件（如
  `*_test.rs`）的 numstat 增+删之和。src 内 `#[cfg(test)]` 与生产代码
  同文件共存，numstat 无法拆分，只能按文件归集——故本指标是上界口径。

与评审报告数字的关系：评审报告（docs/archive/architecture-reviews/core-structure-review-2026-10-24.md）
记 root views 70 / presentation/state 30 / theme 19（12mo），本脚本同口径
实测为 69 / 31 / 20（见 docs/spec/metrics.md 接入成本基线条的差异说明）。
差异 ≤2 提交，来自评审当时的一次性聚合方法无法完全复现；**热模块排序不变**，
不影响 D 组触发判定。

用法：
    python tools/onboard_cost.py              # 最近 12 个月，md 表到 stdout
    python tools/onboard_cost.py --months 6   # 改窗口
    python tools/onboard_cost.py --top 15     # 频率表只列前 15 行
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from datetime import date, timedelta
from pathlib import PurePosixPath

CORE_SRC = "crates/rustmark-core/src/"
ROOT_SRC = "src/"
LAYERS = ("domain", "application", "infrastructure", "shared", "presentation")
ROOT_CRATE_FILES = ("build.rs", "Cargo.toml", "Cargo.lock")
NON_CRATE_TOPS = ("docs", "tools", ".github", "scripts", "assets")
TEST_NAME = re.compile(r"test")


def git(*args: str) -> str:
    """跑 git 并返回 stdout；`core.quotepath=false` 防中文路径被转义。"""
    proc = subprocess.run(
        ["git", "-c", "core.quotepath=false", *args],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    return proc.stdout


def normalize(path: str) -> str:
    return path.replace("\\", "/").strip('"')


def logical_module(path: str) -> str:
    """文件路径 → 逻辑模块名（合并 4a 拆分前后两址，见模块 docstring）。"""
    p = normalize(path)
    for prefix in (CORE_SRC, ROOT_SRC):
        if p.startswith(prefix):
            parts = PurePosixPath(p[len(prefix) :]).parts
            head = parts[0]
            layer = head[: -len(".rs")] if head.endswith(".rs") else head
            if layer not in LAYERS:
                if layer == "app":
                    return "app"
                if layer == "bin":
                    return f"bin/{parts[1]}" if len(parts) > 1 else "bin"
                return "root-other"
            if len(parts) >= 3:
                return f"{layer}/{parts[1]}"
            return layer
    if p.startswith("tests/"):
        parts = PurePosixPath(p).parts
        if len(parts) == 1:
            return "tests-other"
        if len(parts) == 2:  # tests/<file>.rs 单文件目标
            return f"tests/{PurePosixPath(parts[1]).stem}"
        return f"tests/{parts[1]}"
    if p in ROOT_CRATE_FILES:
        return {"Cargo.toml": "root-manifest", "build.rs": "build-rs", "Cargo.lock": "Cargo.lock"}[p]
    if p == f"{CORE_SRC[:-4]}Cargo.toml":  # crates/rustmark-core/Cargo.toml
        return "core-manifest"
    top = PurePosixPath(p).parts[0] if p else ""
    if top in NON_CRATE_TOPS:
        return top
    if top == "crates":
        return "crates-other"
    return "root-files"


def crates_touched(paths: list[str]) -> set[str]:
    """本提交触及的 workspace 成员集合（docs/tools 等不属任何 crate）。"""
    crates: set[str] = set()
    for raw in paths:
        p = normalize(raw)
        if p.startswith(CORE_SRC):
            crates.add("rustmark-core")
        elif p.startswith(ROOT_SRC) or p.startswith("tests/") or p in ROOT_CRATE_FILES:
            crates.add("rustmark")
    return crates


def is_test_file(path: str) -> bool:
    p = normalize(path)
    if p.startswith("tests/"):
        return True
    return bool(TEST_NAME.search(PurePosixPath(p).stem))


def collect(since: str) -> list[tuple[str, list[tuple[str, int, int]]]]:
    """返回 [(commit, [(path, added, deleted), ...]), ...]（按 git log 顺序）。"""
    # `--no-renames`：numstat 默认开 rename 检测，被改名文件会以
    # `{old => new}` 合成路径出现，既污染模块归集（花括号路径落进垃圾桶），
    # 也让「改名同时改了哪个模块」的语义丢失。关掉后改名 = 删旧 + 增新两条
    # 字面记录，新旧模块各自正确计一次。
    out = git(
        "log", "--no-merges", "--no-renames", f"--since={since}",
        "--numstat", "--pretty=format:%H",
    )
    commits: list[tuple[str, list[tuple[str, int, int]]]] = []
    current: str | None = None
    for line in out.splitlines():
        if re.fullmatch(r"[0-9a-f]{40}", line):
            current = line
            commits.append((current, []))
        elif current is not None and "\t" in line:
            added, deleted, path = line.split("\t", 2)
            add = 0 if added == "-" else int(added)
            dele = 0 if deleted == "-" else int(deleted)
            commits[-1][1].append((path, add, dele))
    return commits


def main() -> int:
    parser = argparse.ArgumentParser(description="rustmark 接入成本度量（V11-66）")
    parser.add_argument("--months", type=int, default=12, help="统计窗口（月，默认 12）")
    parser.add_argument("--top", type=int, default=25, help="频率表最多列几行（默认 25）")
    args = parser.parse_args()

    days = round(args.months * 365 / 12)
    since = (date.today() - timedelta(days=days)).isoformat()
    commits = collect(since)
    if not commits:
        print(f"窗口 {since} 起无提交，检查 --months 取值", file=sys.stderr)
        return 2

    # ---- 每提交聚合 ----
    file_counts: list[int] = []
    module_counts: list[int] = []
    crate_counts: list[int] = []
    test_line_counts: list[int] = []
    for _, files in commits:
        file_counts.append(len(files))
        module_counts.append(len({logical_module(p) for p, _, _ in files}))
        crate_counts.append(len(crates_touched([p for p, _, _ in files])))
        test_line_counts.append(
            sum(a + d for p, a, d in files if is_test_file(p))
        )

    # ---- 目录频率 ----
    # freq[mod] = (触及文件集合, 提交数, 文件 touch 次数)；后两者分别按
    # 「提交×模块」「提交×文件」去重口径统计，避免一次大 commit 虚高。
    freq: dict[str, tuple[set[str], int, int]] = {}
    for _, files in commits:
        seen: set[str] = set()
        for p, _, _ in files:
            mod = logical_module(p)
            seen.add(mod)
            files_set, commit_n, touch_n = freq.get(mod, (set(), 0, 0))
            files_set.add(normalize(p))
            freq[mod] = (files_set, commit_n, touch_n + 1)
        for mod in seen:
            files_set, commit_n, touch_n = freq[mod]
            freq[mod] = (files_set, commit_n + 1, touch_n)

    total = len(commits)
    print(f"# 接入成本度量（{since} ~ {date.today().isoformat()}，{total} 个非 merge 提交）")
    print()
    print("## 每提交触及面")
    print()
    print("| 指标 | 值 |")
    print("|---|---|")
    print(f"| 提交总数（不含 merge） | {total} |")
    print(f"| 平均每提交触及文件数 | {sum(file_counts) / total:.1f} |")
    print(f"| 平均每提交触及逻辑模块数 | {sum(module_counts) / total:.1f} |")
    print(f"| 平均每提交触及 workspace crate 数 | {sum(crate_counts) / total:.2f} |")
    print(f"| 平均每提交测试行变更（增+删） | {sum(test_line_counts) / total:.1f} |")
    print(f"| 单提交最大触及文件数 | {max(file_counts)} |")
    print(f"| 单提交最大测试行变更 | {max(test_line_counts)} |")
    print()
    print("## 逻辑模块提交频率（前 %d）" % min(args.top, len(freq)))
    print()
    print("| 逻辑模块 | 提交数 | 触及文件数 | 文件 touch 次数 |")
    print("|---|---|---|---|")
    for mod, (files_set, commit_n, touch_n) in sorted(
        freq.items(), key=lambda kv: (-kv[1][1], kv[0])
    )[: args.top]:
        print(f"| {mod} | {commit_n} | {len(files_set)} | {touch_n} |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
