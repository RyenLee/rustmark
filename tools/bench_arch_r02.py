#!/usr/bin/env python3
"""R-02 验收：构建期耗时前后对比（cargo build 的 Finished in，3 次取中位数）。

每次跑之前 `touch src/lib.rs` 强制重跑 build script（否则 cargo 判定
脚本无需重跑、build.rs 根本不执行——测到的就不是扫描耗时）。
"""
import os
import re
import shutil
import statistics
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FILES = ["tools/arch_check.rs", "build.rs", "src/bin/check-arch.rs", "tests/arch_violation_fixture.rs"]
BASE = os.path.join(ROOT, ".arch_check.r02base")


def run_build():
    lib = os.path.join(ROOT, "src", "lib.rs")
    os.utime(lib, None)
    r = subprocess.run(["cargo", "build"], cwd=ROOT, capture_output=True,
                       text=True, encoding="utf-8", errors="replace")
    out = r.stdout + r.stderr
    m = re.search(r"Finished `dev` profile[^\n]*?in ([\d.]+)s", out)
    if not m:
        print("未找到 Finished in：\n" + out[-2000:])
        sys.exit(1)
    return float(m.group(1))


def main():
    with open(BASE, "w", encoding="utf-8", newline="") as f:
        pass
    # 备份当前（含 R-02 改动）的四个文件
    backups = {}
    for rel in FILES:
        p = os.path.join(ROOT, rel)
        backups[rel] = open(p, "rb").read()

    try:
        # 先测「已纳入 tests」：当前工作区状态
        after = [run_build() for _ in range(3)]

        # 再测基线：git show HEAD 版本覆盖回去
        for rel in FILES:
            p = os.path.join(ROOT, rel)
            r = subprocess.run(["git", "show", f"HEAD:{rel}"], cwd=ROOT,
                               capture_output=True)
            if r.returncode != 0:
                print(f"git show HEAD:{rel} 失败：{r.stderr.decode('utf-8', 'replace')}")
                sys.exit(1)
            open(p, "wb").write(r.stdout)
        before = [run_build() for _ in range(3)]
    finally:
        for rel, data in backups.items():
            open(os.path.join(ROOT, rel), "wb").write(data)
        if os.path.exists(BASE):
            os.remove(BASE)

    print(f"纳入 tests/ 前：{before}  中位数 {statistics.median(before):.2f}s")
    print(f"纳入 tests/ 后：{after}  中位数 {statistics.median(after):.2f}s")
    delta = statistics.median(after) - statistics.median(before)
    print(f"差值：{delta:+.2f}s（{(delta / statistics.median(before) * 100):+.1f}%）")
    # 还原后自检：scan_with_tests 必须在
    with open(os.path.join(ROOT, "build.rs"), encoding="utf-8") as f:
        assert "scan_with_tests" in f.read(), "还原失败：build.rs 丢了 scan_with_tests"
    print("还原自检 OK")


if __name__ == "__main__":
    main()
