#!/usr/bin/env python3
"""R-13 变异验证：把 presentation 内部方向规则改坏，确认新用例当场变红。

方案点名的变异：把 state 的禁止清单清空 → 注入用例红。
另补 5 个方向：views→state 反向也禁（过度收紧）/ rich 名单漏项 /
widgets 不清 / icons 不清 / views 丢 app::frame 窄豁免。

每个变异从干净基准重新生成、跑 fixture 测试、try/finally 还原。
"""
import io
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ARCH = os.path.join(ROOT, "tools", "arch_check.rs")
BASE = os.path.join(ROOT, ".arch_check.r13base")
TARGET = "arch_violation_fixture"

MUTANTS = [
    (
        "方案点名：state 的 views 禁令清空",
        "            v.extend_from_slice(PRESENTATION_STATE_VIEWS_FORBIDDEN);\n",
        "",
        "state_may_not_depend_on_views",
    ),
    (
        "views→state 反向也禁（过度收紧）",
        "        Layer::PresentationViews => PRESENTATION_FORBIDDEN.to_vec(),",
        "        Layer::PresentationViews => {\n            let mut v = PRESENTATION_FORBIDDEN.to_vec();\n            v.extend_from_slice(PRESENTATION_STATE_VIEWS_FORBIDDEN);\n            v\n        },",
        "views_may_depend_on_state",
    ),
    (
        "rich 名单漏掉 theme",
        '    "crate::presentation::views",\n    "crate::presentation::state",\n    "crate::presentation::theme",\n    "crate::presentation::widgets",\n];',
        '    "crate::presentation::views",\n    "crate::presentation::state",\n    "crate::presentation::widgets",\n];',
        "rich_may_not_depend_on_its_siblings",
    ),
    (
        "widgets 不清（漏 PRESENTATION_WIDGETS_FORBIDDEN）",
        "            v.extend_from_slice(PRESENTATION_WIDGETS_FORBIDDEN);\n",
        "",
        "leaf_layers_may_not_depend_on_views",
    ),
    (
        "icons 不清（漏 PRESENTATION_ICONS_FORBIDDEN）",
        "            v.extend_from_slice(PRESENTATION_ICONS_FORBIDDEN);\n",
        "",
        "leaf_layers_may_not_depend_on_views",
    ),
    (
        "views 丢 app::frame 窄豁免（35 文件会同时红）",
        "        | Layer::PresentationViews\n",
        "        | Layer::PresentationState\n",
        "views_keep_the_narrow_app_frame_allowance",
    ),
    (
        "窄豁免外溢到整个 crate::app",
        '        | Layer::PresentationIcons => &["crate::app::frame"],',
        '        | Layer::PresentationIcons => &["crate::app"],',
        "the_narrow_allowance_does_not_leak_to_the_rest_of_app",
    ),
    (
        "layer_of 丢掉根文件形态（只认目录）",
        '            Some("state") => Layer::PresentationState,',
        '            Some("state") if parts.len() > 2 => Layer::PresentationState,',
        "the_layer_root_files_are_judged_too",
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
        ["cargo", "test", "--test", TARGET],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    out = r.stdout + r.stderr
    if "error[" in out:
        return "COMPILE_FAIL", out
    if "架构边界校验失败" in out:
        return "BUILD_GUARD", out
    if "test result:" not in out:
        return "NO_RESULT", out
    line = [l for l in out.splitlines() if l.startswith("test result:")][-1]
    ok = "0 failed" in line
    return ("PASS" if ok else "FAIL"), out


def main():
    base = read(ARCH).replace("\r\n", "\n")
    write(BASE, base)
    rows = []
    try:
        for name, old, new, expect in MUTANTS:
            write(ARCH, base)
            cur = read(ARCH)
            if old not in cur:
                rows.append((name, "ANCHOR_MISS", "锚点未命中：变异未生效"))
                continue
            write(ARCH, cur.replace(old, new, 1))
            status, out = run_tests()
            if status == "COMPILE_FAIL":
                rows.append((name, "COMPILE_FAIL", "编译失败——变异无效"))
            elif status == "BUILD_GUARD":
                rows.append((name, "captured_build", "build.rs 架构校验拦下"))
            elif status == "NO_RESULT":
                rows.append((name, "NO_RESULT", "无 test result 行"))
            else:
                caught = f"test {expect} ... FAILED" in out or f"    {expect}" in out
                rows.append((name, "captured" if caught else "NO_TEETH", status))
    finally:
        write(ARCH, base)
        if os.path.exists(BASE):
            os.remove(BASE)

    print(f"{'变异':<44} {'结果':<14}")
    print("-" * 74)
    teeth = 0
    for name, res, note in rows:
        print(f"{name:<44} {res:<14} {note}")
        if res in ("captured", "captured_build"):
            teeth += 1
    print("-" * 74)
    print(f"共 {len(rows)} 轮，捕获 {teeth}，无牙齿 {len(rows) - teeth}")
    print("（captured = 被测用例红；captured_build = build.rs 架构校验先拦下）")
    return 0 if teeth == len(rows) else 1


if __name__ == "__main__":
    sys.exit(main())
