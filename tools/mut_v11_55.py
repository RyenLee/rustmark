#!/usr/bin/env python3
"""V11-55 变异验证：破坏「对话框过滤器派生自白名单」的两条约束，确认用例当场变红。

用法：python tools/mut_v11_55.py

背景：shared 内部环（text_format ↔ dialog）破除后，`file_filter()` 的实现
从 text_format.rs 迁入 dialog.rs（V11-55 方案 A）。迁移不能只搬代码不搬
牙齿——两条判据必须仍然有效：
  1. `file_filter` 仍**派生自**白名单（而非另写一份扩展名清单）；
  2. `default_filters` 仍走 `file_filter`（而非在 dialog 侧另立清单）。

变异点：
  1. `default_filters` 改成硬编码清单（绕过 file_filter）——问题 045 的
     「唯一出处」被破坏，拖放/守卫/对话框三者开始漂移。
  2. `file_filter` 的扩展名改成硬编码 `&["md"]`（切断与白名单的派生）——
     新增格式不再进对话框，「唯一出处」名存实亡。

纪律（与 mut_r20.py / mut_r21.py 一致）：备份 → 改动 → 跑测试 → 还原
（try/finally），处理 CRLF，识别 `error[`≠无牙齿，锚点未命中从干净基准
重取。检测串用 `f"{expect} ... FAILED"`（容忍任意模块前缀——lib 测试
输出形如 `test shared::dialog::tests::... ... FAILED`）。
"""
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "src" / "shared" / "dialog.rs"

# (名称, 锚点原文, 替换成, 期望变红的用例名, cargo test 参数)
MUTATIONS = [
    (
        "V11-55 default_filters 绕过派生改硬编码清单",
        TARGET,
        "    vec![TextFormats::builtin().file_filter()]",
        '    vec![FileFilter::new("Markdown", &["md", "markdown"])]',
        ["default_filters_derives_from_the_whitelist"],
        ["--lib", "shared::dialog"],
    ),
    (
        "V11-55 file_filter 切断派生改硬编码扩展名",
        TARGET,
        """        FileFilter::new(
            FILTER_NAME,
            &self
                .extensions()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )""",
        """        FileFilter::new(
            FILTER_NAME,
            &["md"],
        )""",
        ["file_filter_is_derived_from_the_whitelist"],
        ["--lib", "shared::dialog"],
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
        backup = target.with_name(target.name + ".v55.bak")
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
        print(f"V11-55 变异验证失败 {len(failures)} 项：{failures}")
        return 1
    print(f"V11-55 变异验证通过：{len(MUTATIONS)}/{len(MUTATIONS)} 个变异点都被用例当场抓住。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
