#!/usr/bin/env python3
"""R-02 变异验证：把守卫改坏，确认新增用例当场变红。

每个变异都从**干净基准**重新生成文件、跑测试、try/finally 还原。
编译失败单独识别——否则会被「无 test result 行」误判成「无牙齿」。
"""
import io
import os
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ARCH = os.path.join(ROOT, "tools", "arch_check.rs")
BASE = os.path.join(ROOT, ".arch_check.r02base")
TARGET = "arch_violation_fixture"

# (名称, 旧串, 新串, 期望被捕获的测试名)
MUTANTS = [
    (
        "tests/ 完全纳入扫描（TESTS_FORBIDDEN 清空）",
        '        Layer::Tests => TESTS_FORBIDDEN.to_vec(),',
        '        Layer::Tests => Vec::new(),',
        "injected_crate_layer_path_in_tests_is_detected",
    ),
    (
        "名单漏掉 crate::app",
        'const TESTS_FORBIDDEN: &[&str] = &[\n    "crate::app",',
        'const TESTS_FORBIDDEN: &[&str] = &[',
        "every_library_layer_name_is_forbidden_in_tests",
    ),
    (
        "scan_with_tests 不扫 tests（退化成 scan）",
        'pub fn scan_with_tests(src_root: &Path, tests_root: &Path) -> Vec<Violation> {\n    scan_impl(src_root, Some(tests_root))\n}',
        'pub fn scan_with_tests(src_root: &Path, tests_root: &Path) -> Vec<Violation> {\n    let _ = tests_root;\n    scan_impl(src_root, None)\n}',
        "injected_crate_layer_path_in_tests_is_detected",
    ),
    (
        "tests 侧文件不带 tests/ 前缀（与 src 侧混淆）",
        '            let rel = format!("tests/{rel}");',
        '            let rel = rel.to_string();',
        "injected_crate_layer_path_in_tests_is_detected",
    ),
    (
        "Tests 层文案不给正确写法",
        '                        "（集成测试是消费者不是层：引用库请写公开 API `rustmark::…`；\\\n',
        '                        "（集成测试是消费者不是层）\\\n',
        "injected_crate_layer_path_in_tests_is_detected",
    ),
    (
        "表现层的窄豁免前缀被扩到 Tests 层（crate::app::frame 放行）",
        '        Layer::Presentation | Layer::PresentationState => &["crate::app::frame"],',
        '        Layer::Presentation | Layer::PresentationState | Layer::Tests => &["crate::app::frame"],',
        "the_narrow_allow_prefix_does_not_apply_to_tests",
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
    if "test result:" not in out:
        return "NO_RESULT", out
    line = [l for l in out.splitlines() if l.startswith("test result:")][-1]
    ok = "0 failed" in line
    return ("PASS" if ok else "FAIL"), out


def main():
    # 干净基准（统一成 LF 再写，读时也读 LF）
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
            elif status == "NO_RESULT":
                rows.append((name, "NO_RESULT", "无 test result 行"))
            else:
                caught = f"test {expect} ... FAILED" in out or f"    {expect}" in out
                rows.append((name, "captured" if caught else "NO_TEETH", f"{status}"))
    finally:
        write(ARCH, base)
        if os.path.exists(BASE):
            os.remove(BASE)

    print(f"{'变异':<52} {'结果':<12}")
    print("-" * 70)
    teeth = 0
    for name, res, note in rows:
        print(f"{name:<52} {res:<12} {note}")
        if res == "captured":
            teeth += 1
    print("-" * 70)
    print(f"共 {len(rows)} 轮，捕获 {teeth}，无牙齿 {len(rows) - teeth}")
    return 0 if teeth == len(rows) else 1


if __name__ == "__main__":
    sys.exit(main())
