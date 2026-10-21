#!/usr/bin/env python3
"""R-11 子批 3（preview 组）变异验证：把新增的写入口改坏，确认用例当场变红。

只动 `src/app/frame/preview.rs`，用例在 `tests/presentation/scroll_sync_test.rs`
末尾「R-11 子批 3 · preview 组」那一块（一律走 `EditorApp` 方法入口）。

第 4 轮是**部分实现**变异：只写 blocks、漏写 rects 与 scroll_y——
正是 `store_preview_render` 三字段一起落定要防的无声缺陷。

失败形态口径同 mut_r11_3_sidebar。
"""
import io
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "src", "app", "frame", "preview.rs")

# (名称, 旧串, 新串, 期望变红的用例真实名)
MUTANTS = [
    (
        "块序列写入：方法改空实现",
        "    pub fn set_preview_blocks(&mut self, blocks: Vec<BlockView>) {\n"
        "        self.preview.blocks = blocks;\n"
        "    }",
        "    pub fn set_preview_blocks(&mut self, blocks: Vec<BlockView>) {\n"
        "        let _ = blocks;\n"
        "    }",
        "set_preview_blocks_replaces_the_cached_sequence",
    ),
    (
        "帧末落定：只写 blocks，漏写 rects 与 scroll_y",
        "        self.preview.blocks = blocks;\n"
        "        self.preview.rects = rects;\n"
        "        self.preview.scroll_y = scroll_y;",
        "        self.preview.blocks = blocks;\n"
        "        let _ = (rects, scroll_y);",
        "store_preview_render_lands_all_three_fields_together",
    ),
    (
        "帧末落定：方法改空实现",
        "    pub fn store_preview_render(\n"
        "        &mut self,\n"
        "        blocks: Vec<BlockView>,\n"
        "        rects: Vec<BlockRect>,\n"
        "        scroll_y: f32,\n"
        "    ) {\n"
        "        self.preview.blocks = blocks;\n"
        "        self.preview.rects = rects;\n"
        "        self.preview.scroll_y = scroll_y;\n"
        "    }",
        "    pub fn store_preview_render(\n"
        "        &mut self,\n"
        "        blocks: Vec<BlockView>,\n"
        "        rects: Vec<BlockRect>,\n"
        "        scroll_y: f32,\n"
        "    ) {\n"
        "        let _ = (blocks, rects, scroll_y);\n"
        "    }",
        "store_preview_render_lands_all_three_fields_together",
    ),
    (
        "滚动偏移：方法改空实现",
        "    pub fn set_preview_scroll_y(&mut self, y: f32) {\n"
        "        self.preview.scroll_y = y;\n"
        "    }",
        "    pub fn set_preview_scroll_y(&mut self, y: f32) {\n"
        "        let _ = y;\n"
        "    }",
        "set_preview_scroll_y_writes_the_offset_only",
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
        ["cargo", "test", "--test", "presentation", "scroll_sync_test"],
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
