#!/usr/bin/env python3
"""Release Notes 生成（S9-06，阶段归组）。

按 commit subject 里的 `[S{阶段}-{编号}]` 标记**以阶段为单位**聚合，产出 markdown。
同一阶段的多个任务归入一组，组名标注该阶段覆盖的编号区间（如 `阶段 9（S9-01~S9-08）`）；
`[S9-06]` 这类标记缺失的提交归入「其他」。
「生成」与「上传」分离：本脚本只产文本，发布由 `gh release create --notes-file` 完成。

两种输入模式：

- CI 模式（默认）：给定 `--tag`，用 git 取 `前一个 tag..当前 tag` 的 subject 列表。
- 离线模式：`--log-file`，从文件逐行读取 subject（每行一条）。用于无远端、
  无 git 历史环境下的演练（`tools/release_dryrun.py` 调用），也便于手工补写 Notes。

用法：
    python tools/gen_release_notes.py --tag v0.1.0            # CI / 有仓库时
    python tools/gen_release_notes.py --tag v0.1.0 --log-file /tmp/subjects.txt

排序说明：组名按阶段号**数值**升序，「其他」恒排最后。原内联实现用字符串
排序，`阶段 10` 会排在 `阶段 2` 之前——阶段数到两位数即出错，此处已修正。
"""

from __future__ import annotations

import argparse
import collections
import re
import subprocess
import sys

PHASE_TAG = re.compile(r"\[S(\d+)-(\d+)\]")
OTHER_GROUP = "其他"


def group_subjects(subjects: list[str]) -> dict[str, list[str]]:
    """按 `[S{阶段}-{编号}]` 的**阶段号**聚合，返回 {组名: [subject, ...]}。

    组名标注该阶段出现的编号区间：单编号 `阶段 9（S9-04）`，多编号
    `阶段 9（S9-01~S9-08）`。同阶段多任务必须合并为一组——若把编号写进
    组名，一个阶段的 8 条提交会碎成 8 个分组，与 S9-06「按阶段归组」的
    验收要求相悖（该缺陷由离线演练发现，见 docs/release.md §4）。
    """
    phases: dict[int, list[str]] = collections.defaultdict(list)
    numbers: dict[int, set[str]] = collections.defaultdict(set)
    others: list[str] = []

    for subject in subjects:
        if not subject.strip():
            continue
        m = PHASE_TAG.search(subject)
        if not m:
            others.append(subject)
            continue
        phase = int(m.group(1))
        phases[phase].append(subject)
        numbers[phase].add(m.group(2))

    groups: dict[str, list[str]] = {}
    for phase in sorted(phases):
        nums = sorted(numbers[phase], key=int)
        span = (
            f"S{phase}-{nums[0]}~S{phase}-{nums[-1]}"
            if len(nums) > 1
            else f"S{phase}-{nums[0]}"
        )
        groups[f"阶段 {phase}（{span}）"] = phases[phase]
    if others:
        groups[OTHER_GROUP] = others
    return groups


def sort_groups(groups: dict[str, list[str]]) -> list[tuple[str, list[str]]]:
    """阶段号数值升序；无阶段标记与不可解析的组排最后（组名二级排序稳定）。"""

    def key(item: tuple[str, list[str]]) -> tuple[int, int, str]:
        name = item[0]
        if name == OTHER_GROUP:
            return (1, 0, name)
        m = re.search(r"阶段 (\d+)", name)
        if m:
            return (0, int(m.group(1)), name)
        return (1, 0, name)

    return sorted(groups.items(), key=key)


def render(tag: str, groups: dict[str, list[str]]) -> str:
    """渲染 Release Notes。

    开头固定为**三段式**（SEC-03，不可删改）：

    1. 未签名——为什么会被拦、系统提示长什么样；
    2. 绕过方式——三平台各自的操作；
    3. 校验和——先核对再运行的命令。

    这三段是**发布页的合规义务**，不是文案润色：未签名的产物必须让用户在
    「被系统拦下」的那一刻能就地找到解释与操作，而不是再去翻 README。
    `tools/release_dryrun.py` 会断言三个小标题都在，删掉任何一段演练即红。
    """
    lines = [
        f"## rustmark {tag}",
        "",
        "### 一、未签名（请先读）",
        "",
        "本版本**未签名**，系统会拦一次：Windows 弹 SmartScreen「已保护你的电脑」；",
        "macOS 提示「无法打开，因为无法验证开发者」。这不是文件损坏，是未购买代码",
        "签名证书的必然结果（评估见 `docs/release.md` §6）。",
        "",
        "### 二、绕过方式",
        "",
        "- **Windows**：「更多信息」→「仍要运行」。",
        "- **macOS**：右键（或 Control + 点击）应用 →「打开」→ 再点「打开」；",
        "  或执行 `xattr -dr com.apple.quarantine rustmark.app`。",
        "- **Linux**：AppImage 首次运行前 `chmod +x rustmark-*.AppImage`。",
        "",
        "### 三、校验和（建议先核对再运行）",
        "",
        "```bash",
        "sha256sum -c SHA256SUMS",
        "```",
        "",
        "`SHA256SUMS` 随 Release 附件提供；单文件另附 `<name>.sha256`。",
        "",
        "### 变更内容",
        "",
    ]
    for name, items in sort_groups(groups):
        lines.append(f"### {name}")
        lines.extend(f"- {s}" for s in items)
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def subjects_from_git(tag: str) -> list[str]:
    """取 `前一个 tag..当前 tag` 的 subject 列表；无前一个 tag 时取该 tag 的单条历史。"""
    prev = subprocess.run(
        ["git", "describe", "--tags", "--abbrev=0", f"{tag}^"],
        capture_output=True,
        text=True,
    ).stdout.strip()
    rng = f"{prev}..{tag}" if prev else tag
    out = subprocess.run(
        ["git", "log", "--pretty=%s", rng],
        capture_output=True,
        text=True,
    )
    if out.returncode != 0:
        print(f"::error::git log 失败：{out.stderr.strip()}", file=sys.stderr)
        return []
    return out.stdout.splitlines()


def main() -> int:
    parser = argparse.ArgumentParser(description="rustmark Release Notes 生成（阶段归组）")
    parser.add_argument("--tag", required=True, help="发布 tag，形如 v0.1.0")
    parser.add_argument(
        "--log-file",
        type=str,
        default=None,
        help="离线模式：从文件逐行读取 commit subject（跳过空行）",
    )
    args = parser.parse_args()

    if args.log_file:
        with open(args.log_file, encoding="utf-8") as fh:
            subjects = [ln.rstrip("\n") for ln in fh if ln.strip()]
    else:
        subjects = subjects_from_git(args.tag)

    print(render(args.tag, group_subjects(subjects)), end="")
    return 0


if __name__ == "__main__":
    sys.exit(main())
