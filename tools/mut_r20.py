#!/usr/bin/env python3
"""R-20 变异验证：破坏字体装配的两条「顺序」约束，确认用例当场变红。

用法：python tools/mut_r20.py

变异点：
  1. assembly.rs 的 `prepend_selected_to_monospace` 把选中字体**置顶**（insert(0)）
     改**追加**（push）——用户选中的西文字体就看不见了（设置变死设置）。
  2. assembly.rs 的 `extend_with_cjk` 把 CJK 字体**追加到族末尾**改**插到开头**——
     西文与代码块会改用中文字体的拉丁字形、代码块失去等宽。

纪律（与 mut_r12.py / mut_r17.py 一致）：备份 → 改动 → 跑测试 → 还原
（try/finally），处理 CRLF，识别 `error[`≠无牙齿、try/finally 还原、锚点未命中
从干净基准重取。检测串用 `f"{expect} ... FAILED"`（容忍任意模块前缀）。
"""
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "src" / "infrastructure" / "fonts" / "assembly.rs"

# (名称, 锚点原文, 替换成, 期望变红的用例名子串, cargo test 参数)
MUTATIONS = [
    (
        "R20 选中字体置顶改追加 (insert(0)→push)",
        TARGET,
        "    mono.insert(0, sel.to_string());",
        "    mono.push(sel.to_string());",
        ["selected_font_is_prepended_without_duplicates"],
        ["--lib", "infrastructure::fonts"],
    ),
    (
        "R20 CJK 追加到末尾改插到开头 (push→insert(0))",
        TARGET,
        "            .push(CJK_KEY.to_owned());",
        "            .insert(0, CJK_KEY.to_owned());",
        ["cjk_is_appended_not_prepended"],
        ["--lib", "infrastructure::fonts"],
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
        backup = target.with_name(target.name + ".r20.bak")
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
        print(f"R-20 变异验证失败 {len(failures)} 项：{failures}")
        return 1
    print(f"R-20 变异验证通过：{len(MUTATIONS)}/{len(MUTATIONS)} 个变异点都被用例当场抓住。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
