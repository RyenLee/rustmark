#!/usr/bin/env python3
"""R-03 变异验证：把实现搬回 shared / 削弱禁令，确认新用例当场变红。

方案点名的变异：把 preview_view 的调用改回直连 shared::system::open_external
（即实现留在 shared）→ 新禁令必须命中。

每个变异都从**干净基准**重新生成文件、跑 fixture 测试、try/finally 还原。
编译失败单独识别——否则会被「无 test result 行」误判成「无牙齿」。
"""
import io
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ARCH = os.path.join(ROOT, "tools", "arch_check.rs")
BASE = os.path.join(ROOT, ".arch_check.r03base")
TARGET = "arch_violation_fixture"

# (名称, 旧串, 新串, 期望被捕获的测试名)
MUTANTS = [
    (
        "方案点名：shared 留 open_external 薄封装（改回直连）",
        '    "pulldown_cmark",\n    "std::process",',
        '    "pulldown_cmark",',
        "a_free_function_wrapper_in_shared_is_still_banned",
    ),
    (
        "SHARED_FORBIDDEN 整个丢掉 std::process（禁令失效）",
        '    "std::process",\n',
        '',
        "std_process_is_forbidden_in_shared",
    ),
    (
        "禁令误扩到 infrastructure（拖累执行端归宿）",
        '        Layer::Infrastructure => INFRASTRUCTURE_FORBIDDEN.to_vec(),',
        '        Layer::Infrastructure => {\n            let mut v = INFRASTRUCTURE_FORBIDDEN.to_vec();\n            v.extend_from_slice(SHARED_FORBIDDEN);\n            v\n        },',
        "std_process_is_allowed_in_infrastructure",
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
    # 架构校验失败会让 build.rs panic（"架构边界校验失败"），
    # 这也是「守卫在工作」的证明——但它不是被测用例红的，
    # 必须单独归类，不能算「无牙齿」也不能算「捕获」。
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
                # 守卫在 build.rs 就拦住了：变异确实「造成了违规」，
                # 但被测用例没机会跑。单独归类为 captured_build。
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

    print(f"{'变异':<48} {'结果':<14}")
    print("-" * 76)
    teeth = 0
    for name, res, note in rows:
        print(f"{name:<48} {res:<14} {note}")
        if res in ("captured", "captured_build"):
            teeth += 1
    print("-" * 76)
    print(f"共 {len(rows)} 轮，捕获 {teeth}，无牙齿 {len(rows) - teeth}")
    print("（captured = 被测用例红；captured_build = build.rs 架构校验先拦下，")
    print("  同为「守卫有效」，但被测用例没机会跑，故分开记）")
    return 0 if teeth == len(rows) else 1


if __name__ == "__main__":
    sys.exit(main())
