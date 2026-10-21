#!/usr/bin/env python3
"""R-11 子批 3（收尾四组：settings_ui / find / status_bar / shortcut_overview）变异验证。

把新增的写入口改坏，确认用例当场变红。只动对应 `src/app/frame/*.rs`，用例在
`tests/app/remaining_state_writes_test.rs`（一律走 `EditorApp` 方法入口）。

每个变异编辑一个独立源文件，跑完即还原；失败形态口径同 mut_r11_3_preview：
捕获 / 无牙齿 / 锚点未命中 / 编译失败。
"""
import io
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = {
    "settings": os.path.join(ROOT, "src", "app", "frame", "settings.rs"),
    "find": os.path.join(ROOT, "src", "app", "frame", "find.rs"),
    "status_bar": os.path.join(ROOT, "src", "app", "frame", "status_bar.rs"),
    "shortcut_overview": os.path.join(ROOT, "src", "app", "frame", "shortcut_overview.rs"),
}

# (名称, 源键, 旧串, 新串, 期望变红的用例真实名)
MUTANTS = [
    (
        "设置分区：方法改空实现",
        "settings",
        "    pub fn set_settings_section(&mut self, section: SettingsTab) {\n"
        "        self.settings_ui.section = section;\n"
        "    }",
        "    pub fn set_settings_section(&mut self, section: SettingsTab) {\n"
        "        let _ = section;\n"
        "    }",
        "settings_section_is_written_through_the_app_method",
    ),
    (
        "清错误/提示：方法改空实现",
        "settings",
        "    pub fn clear_settings_messages(&mut self) {\n"
        "        self.settings_ui.error = None;\n"
        "        self.settings_ui.notice = None;\n"
        "    }",
        "    pub fn clear_settings_messages(&mut self) {\n"
        "        let _ = ();\n"
        "    }",
        "settings_messages_clear_through_the_app_method",
    ),
    (
        "清待确认冲突：方法改空实现",
        "settings",
        "    pub fn clear_settings_pending_conflict(&mut self) {\n"
        "        self.settings_ui.pending_conflict = None;\n"
        "    }",
        "    pub fn clear_settings_pending_conflict(&mut self) {\n"
        "        let _ = ();\n"
        "    }",
        "settings_pending_conflict_clears_through_the_app_method",
    ),
    (
        "查找焦点：方法改空实现",
        "find",
        "    pub fn set_find_focused(&mut self, focused: bool) {\n"
        "        self.find.find_focused = focused;\n"
        "    }",
        "    pub fn set_find_focused(&mut self, focused: bool) {\n"
        "        let _ = focused;\n"
        "    }",
        "find_focused_is_written_through_the_app_method",
    ),
    (
        "状态栏编码：方法改空实现",
        "status_bar",
        "    pub fn set_status_bar_encoding(&mut self, encoding: Option<TextEncoding>) {\n"
        "        self.status_bar.encoding = encoding;\n"
        "    }",
        "    pub fn set_status_bar_encoding(&mut self, encoding: Option<TextEncoding>) {\n"
        "        let _ = encoding;\n"
        "    }",
        "status_bar_encoding_is_written_through_the_app_method",
    ),
    (
        "快捷键总览筛选：方法改空实现",
        "shortcut_overview",
        "    pub fn set_shortcut_overview_query(&mut self, query: String) {\n"
        "        self.shortcut_overview_ui.query = query;\n"
        "    }",
        "    pub fn set_shortcut_overview_query(&mut self, query: String) {\n"
        "        let _ = query;\n"
        "    }",
        "shortcut_overview_query_is_written_through_the_app_method",
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
        ["cargo", "test", "--test", "app", "remaining_state_writes_test"],
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
    bases = {k: read(v).replace("\r\n", "\n") for k, v in SRC.items()}
    rows = []
    try:
        for name, key, old, new, expect in MUTANTS:
            write(SRC[key], bases[key])
            cur = read(SRC[key])
            if old not in cur:
                rows.append((name, "ANCHOR_MISS", "锚点未命中：变异未生效"))
                continue
            write(SRC[key], cur.replace(old, new, 1))
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
        for k, v in SRC.items():
            write(v, bases[k])

    print(f"{'变异':<34} {'结果':<16} 说明")
    print("-" * 96)
    teeth = 0
    for name, res, note in rows:
        print(f"{name:<34} {res:<16} {note}")
        if res == "captured":
            teeth += 1
    print("-" * 96)
    print(f"共 {len(rows)} 轮，捕获 {teeth}，无牙齿 {len(rows) - teeth}")
    return 0 if teeth == len(rows) else 1


if __name__ == "__main__":
    sys.exit(main())
