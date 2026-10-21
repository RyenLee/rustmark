#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""R-06 变异验证：证明 tab_cursor 三层判据对「切标签丢光标」这个 bug 有牙齿。

把 `switch_document` 里的 `restore_active_doc_cursor()` 换回旧行为
（硬置 `cursor = 0` / `anchor = None`、不写 `pending_cursor`），
三个用例必须**全部**变红——漏红一个即该判据无牙齿。

分类：CAPTURED（全红，有牙齿）/ NO_TEETH（有测试没红）/
      COMPILE_FAIL（变异破坏了编译，判据无从谈起）。
"""

import subprocess
import sys

ROOT = "."
TABS = "src/app/frame/tabs.rs"

ANCHOR = (
    "        // R-06：恢复目标文档的光标 / 选区，取代原先的「硬置 0」。\n"
    "        self.restore_active_doc_cursor();"
)
MUTANT = (
    "        // [变异 R-06] 模拟旧行为：硬置 0、不写 pending_cursor\n"
    "        self.editor.cursor = 0;\n"
    "        self.editor.anchor = None;"
)

TESTS = [
    "switching_tabs_restores_per_document_cursor",
    "switching_back_schedules_pending_cursor_then_frame_consumes_it",
    "switching_back_resets_the_egui_caret_to_the_restored_position",
]


def run_tests() -> str:
    proc = subprocess.run(
        ["cargo", "test", "--test", "app", "tab_cursor"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    return proc.stdout + proc.stderr


def main() -> int:
    with open(TABS, "rb") as f:
        raw = f.read().decode("utf-8")
    crlf = "\r\n" in raw
    text = raw.replace("\r\n", "\n") if crlf else raw

    if text.count(ANCHOR) != 1:
        print(f"[错误] 锚点命中 {text.count(ANCHOR)} 次（应为 1）；tabs.rs 与预期不符。")
        return 2

    mutated = text.replace(ANCHOR, MUTANT)
    out_text = mutated.replace("\n", "\r\n") if crlf else mutated

    try:
        with open(TABS, "w", encoding="utf-8", newline="") as f:
            f.write(out_text)

        out = run_tests()

        if ("error[" in out or "error:" in out) and "test result:" not in out:
            print("结果：COMPILE_FAIL —— 变异破坏了编译，判据无从谈起。")
            return 1

        greens = [t for t in TESTS if f"{t} ... FAILED" in out]
        reds = [t for t in TESTS if f"{t} ... ok" in out]

        if len(greens) == len(TESTS):
            print(f"结果：CAPTURED —— {len(greens)}/{len(TESTS)} 全红，判据有牙齿。")
            for t in greens:
                print(f"  ✓ {t}")
            return 0

        print(f"结果：NO_TEETH —— 应变红 {len(TESTS)}，实红 {len(greens)}。")
        for t in reds:
            print(f"  ✗ 未变红：{t}（该判据无牙齿）")
        return 1
    finally:
        with open(TABS, "w", encoding="utf-8", newline="") as f:
            f.write(raw)
        print("已还原 tabs.rs。")


if __name__ == "__main__":
    sys.exit(main())
