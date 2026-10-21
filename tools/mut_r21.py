#!/usr/bin/env python3
"""R-21 变异验证（item 8 判据加强）：破坏分隔线解析，确认用例当场变红。

用法：python tools/mut_r21.py

背景：`thematic_break_is_parsed` 原只断言 `!blocks.is_empty()`（「解析出块」
不等于「解析出分割线」——`---` 当普通段落、甚至被整段丢弃，旧断言全绿）。
R-21 加强为「块数 == 3 + 中间块是 `BlockKind::Rule` + 源码区间覆盖 `---`」。

变异点：
  1. `Event::Rule` 臂改永不匹配（`if false` 守卫）——分隔线被**丢弃**，
     块数从 3 掉到 2 → 块数断言红。
  2. `Rule` 块降级成 `BlockKind::Paragraph`——分隔线被**当普通段落**，
     块数仍是 3 但类型断言红（证明「类型」判据独立有牙齿）。

纪律（与 mut_r20.py 一致）：备份 → 改动 → 跑测试 → 还原（try/finally），
处理 CRLF，识别 `error[`≠无牙齿、锚点未命中跳过。检测串用
`f"{expect} ... FAILED"`（容忍任意模块前缀）。
"""
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "src" / "infrastructure" / "parsing" / "pulldown_parser" / "inline.rs"

# (名称, 锚点原文, 替换成, 期望变红的用例名子串, cargo test 参数)
MUTATIONS = [
    (
        "R21-8 hr 分支改永不匹配（分隔线被丢弃）",
        TARGET,
        "        Event::Rule => {",
        "        Event::Rule if false => {",
        ["thematic_break_is_parsed"],
        ["--test", "infrastructure"],
    ),
    (
        "R21-8 Rule 块降级成普通段落（分隔线被当段落）",
        TARGET,
        "                BlockKind::Rule,",
        "                BlockKind::Paragraph,",
        ["thematic_break_is_parsed"],
        ["--test", "infrastructure"],
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
        backup = target.with_name(target.name + ".r21.bak")
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
        print(f"R-21 变异验证失败 {len(failures)} 项：{failures}")
        return 1
    print(f"R-21 变异验证通过：{len(MUTATIONS)}/{len(MUTATIONS)} 个变异点都被用例当场抓住。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
