#!/usr/bin/env python3
"""R-12 变异验证：破坏「表现层不得直写 app 状态字段」规则，确认用例当场变红。

用法：python tools/mut_r12.py
变异：把规则的判定条件改成 `if false`，使直写守卫失效 → 注入用例必须变红；
若变异后注入用例仍绿，说明守卫没有牙齿（NO_TEETH）。
备份 → 改动 → 跑测试 → 还原（try/finally），与 mut_arch_check.py 同一纪律。
"""
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "tools" / "arch_check.rs"
BACKUP = ROOT / "tools" / ".arch_check.r12.bak"

# (名称, 锚点原文, 替换成, 期望变红的用例名子串列表)
MUTATIONS = [
        (
        "R12 直写守卫失效（if false）",
        """    if is_presentation_layer(layer) {""",
        """    if false {""",
        [
            "presentation_direct_write_of_app_state_field_is_detected",
            "presentation_single_level_write_of_app_state_field_is_detected",
        ],
    ),
]


def run_tests() -> tuple[bool, str]:
    proc = subprocess.run(
        ["cargo", "test", "--test", "arch_violation_fixture"],
        cwd=ROOT, capture_output=True, text=True, timeout=900,
    )
    out = proc.stdout + proc.stderr
    return proc.returncode == 0, out


def main() -> int:
    shutil.copy2(TARGET, BACKUP)
    failures = []
    try:
        for name, anchor, replacement, expected_red in MUTATIONS:
            raw = BACKUP.read_bytes()
            if anchor.replace("\n", "\r\n").encode() not in raw and anchor.encode() not in raw:
                print(f"[SKIP] {name}: 锚点未命中（可能已被其它改动影响）")
                failures.append(name)
                continue
            crlf = b"\r\n" in raw
            a = anchor.replace("\n", "\r\n") if crlf else anchor
            r = replacement.replace("\n", "\r\n") if crlf else replacement
            mutated = raw.replace(a.encode(), r.encode(), 1)
            if mutated == raw:
                print(f"[SKIP] {name}: 替换无效果")
                failures.append(name)
                continue
            TARGET.write_bytes(mutated)
            try:
                ok, out = run_tests()
            finally:
                shutil.copy2(BACKUP, TARGET)
            if "error[" in out:
                print(f"[WEAK] {name}: 变异导致编译失败——不能算「有牙齿」")
                failures.append(name)
                continue
            if ok:
                print(f"[WEAK] {name}: 破坏后测试仍全绿——守卫没有牙齿！")
                failures.append(name)
                continue
            fail_section = out.split("failures:")[-1] if "failures:" in out else ""
            missing = [t for t in expected_red if t not in fail_section]
            if missing:
                print(f"[PART] {name}: 变红但与预期不符，未变红：{missing}")
                failures.append(name)
            else:
                print(f"[OK]   {name}: {len(expected_red)} 条预期用例全部变红")
    finally:
        shutil.copy2(BACKUP, TARGET)
        BACKUP.unlink(missing_ok=True)
    print()
    if failures:
        print(f"R-12 变异验证失败 {len(failures)} 项：{failures}")
        return 1
    print(f"R-12 变异验证通过：{len(MUTATIONS)}/{len(MUTATIONS)} 个变异点都被用例当场抓住。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
