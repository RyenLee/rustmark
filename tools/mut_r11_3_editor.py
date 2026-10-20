#!/usr/bin/env python3
"""R-11 子批 3（editor 组）变异验证：把新增的写入口改坏，确认用例当场变红。

只动 `src/app/frame/editor.rs` 末尾「R-11 子批 3（editor 组）」那一块，
用例在 `tests/app/editor_state_writes_test.rs`。

每个变异从干净基准重新生成、跑目标测试、finally 还原。

失败形态（沿用 mut_r04 / mut_r11_1 / mut_r11_2 口径）：
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
SRC = os.path.join(ROOT, "src", "app", "frame", "editor.rs")

# (名称, 旧串, 新串, 期望变红的用例真实名)
MUTANTS = [
    (
        "光标/选区：方法改空实现",
        "    pub fn set_editor_selection(&mut self, cursor: usize, anchor: Option<usize>) {\n"
        "        self.editor.cursor = cursor;\n"
        "        self.editor.anchor = anchor;\n"
        "    }",
        "    pub fn set_editor_selection(&mut self, cursor: usize, anchor: Option<usize>) {\n"
        "        let _ = (cursor, anchor);\n"
        "    }",
        "editor_selection_is_written_through_the_app_method",
    ),
    (
        "拼写菜单：开菜单改空实现",
        "    pub fn set_spell_menu(&mut self, menu: Option<SpellSuggestMenu>) {\n"
        "        self.editor.spell_menu = menu;\n"
        "    }",
        "    pub fn set_spell_menu(&mut self, menu: Option<SpellSuggestMenu>) {\n"
        "        let _ = menu;\n"
        "    }",
        "spell_menu_opens_through_the_app_method",
    ),
    (
        "拼写菜单：关闭改空实现",
        "    pub fn close_spell_menu(&mut self) {\n"
        "        self.editor.spell_menu = None;\n"
        "    }",
        "    pub fn close_spell_menu(&mut self) {\n"
        "    }",
        "spell_menu_closes_through_the_app_method",
    ),
    (
        "滚动偏移：方法改空实现",
        "    pub fn set_editor_scroll_y(&mut self, y: f32) {\n"
        "        self.editor.scroll_y = y;\n"
        "    }",
        "    pub fn set_editor_scroll_y(&mut self, y: f32) {\n"
        "        let _ = y;\n"
        "    }",
        "editor_scroll_is_written_through_the_app_method",
    ),
    (
        "行号开关：方法改空实现",
        "    pub fn set_line_numbers(&mut self, on: bool) {\n"
        "        self.editor.line_numbers = on;\n"
        "    }",
        "    pub fn set_line_numbers(&mut self, on: bool) {\n"
        "        let _ = on;\n"
        "    }",
        "line_numbers_flag_is_written_through_the_app_method",
    ),
    (
        "拼写检查：切换改空实现（点了没反应）",
        "    pub fn toggle_spell_check(&mut self) {\n"
        "        self.editor.spell_check = !self.editor.spell_check;\n"
        "    }",
        "    pub fn toggle_spell_check(&mut self) {\n"
        "    }",
        "spell_check_toggles_through_the_app_method",
    ),
    (
        "自动换行：切换改空实现（点了没反应）",
        "    pub fn toggle_word_wrap(&mut self) {\n"
        "        self.editor.word_wrap = !self.editor.word_wrap;\n"
        "    }",
        "    pub fn toggle_word_wrap(&mut self) {\n"
        "    }",
        "word_wrap_toggles_through_the_app_method",
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
        ["cargo", "test", "--test", "app", "editor_state_writes"],
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
