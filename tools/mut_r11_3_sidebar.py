#!/usr/bin/env python3
"""R-11 子批 3（sidebar 组）变异验证：把新增的写入口改坏，确认用例当场变红。

只动 `src/app/frame/sidebar.rs`，用例在 `tests/presentation/sidebar_view_test.rs`
末尾「R-11 子批 3 · sidebar 组」那一块（一律走 `EditorApp` 方法入口，无自指）。

每个变异从干净基准重新生成、跑目标测试、finally 还原。

失败形态（沿用 mut_r04 / mut_r11_1 / mut_r11_2 / mut_r11_3_editor 口径）：
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
SRC = os.path.join(ROOT, "src", "app", "frame", "sidebar.rs")

# (名称, 旧串, 新串, 期望变红的用例真实名)
MUTANTS = [
    (
        "恢复新建草稿：方法改空实现",
        "    pub fn restore_create_draft(&mut self, parent: String, name: String, is_directory: bool) {\n"
        "        self.sidebar.creating = Some((parent, name, is_directory));\n"
        "    }",
        "    pub fn restore_create_draft(&mut self, parent: String, name: String, is_directory: bool) {\n"
        "        let _ = (parent, name, is_directory);\n"
        "    }",
        "restoring_the_create_draft_puts_back_what_the_user_typed",
    ),
    (
        "恢复重命名草稿：方法改空实现",
        "    pub fn restore_rename_draft(&mut self, path: String, name: String) {\n"
        "        self.sidebar.renaming_path = Some(path);\n"
        "        self.sidebar.rename_draft = name;\n"
        "    }",
        "    pub fn restore_rename_draft(&mut self, path: String, name: String) {\n"
        "        let _ = (path, name);\n"
        "    }",
        "restoring_a_draft_keeps_the_error_and_the_menu",
    ),
    (
        "单选高亮：方法改空实现",
        "    pub fn select_sidebar_path(&mut self, path: String) {\n"
        "        self.sidebar.selected_path = Some(path);\n"
        "    }",
        "    pub fn select_sidebar_path(&mut self, path: String) {\n"
        "        let _ = path;\n"
        "    }",
        "selecting_a_row_highlights_without_opening_a_document",
    ),
    (
        "大纲选中：方法改空实现",
        "    pub fn select_outline_item(&mut self, text: String) {\n"
        "        self.sidebar.outline_selected = Some(text);\n"
        "    }",
        "    pub fn select_outline_item(&mut self, text: String) {\n"
        "        let _ = text;\n"
        "    }",
        "selecting_an_outline_item_highlights_it",
    ),
    (
        "搜索词回写：方法改空实现",
        "    pub fn set_search_query(&mut self, query: String) {\n"
        "        self.sidebar.search_query = query;\n"
        "    }",
        "    pub fn set_search_query(&mut self, query: String) {\n"
        "        let _ = query;\n"
        "    }",
        "setting_the_search_query_updates_the_field",
    ),
    (
        "搜索范围：方法改空实现",
        "    pub fn set_search_scope(&mut self, scope: SearchScope) {\n"
        "        self.sidebar.search_scope = scope;\n"
        "    }",
        "    pub fn set_search_scope(&mut self, scope: SearchScope) {\n"
        "        let _ = scope;\n"
        "    }",
        "switching_the_search_scope_takes_effect",
    ),
    (
        "搜索开关：翻转改空实现（点了没反应）",
        "    pub fn toggle_search_option(&mut self, which: SearchToggle) {\n"
        "        self.sidebar.search_options = self.sidebar.search_options.toggled(which);\n"
        "    }",
        "    pub fn toggle_search_option(&mut self, which: SearchToggle) {\n"
        "        let _ = which;\n"
        "    }",
        "toggling_a_search_option_flips_both_ways",
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
        ["cargo", "test", "--test", "presentation", "sidebar_view_test"],
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
