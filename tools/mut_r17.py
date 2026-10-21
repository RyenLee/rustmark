#!/usr/bin/env python3
"""R-17 变异验证：破坏 widgets 收口的两处配方，确认用例当场变红。

用法：python tools/mut_r17.py

变异点：
  1. text.rs 的 `elided_segment` 高度归 0 —— 复现「0 高子矩形」反模式，
     搜索分组头文件名段应被压到行中点之下，对齐测试必须变红。
  2. row.rs 的 `paint_row_background` 选中态底色改错（accent_soft→bg_tertiary），
     选中态单测必须变红。
  3. row.rs 的 `paint_row_background` 悬停态底色改错（bg_tertiary→accent_soft），
     悬停态单测必须变红。

纪律（与 mut_r12.py / mut_arch_check.py 一致）：备份 → 改动 → 跑测试 → 还原
（try/finally），处理 CRLF，识别 `error[`≠无牙齿、try/finally 还原、锚点未命中
从干净基准重取。检测串用 `f"{expect} ... FAILED"`（容忍任意模块前缀，见项目记忆
关于变异脚本必须匹配测试名后半段的条目）。
"""
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET_TEXT = ROOT / "src" / "presentation" / "widgets" / "text.rs"
TARGET_ROW = ROOT / "src" / "presentation" / "widgets" / "row.rs"

# (名称, 目标文件, 锚点原文, 替换成, 期望变红的用例名子串, cargo test 参数)
MUTATIONS = [
    (
        "R17 elided_segment 高度归 0",
        TARGET_TEXT,
        "        egui::vec2(avail, measured.height()),",
        "        egui::vec2(avail, 0.0),",
        ["search_group_header_filename_and_count_share_one_baseline"],
        ["--test", "presentation", "search_group_header_filename_and_count_share_one_baseline"],
    ),
    (
        "R17 选中底色改错色 (accent_soft→bg_tertiary)",
        TARGET_ROW,
        "        painter.rect_filled(rect, egui::CornerRadius::same(4), theme.accent_soft);",
        "        painter.rect_filled(rect, egui::CornerRadius::same(4), theme.bg_tertiary);",
        ["paint_row_background_selected_draws_accent_soft"],
        ["--lib", "paint_row_background"],
    ),
    (
        "R17 悬停底色改错色 (bg_tertiary→accent_soft)",
        TARGET_ROW,
        "        painter.rect_filled(rect, egui::CornerRadius::same(4), theme.bg_tertiary);",
        "        painter.rect_filled(rect, egui::CornerRadius::same(4), theme.accent_soft);",
        ["paint_row_background_hovered_draws_bg_tertiary"],
        ["--lib", "paint_row_background"],
    ),
]


def run_tests(args: list[str]) -> tuple[bool, str]:
    proc = subprocess.run(
        ["cargo", "test", *args],
        cwd=ROOT, capture_output=True, text=True, timeout=900,
    )
    out = proc.stdout + proc.stderr
    return proc.returncode == 0, out


def main() -> int:
    failures = []
    for name, target, anchor, replacement, expected_red, test_args in MUTATIONS:
        backup = target.with_name(target.name + ".r17.bak")
        shutil.copy2(target, backup)
        try:
            raw = backup.read_bytes()
            crlf = b"\r\n" in raw
            a = anchor.replace("\n", "\r\n") if crlf else anchor
            r = replacement.replace("\n", "\r\n") if crlf else replacement
            if a.encode() not in raw:
                print(f"[SKIP] {name}: 锚点未命中（可能已被其它改动影响）")
                failures.append(name)
                continue
            mutated = raw.replace(a.encode(), r.encode(), 1)
            if mutated == raw:
                print(f"[SKIP] {name}: 替换无效果")
                failures.append(name)
                continue
            target.write_bytes(mutated)
            try:
                ok, out = run_tests(test_args)
            finally:
                shutil.copy2(backup, target)
            if "error[" in out:
                print(f"[WEAK] {name}: 变异导致编译失败——不能算「有牙齿」")
                failures.append(name)
                continue
            if ok:
                print(f"[WEAK] {name}: 破坏后测试仍全绿——守卫没有牙齿！")
                failures.append(name)
                continue
            missing = [
                t for t in expected_red
                if f"{t} ... FAILED" not in out
            ]
            if missing:
                print(f"[PART] {name}: 变红但与预期不符，未变红：{missing}")
                failures.append(name)
            else:
                print(f"[OK]   {name}: {len(expected_red)} 条预期用例全部变红")
        finally:
            backup.unlink(missing_ok=True)
    print()
    if failures:
        print(f"R-17 变异验证失败 {len(failures)} 项：{failures}")
        return 1
    print(f"R-17 变异验证通过：{len(MUTATIONS)}/{len(MUTATIONS)} 个变异点都被用例当场抓住。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
