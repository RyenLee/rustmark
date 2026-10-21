#!/usr/bin/env python3
"""R-11 子批 3（ui 组）变异验证：把新增的写入口改坏，确认用例当场变红。

只动 `src/app/frame/ui.rs`，用例在 `tests/app/ui_state_writes_test.rs`
（一律走 `EditorApp` 方法入口）。

失败形态口径同 mut_r11_3_preview：捕获 / 无牙齿 / 锚点未命中 / 编译失败。
"""
import io
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "src", "app", "frame", "ui.rs")

# (名称, 旧串, 新串, 期望变红的用例真实名)
MUTANTS = [
    (
        "分割线拖拽：方法改空实现",
        "    pub fn set_split_dragging(&mut self, dragging: bool) {\n"
        "        self.ui.split_dragging = dragging;\n"
        "    }",
        "    pub fn set_split_dragging(&mut self, dragging: bool) {\n"
        "        let _ = dragging;\n"
        "    }",
        "split_dragging_is_written_through_the_app_method",
    ),
    (
        "侧栏拖拽：方法改空实现",
        "    pub fn set_sidebar_dragging(&mut self, dragging: bool) {\n"
        "        self.ui.sidebar_dragging = dragging;\n"
        "    }",
        "    pub fn set_sidebar_dragging(&mut self, dragging: bool) {\n"
        "        let _ = dragging;\n"
        "    }",
        "sidebar_dragging_is_written_through_the_app_method",
    ),
    (
        "标签条激活项：方法改空实现",
        "    pub fn set_tabstrip_last_active(&mut self, id: Option<u64>) {\n"
        "        self.ui.tabstrip_last_active = id;\n"
        "    }",
        "    pub fn set_tabstrip_last_active(&mut self, id: Option<u64>) {\n"
        "        let _ = id;\n"
        "    }",
        "tabstrip_last_active_is_written_through_the_app_method",
    ),
    (
        "标签条滚动偏移：方法改空实现",
        "    pub fn set_tabstrip_scroll_x(&mut self, x: f32) {\n"
        "        self.ui.tabstrip_scroll_x = x;\n"
        "    }",
        "    pub fn set_tabstrip_scroll_x(&mut self, x: f32) {\n"
        "        let _ = x;\n"
        "    }",
        "tabstrip_scroll_x_is_written_through_the_app_method",
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
        ["cargo", "test", "--test", "app", "ui_state_writes_test"],
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

    print(f"{'变异':<36} {'结果':<16} 说明")
    print("-" * 96)
    teeth = 0
    for name, res, note in rows:
        print(f"{name:<36} {res:<16} {note}")
        if res == "captured":
            teeth += 1
    print("-" * 96)
    print(f"共 {len(rows)} 轮，捕获 {teeth}，无牙齿 {len(rows) - teeth}")
    return 0 if teeth == len(rows) else 1


if __name__ == "__main__":
    sys.exit(main())
