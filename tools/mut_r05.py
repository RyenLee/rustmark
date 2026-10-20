#!/usr/bin/env python3
"""R-05 变异验证：把「0 / 空串 → 未设」的归一分支改坏，确认单测当场变红。

只动 `src/presentation/state/settings_ui_state.rs` 的纯函数
`normalize_writing_goal`，用例是该模块的 `normalize_writing_goal_treats_*`。

每个变异从干净基准重新生成、跑目标单测、finally 还原。
失败形态（与 mut_r04.py 同口径）：
- captured：期望的用例变红；
- NO_TEETH：用例仍绿（守卫失效，最严重）；
- COMPILE_FAIL / ANCHOR_MISS / NO_RESULT：变异本身无效。
"""
import io
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "src", "presentation", "state", "settings_ui_state.rs")
TEST = "normalize_writing_goal_treats_zero_and_empty_as_unset"

# (名称, 旧串, 新串)
MUTANTS = [
    (
        "显式 0 不再归一到未设（敲 0 仍保留原目标）",
        "        Ok(0) => 0,",
        "        Ok(0) => current,",
    ),
    (
        "空串不再归一到未设（清空输入框仍保留原目标）",
        "    if raw.is_empty() {\n        return 0;\n    }",
        "    if raw.is_empty() {\n        return current;\n    }",
    ),
]


def read(p):
    with io.open(p, encoding="utf-8", newline="") as f:
        return f.read()


def write(p, s):
    with io.open(p, "w", encoding="utf-8", newline="") as f:
        f.write(s)


def run_tests():
    r = subprocess.run(
        ["cargo", "test", "--lib", TEST],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    out = r.stdout + r.stderr
    if "error[" in out:
        return "COMPILE_FAIL", out
    if "test result:" not in out:
        return "NO_RESULT", out
    line = [l for l in out.splitlines() if l.startswith("test result:")][-1]
    return ("PASS" if "0 failed" in line else "FAIL"), out


def main():
    base = read(SRC).replace("\r\n", "\n")
    rows = []
    try:
        for name, old, new in MUTANTS:
            write(SRC, base)
            cur = read(SRC)
            if old not in cur:
                rows.append((name, "ANCHOR_MISS", "锚点未命中：变异未生效"))
                continue
            write(SRC, cur.replace(old, new, 1))
            status, out = run_tests()
            if status == "COMPILE_FAIL":
                rows.append((name, "COMPILE_FAIL", "编译失败——变异无效"))
            elif status == "NO_RESULT":
                rows.append((name, "NO_RESULT", "无 test result 行"))
            else:
                caught = f"{TEST} ... FAILED" in out
                if caught:
                    rows.append((name, "captured", status))
                else:
                    failed = [l.strip() for l in out.splitlines() if " ... FAILED" in l]
                    note = "NO_TEETH；实际失败：" + "; ".join(failed) if failed else "NO_TEETH（无任何测试失败）"
                    rows.append((name, "NO_TEETH", note))
    finally:
        write(SRC, base)

    print(f"{'变异':<44} {'结果':<14} 测试状态")
    print("-" * 84)
    teeth = 0
    for name, res, note in rows:
        print(f"{name:<44} {res:<14} {note}")
        if res == "captured":
            teeth += 1
    print("-" * 84)
    print(f"共 {len(rows)} 轮，捕获 {teeth}，无牙齿 {len(rows) - teeth}")
    return 0 if teeth == len(rows) else 1


if __name__ == "__main__":
    sys.exit(main())
