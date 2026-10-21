#!/usr/bin/env python3
"""R-11 子批 2 变异验证：把新增的差异对比写入口改坏，确认用例当场变红。

只动 `src/app/frame/diff.rs`，用例在 `tests/presentation/s6_11f_test.rs`。

每个变异从干净基准重新生成、跑目标测试、finally 还原。

失败形态（沿用 mut_r04 / mut_r11_1 口径）：
- captured：期望的用例变红；
- NO_TEETH：用例仍绿（守卫失效，最严重）；
- captured_build：build.rs 的架构校验先 panic，用例没机会跑；
- COMPILE_FAIL / ANCHOR_MISS / NO_RESULT：变异本身无效。
"""
import io
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "src", "app", "frame", "diff.rs")

# (名称, 旧串, 新串, 期望变红的用例真实名)
# 用例名只写 `模块名::测试名` 的后半段：集成测试输出带 `s6_11f_test::` 前缀。
MUTANTS = [
    (
        "错误记录：方法改空实现",
        "    pub fn set_diff_error(&mut self, message: impl Into<String>) {\n"
        "        self.diff_ui.error = Some(message.into());\n"
        "    }",
        "    pub fn set_diff_error(&mut self, message: impl Into<String>) {\n"
        "        let _ = message;\n"
        "    }",
        "diff_read_failure_is_recorded_through_the_app_method",
    ),
    (
        "换一个文件：方法改空实现",
        "        self.diff_ui.right_path = None;\n"
        "        self.diff_ui.rows.clear();",
        "        // 空实现",
        "swapping_the_diff_right_file_clears_both_the_choice_and_the_rows",
    ),
    (
        "换一个文件：只清选择、不清结果行",
        "        self.diff_ui.right_path = None;\n"
        "        self.diff_ui.rows.clear();",
        "        self.diff_ui.right_path = None;",
        "swapping_the_diff_right_file_clears_both_the_choice_and_the_rows",
    ),
    (
        "打开面板：方法改空实现（不再清错误）",
        "    pub fn open_diff_overlay(&mut self) {\n"
        "        if self.diff_ui.right_path.is_none() {\n"
        "            self.diff_ui.error = None;\n"
        "        }\n"
        "    }",
        "    pub fn open_diff_overlay(&mut self) {\n"
        "    }",
        "opening_diff_without_a_right_side_clears_the_stale_error",
    ),
    (
        "打开面板：清错误改成无条件",
        "        if self.diff_ui.right_path.is_none() {\n"
        "            self.diff_ui.error = None;\n"
        "        }",
        "        self.diff_ui.error = None;",
        "opening_diff_with_a_kept_right_side_keeps_the_error",
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
        ["cargo", "test", "--test", "presentation", "diff"],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    out = r.stdout + r.stderr
    if "error[" in out:
        return "COMPILE_FAIL", out
    if "test result:" not in out:
        if "error" in out:
            return "COMPILE_FAIL", out
        return "captured_build", out
    return "RAN", out


def main():
    base = read(SRC).replace("\r\n", "\n")
    rows = []
    try:
        for name, old, new, expect in MUTANTS:
            write(SRC, base)
            cur = read(SRC)
            if old not in cur:
                rows.append((name, "ANCHOR_MISS", "锚点未命中：变异未生效"))
                continue
            write(SRC, cur.replace(old, new, 1))
            status, out = run_tests()
            if status != "RAN":
                rows.append((name, status, "变异无效或没跑起来"))
                continue
            if f"{expect} ... FAILED" in out:
                rows.append((name, "captured", expect))
            else:
                failed = [l.strip() for l in out.splitlines() if " ... FAILED" in l]
                note = (
                    "NO_TEETH；实际失败：" + "; ".join(failed[:4])
                    if failed
                    else "NO_TEETH（无任何测试失败）"
                )
                rows.append((name, "NO_TEETH", note))
    finally:
        write(SRC, base)

    print(f"{'变异':<40} {'结果':<16} 说明")
    print("-" * 96)
    teeth = 0
    for name, res, note in rows:
        print(f"{name:<40} {res:<16} {note}")
        if res == "captured":
            teeth += 1
    print("-" * 96)
    print(f"共 {len(rows)} 轮，捕获 {teeth}，无牙齿 {len(rows) - teeth}")
    return 0 if teeth == len(rows) else 1


if __name__ == "__main__":
    sys.exit(main())
