#!/usr/bin/env python3
"""变异验证：逐个破坏修复点，确认对应用例当场变红。

用法：python tools/mut_arch_check.py
每个变异：备份 → 改动 → 跑测试 → 从备份还原（try/finally）。
"""
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "tools" / "arch_check.rs"
BACKUP = ROOT / "tools" / ".arch_check.rs.bak"

# (名称, 锚点原文, 替换成, 期望变红的用例名子串列表)
MUTATIONS = [
    (
        "M1 豁免窗口退回「前一宽口径」",
        """                    allow_covers(&a.name, cand.banned)
                        && (cand.win_start <= a.line && a.line <= cand.win_end
                            // 独立成行的豁免额外罩住紧邻的下一行；行尾豁免不罩——
                            // 否则第 1 行的行尾豁免会把第 2 行的独立违规一并压掉。
                            || (a.own_line && cand.win_start == a.line + 1))""",
        """                    allow_covers(&a.name, cand.banned)
                        && a.line + 1 >= cand.win_start
                        && a.line <= cand.win_end""",
        ["a_trailing_allow_does_not_cover_the_next_line",
         "a_trailing_allow_does_not_cover_the_next_reference"],
    ),
    (
        "M2 未知层退回 Free（默认放行）",
        """        "main" | "lib" if parts.len() == 1 => Layer::Free,
        _ => Layer::Unknown,""",
        """        "main" | "lib" if parts.len() == 1 => Layer::Free,
        _ => Layer::Free,""",
        ["an_unknown_top_level_directory_is_reported",
         "an_unknown_top_level_file_is_reported",
         "an_unknown_file_still_obeys_the_repaint_rule"],
    ),
    (
        "M3 未知层检查插到帧预约循环之前",
        """    // ---- 定时任务的帧预约只允许一个出口（T11 / ADR-009）----""",
        """    if layer == Layer::Unknown {
        violations.push(Violation {
            file: rel.to_string(),
            line: 1,
            detail: "MUTANT-unknown-first".to_string(),
        });
        return violations;
    }
    // ---- 定时任务的帧预约只允许一个出口（T11 / ADR-009）----""",
        ["an_unknown_file_still_obeys_the_repaint_rule"],
    ),
    (
        "M4 豁免体检跳回「无层禁令提前返回」之后",
        """    let mut used_allow = vec![false; lexed.allows.len()];
    let forbidden = forbidden_for(layer);
    if !forbidden.is_empty() {""",
        """    let forbidden = forbidden_for(layer);
    if forbidden.is_empty() {
        return violations;
    }
    let mut used_allow = vec![false; lexed.allows.len()];
    if true {""",
        ["a_stale_allow_in_a_layer_without_bans_is_reported"],
    ),
    (
        "M5 删掉 extern crate 分支",
        """        if t.kind == TokKind::Ident
            && t.text == "extern"
            && tokens.get(i + 1).map(|n| n.text.as_str()) == Some("crate")
        {""",
        """        if false {""",
        ["extern_crate_is_detected", "aliased_extern_crate_is_detected"],
    ),
    (
        "M6 帧预约豁免文案退回通用版",
        """        let detail = if repaint_here {
            format!(
                "arch-allow 豁免无效：{}（该行确实有违规，但「{REPAINT_RESERVATION} 只允许出现在 \\
                 {REPAINT_RESERVATION_HOME}」这条规则不设豁免）",
                a.name
            )
        } else {
            format!(
                "arch-allow 豁免无效：{}（该行没有依赖它的违规，豁免要么写错、要么已过时）",
                a.name
            )
        };""",
        """        let _ = repaint_here;
        let detail = format!(
            "arch-allow 豁免无效：{}（该行没有依赖它的违规，豁免要么写错、要么已过时）",
            a.name
        );""",
        ["the_repaint_rule_has_no_exemption"],
    ),
    (
        "M7 豁免窗口过紧：只认行内，不认前一行",
        """                    allow_covers(&a.name, cand.banned)
                        && (cand.win_start <= a.line && a.line <= cand.win_end
                            // 独立成行的豁免额外罩住紧邻的下一行；行尾豁免不罩——
                            // 否则第 1 行的行尾豁免会把第 2 行的独立违规一并压掉。
                            || (a.own_line && cand.win_start == a.line + 1))""",
        """                    allow_covers(&a.name, cand.banned)
                        && cand.win_start <= a.line
                        && a.line <= cand.win_end""",
        ["an_own_line_allow_covers_the_statement_below",
         "an_own_line_allow_covers_a_multiline_use_below"],
    ),
    (
        "M8 豁免窗口向前无界：只留下界约束",
        """                    allow_covers(&a.name, cand.banned)
                        && (cand.win_start <= a.line && a.line <= cand.win_end
                            // 独立成行的豁免额外罩住紧邻的下一行；行尾豁免不罩——
                            // 否则第 1 行的行尾豁免会把第 2 行的独立违规一并压掉。
                            || (a.own_line && cand.win_start == a.line + 1))""",
        """                    allow_covers(&a.name, cand.banned)
                        && a.line <= cand.win_end""",
        ["an_allow_inside_a_use_group_does_not_leak_past_it",
         "an_own_line_allow_does_not_skip_a_line"],
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
            # 归一化为文件实际行尾再替换
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
                print(f"[WEAK] {name}: 破坏后测试仍全绿——用例没有牙齿！")
                failures.append(name)
                continue
            red = [t for t in expected_red if f"... {t} ... FAILED" in out or t in out.split("failures:")[-1]]
            # 逐条确认期望变红的用例确实出现在失败清单里
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
        print(f"变异验证失败 {len(failures)} 项：{failures}")
        return 1
    print(f"变异验证全部通过：{len(MUTATIONS)}/{len(MUTATIONS)} 个变异点都被用例当场抓住。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
