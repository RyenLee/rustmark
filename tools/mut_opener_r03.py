#!/usr/bin/env python3
"""R-03 用户可见层变异验证：把 opener 调用改坏，确认 5 条用例当场变红。

方案原文只要求「改回直连 → 编译失败」这一条（已由 mut_arch_r03.py 覆盖
禁令侧）。这里补**行为侧**：端口化后的调用逻辑本身也要有牙齿——
否则「连上了端口但参数错了 / 校验丢了」没人发现。

每个变异从干净基准重新生成、跑测试、try/finally 还原。
"""
import io
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
INLINE = os.path.join(ROOT, "src", "presentation", "views", "preview_view", "inline.rs")
BASE = os.path.join(ROOT, ".inline.r03base")
TARGET = "presentation"
FILTER = "external_opener"

# (名称, 旧串, 新串, 期望被捕获的测试名)
MUTANTS = [
    (
        "丢掉安全校验（is_safe_to_open 恒 true）",
        "            if system::is_safe_to_open(url) {",
        "            if true {",
        "an_unsafe_link_never_reaches_the_opener",
    ),
    (
        "校验反相（只放行危险地址）",
        "            if system::is_safe_to_open(url) {",
        "            if !system::is_safe_to_open(url) {",
        "clicking_a_safe_link_calls_the_opener_with_that_url",
    ),
    (
        "参数传错（把整段内容而非 url 交给 opener）",
        "                let _ = app.services.opener.open(url);",
        "                let _ = app.services.opener.open(content);",
        "clicking_a_safe_link_calls_the_opener_with_that_url",
    ),
    (
        "锚点分支不提前返回（内部跳转误落外部打开）",
        "                if let Some((_, id)) = internal_targets.iter().find(|(k, _)| k == anchor) {\n                    app.ui.sync.request_click(*id);\n                }\n                return;",
        "                if let Some((_, id)) = internal_targets.iter().find(|(k, _)| k == anchor) {\n                    app.ui.sync.request_click(*id);\n                }",
        "an_anchor_link_does_not_open_anything",
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
        ["cargo", "test", "--test", TARGET, FILTER],
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
    base = read(INLINE).replace("\r\n", "\n")
    write(BASE, base)
    rows = []
    try:
        for name, old, new, expect in MUTANTS:
            write(INLINE, base)
            cur = read(INLINE)
            if old not in cur:
                rows.append((name, "ANCHOR_MISS", "锚点未命中：变异未生效"))
                continue
            write(INLINE, cur.replace(old, new, 1))
            status, out = run_tests()
            if status == "COMPILE_FAIL":
                rows.append((name, "COMPILE_FAIL", "编译失败——变异无效"))
            elif status == "BUILD_GUARD":
                rows.append((name, "captured_build", "build.rs 架构校验拦下"))
            elif status == "NO_RESULT":
                rows.append((name, "NO_RESULT", "无 test result 行"))
            else:
                caught = f"test external_opener_test::{expect} ... FAILED" in out
                rows.append((name, "captured" if caught else "NO_TEETH", status))
    finally:
        write(INLINE, base)
        if os.path.exists(BASE):
            os.remove(BASE)

    print(f"{'变异':<40} {'结果':<14}")
    print("-" * 68)
    teeth = 0
    for name, res, note in rows:
        print(f"{name:<40} {res:<14} {note}")
        if res in ("captured", "captured_build"):
            teeth += 1
    print("-" * 68)
    print(f"共 {len(rows)} 轮，捕获 {teeth}，无牙齿 {len(rows) - teeth}")
    return 0 if teeth == len(rows) else 1


if __name__ == "__main__":
    sys.exit(main())
