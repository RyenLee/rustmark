#!/usr/bin/env python3
"""R-11 子批 1 变异验证：把新增的导出状态写入口改坏，确认用例当场变红。

只动 `src/app/frame/export.rs`，用例在 `tests/presentation/export_view_test.rs`。

每个变异从干净基准重新生成、跑目标测试、finally 还原。

失败形态（沿用 mut_r04/mut_r05 口径）：
- captured：期望的用例变红；
- NO_TEETH：用例仍绿（守卫失效，最严重）；
- captured_build：build.rs 的架构校验先 panic，用例没机会跑；
- COMPILE_FAIL / ANCHOR_MISS / NO_RESULT：变异本身无效。

锚点一律带上**函数名**：`export.rs` 里多个方法写同一个字段
（如 `include_toc` 同时出现在 `set_export_include_toc` 与
`set_pdf_include_toc`），只写赋值那一行会命中第一个匹配。
"""
import io
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "src", "app", "frame", "export.rs")

# (名称, 旧串, 新串, 期望变红的用例真实名)
# 用例名只写 `模块名::测试名` 的后半段：集成测试输出带 `export_view_test::` 前缀。
MUTANTS = [
    (
        "长图开关：方法改空实现",
        "    pub fn set_export_long_image(&mut self, long: bool) {\n"
        "        self.export_ui.long_image = long;\n"
        "    }",
        "    pub fn set_export_long_image(&mut self, long: bool) {\n"
        "        let _ = long;\n"
        "    }",
        "long_image_flag_is_written_through_the_app_method",
    ),
    (
        "目录开关：方法改空实现",
        "    pub fn set_export_include_toc(&mut self, on: bool) {\n"
        "        self.export_ui.include_toc = on;\n"
        "    }",
        "    pub fn set_export_include_toc(&mut self, on: bool) {\n"
        "        let _ = on;\n"
        "    }",
        "include_toc_flag_is_written_through_the_app_method",
    ),
    (
        "内联图片开关：方法改空实现",
        "    pub fn set_export_embed_images(&mut self, embed: bool) {\n"
        "        self.export_ui.embed_images = embed;\n"
        "    }",
        "    pub fn set_export_embed_images(&mut self, embed: bool) {\n"
        "        let _ = embed;\n"
        "    }",
        "embed_images_flag_is_written_through_the_app_method",
    ),
    (
        "纸张尺寸：方法改空实现",
        "    pub fn set_export_page_size(&mut self, size: PageSize) {\n"
        "        self.export_ui.page_size = size;\n"
        "    }",
        "    pub fn set_export_page_size(&mut self, size: PageSize) {\n"
        "        let _ = size;\n"
        "    }",
        "page_size_is_written_through_the_app_method",
    ),
    (
        "分页开关：方法改空实现",
        "    pub fn set_export_paginate(&mut self, paginate: bool) {\n"
        "        self.export_ui.paginate = paginate;\n"
        "    }",
        "    pub fn set_export_paginate(&mut self, paginate: bool) {\n"
        "        let _ = paginate;\n"
        "    }",
        "pagination_flag_is_written_through_the_app_method",
    ),
    (
        "PDF 目录联动：丢掉「勾上才补分页」",
        "        self.export_ui.include_toc = toc;\n"
        "        if toc {\n"
        "            self.export_ui.paginate = true;\n"
        "        }",
        "        self.export_ui.include_toc = toc;",
        "pdf_toc_implies_pagination",
    ),
    (
        "PDF 目录联动：改成无条件补分页",
        "        if toc {\n"
        "            self.export_ui.paginate = true;\n"
        "        }",
        "        self.export_ui.paginate = true;",
        "unchecking_pdf_toc_leaves_pagination_alone",
    ),
    (
        "打开浮层：不再填建议文件名",
        "        self.export_ui.file_stem = stem;",
        "        let _ = stem;",
        "opening_export_fills_the_suggested_name_theme_and_directory",
    ),
    (
        "打开浮层：不再填导出主题",
        "        self.export_ui.theme = theme_kind.to_string();",
        "        let _ = theme_kind;",
        "opening_export_fills_the_suggested_name_theme_and_directory",
    ),
    (
        "打开浮层：不再填默认输出目录",
        "        self.export_ui.output_dir = self.active_document_dir().unwrap_or_default();",
        "        let _ = self.active_document_dir();",
        "opening_export_fills_the_suggested_name_theme_and_directory",
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
        ["cargo", "test", "--test", "presentation", "export_view_test"],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    out = r.stdout + r.stderr
    if "error[" in out:
        return "COMPILE_FAIL", out
    if "test result:" not in out:
        # 无 result 行且输出含 error：多半是 build.rs 的架构校验先 panic，
        # 或变异导致编译以 `error:`（非 `error[`）形态失败。
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

    print(f"{'变异':<38} {'结果':<16} 说明")
    print("-" * 96)
    teeth = 0
    for name, res, note in rows:
        print(f"{name:<38} {res:<16} {note}")
        if res == "captured":
            teeth += 1
    print("-" * 96)
    print(f"共 {len(rows)} 轮，捕获 {teeth}，无牙齿 {len(rows) - teeth}")
    return 0 if teeth == len(rows) else 1


if __name__ == "__main__":
    sys.exit(main())
