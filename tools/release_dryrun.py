#!/usr/bin/env python3
"""离线预发布演练（S9-06 的可离线部分）。

完整的预发布演练需要 fork 仓库、CI 与三平台真机（docs/release.md §4），
当前环境无远端、无 git 历史、仅 Windows，无法闭环。但发布流程中**不依赖
远端**的环节可以先离线验证，从而把「待真机」收敛到最小且每条都有归属：

1. tag ↔ Cargo.toml 版本守卫：绿路径 + 红路径（错误 tag 必须被拦截）
2. SHA256SUMS 汇总与校验：对 `dist/` 产物重算 sha256（等价 `sha256sum -c`，
   用 hashlib 以保证 Windows 无 sha256sum 也可用），并用篡改一字节的副本
   验证校验确实会失败
3. Release Notes 阶段归组：构造 subject 列表验证分组与排序（含两位数阶段号）
4. 无私钥 / 证书入仓：扫描工作区（排除 target / dist）无证书与私钥，
   `.gitignore` 已排除相关扩展名
5. 回滚步骤：docs/release.md 三条命令齐备，且与 release.yml 的 tag 触发一致

用法：
    python tools/release_dryrun.py [--tag v0.1.0] [--dist dist]
退出码：0 全部通过；1 存在失败项（CI 会红）。
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import os
import re
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TOOLS = ROOT / "tools"

CARGO_VERSION_RE = re.compile(r'^version\s*=\s*"([^"]+)"', re.MULTILINE)
PRIVATE_KEY_RE = re.compile(rb"BEGIN [A-Z0-9 ]*PRIVATE KEY")
SECRET_SUFFIXES = {".p12", ".pfx", ".cer", ".key"}
PRUNE_DIRS = {"target", "dist", ".git", "node_modules", ".workbuddy", "cov-fresh"}

PASS = "PASS"
FAIL = "FAIL"
WARN = "WARN"


class Report:
    """演练结果收集器。"""

    def __init__(self) -> None:
        self.rows: list[tuple[str, str, str]] = []

    def add(self, status: str, name: str, detail: str) -> None:
        self.rows.append((status, name, detail))
        print(f"[{status}] {name}：{detail}")

    def ok(self, name: str, detail: str) -> None:
        self.add(PASS, name, detail)

    def bad(self, name: str, detail: str) -> None:
        self.add(FAIL, name, detail)

    def warn(self, name: str, detail: str) -> None:
        self.add(WARN, name, detail)

    @property
    def failures(self) -> int:
        return sum(1 for s, _, _ in self.rows if s == FAIL)

    def summary(self) -> str:
        total = len(self.rows)
        return f"演练项 {total - self.failures}/{total} 通过（失败 {self.failures}）"


def cargo_version(path: Path) -> str | None:
    """取 Cargo.toml 第一个 `version = "..."`（[package] 段），与 CI 的 sed 取法一致。"""
    m = CARGO_VERSION_RE.search(path.read_text(encoding="utf-8"))
    return m.group(1) if m else None


def guard_passes(tag: str, version: str) -> bool:
    """tag 去掉前导 v 后须与 Cargo.toml version 完全相等。"""
    return tag.lstrip("v") == version


def check_version_guard(rep: Report, tag: str) -> None:
    version = cargo_version(ROOT / "Cargo.toml")
    if version is None:
        rep.bad("版本守卫", "Cargo.toml 未解析到 version")
        return

    if guard_passes(tag, version):
        rep.ok("版本守卫（绿路径）", f"tag {tag} ↔ Cargo.toml {version} 一致")
    else:
        rep.bad("版本守卫（绿路径）", f"tag {tag} 与 Cargo.toml {version} 不一致")

    # 红路径：错误 tag 必须被拦截，否则守卫形同虚设。
    wrong = "v9.9.9"
    if guard_passes(wrong, version):
        rep.bad("版本守卫（红路径）", f"错误 tag {wrong} 未被拦截，守卫失效")
    else:
        rep.ok("版本守卫（红路径）", f"错误 tag {wrong} 被拦截（版本 {version}）")

    yml = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
    needed = ["Verify tag matches Cargo.toml version", "GITHUB_REF_NAME#v", "exit 1"]
    missing = [n for n in needed if n not in yml]
    if missing:
        rep.bad("版本守卫（CI 同源）", f"release.yml 缺少 {missing}")
    else:
        rep.ok("版本守卫（CI 同源）", "release.yml 守卫步骤与本地判定同源")


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def check_checksums(rep: Report, dist: Path) -> None:
    """汇总 SHA256SUMS 并逐条重算校验；再用篡改副本验证校验会失败。"""
    if not dist.is_dir():
        rep.warn("校验和", f"{dist} 不存在，跳过（先跑 scripts/package-* 生成产物）")
        return

    sidecars = sorted(dist.glob("*.sha256"))
    if not sidecars:
        rep.warn("校验和", f"{dist} 下无 *.sha256，跳过")
        return

    pairs: list[tuple[str, str]] = []
    for sc in sidecars:
        for line in sc.read_text(encoding="utf-8").splitlines():
            parts = line.split()
            if len(parts) == 2:
                pairs.append((parts[0], parts[1]))
    pairs.sort(key=lambda p: p[1])  # 等价 CI 的 `sort -k2`

    if not pairs:
        rep.bad("校验和", "*.sha256 内容为空或格式不符（应为 `<hex>  <文件名>`）")
        return

    missing = [n for _, n in pairs if not (dist / n).is_file()]
    if missing:
        rep.bad("校验和（汇总）", f"SHA256SUMS 引用的产物缺失：{missing}")
        return
    rep.ok("校验和（汇总）", f"{len(pairs)} 个产物已汇总（等价 `sha256sum -c` 输入）")

    mismatched = [n for want, n in pairs if sha256_of(dist / n) != want]
    if mismatched:
        rep.bad("校验和（校验）", f"重算不一致：{mismatched}")
    else:
        rep.ok("校验和（校验）", f"{len(pairs)} 个产物 sha256 重算全部一致")

    # 红路径：篡改一字节必须被校验发现。用临时副本，绝不改动 dist 内真实产物。
    src = dist / pairs[0][1]
    with tempfile.TemporaryDirectory() as td:
        bad = Path(td) / src.name
        shutil.copyfile(src, bad)
        with bad.open("r+b") as fh:
            fh.seek(-1, os.SEEK_END)
            last = fh.read(1)
            fh.seek(-1, os.SEEK_END)
            fh.write(bytes([last[0] ^ 0xFF]))
        if sha256_of(bad) == pairs[0][0]:
            rep.bad("校验和（红路径）", "篡改一字节后哈希仍相同，校验无效")
        else:
            rep.ok("校验和（红路径）", f"篡改 {src.name} 末字节后哈希变化，可被校验发现")


def load_notes_module():
    spec = importlib.util.spec_from_file_location("gen_release_notes", TOOLS / "gen_release_notes.py")
    if spec is None or spec.loader is None:
        return None
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def check_release_notes(rep: Report, tag: str) -> None:
    mod = load_notes_module()
    if mod is None:
        rep.bad("Release Notes 归组", "无法加载 tools/gen_release_notes.py")
        return

    subjects = [
        "feat: 需求基线与技术决策 [S1-01]",
        "feat: 工程骨架与分层边界 [S2-03]",
        "fix: 长图拼接越界 [S9-04]",
        "feat: 崩溃日志与保留策略 [S9-08]",
        "chore: 阶段十预备 [S10-01]",
        "docs: 补一条无标记提交",
    ]
    groups = mod.group_subjects(subjects)
    order = [name for name, _ in mod.sort_groups(groups)]

    # 阶段 1 / 2 / 9 / 10 + 其他；同阶段的 S9-04 与 S9-08 必须合并为一组。
    expected_groups = 5
    if len(groups) != expected_groups:
        rep.bad("Release Notes 归组", f"分组数 {len(groups)} != {expected_groups}：{order}")
        return

    phase9 = [n for n in order if n.startswith("阶段 9")]
    if len(phase9) != 1 or len(groups[phase9[0]]) != 2:
        rep.bad("Release Notes 归组", f"同阶段任务未聚合：{order}")
        return
    if "S9-04~S9-08" not in phase9[0]:
        rep.bad("Release Notes 归组", f"阶段 9 组名未标注编号区间：{phase9[0]}")
        return
    rep.ok("Release Notes 归组", f"同阶段 S9-04/S9-08 合并为 {phase9[0]}，共 {len(groups)} 组")

    # 两位数阶段号必须排在个位阶段之后（原字符串排序会出错，见脚本 docstring）。
    idx9 = next(i for i, n in enumerate(order) if n.startswith("阶段 9"))
    idx10 = next(i for i, n in enumerate(order) if n.startswith("阶段 10"))
    if idx10 < idx9:
        rep.bad("Release Notes 排序", f"阶段 10 排在阶段 9 之前：{order}")
    elif order[-1] != "其他":
        rep.bad("Release Notes 排序", f"「其他」未排最后：{order}")
    else:
        rep.ok("Release Notes 归组与排序", f"{len(groups)} 组，顺序 {order}")

    rendered = mod.render(tag, groups)
    if f"## rustmark {tag}" in rendered and "### 阶段 10" in rendered and "sha256sum -c" in rendered:
        rep.ok("Release Notes 渲染", f"标题、阶段 10 分组与校验和提示齐备（tag {tag}）")
    else:
        rep.bad("Release Notes 渲染", "渲染输出缺少标题 / 阶段 10 分组 / 校验和提示")


def check_no_secrets(rep: Report) -> None:
    hits: list[str] = []
    for dirpath, dirnames, filenames in os.walk(ROOT):
        dirnames[:] = [d for d in dirnames if d not in PRUNE_DIRS]
        for fn in filenames:
            p = Path(dirpath) / fn
            rel = p.relative_to(ROOT).as_posix()
            if p.suffix.lower() in SECRET_SUFFIXES:
                hits.append(f"{rel}（证书/私钥扩展名）")
                continue
            try:
                if p.stat().st_size > 2 << 20:
                    continue
                if PRIVATE_KEY_RE.search(p.read_bytes()):
                    hits.append(f"{rel}（含 PRIVATE KEY 文本）")
            except OSError:
                continue

    if hits:
        rep.bad("无私钥入仓", f"命中 {hits[:5]}")
    else:
        rep.ok("无私钥入仓", "工作区（排除 target/dist）无证书与私钥文件")

    gi = ROOT / ".gitignore"
    text = gi.read_text(encoding="utf-8") if gi.is_file() else ""
    missing = [s for s in ("*.p12", "*.pfx", "*.cer", "*.key") if s not in text]
    if missing:
        rep.bad("私钥排除规则", f".gitignore 缺少 {missing}")
    else:
        rep.ok("私钥排除规则", ".gitignore 已排除 *.p12 / *.pfx / *.cer / *.key")


def check_rollback_docs(rep: Report) -> None:
    doc = ROOT / "docs" / "release.md"
    if not doc.is_file():
        rep.bad("回滚步骤", "docs/release.md 缺失")
        return
    text = doc.read_text(encoding="utf-8")
    needed = {
        "gh release delete": "删除 Release",
        "git push origin :refs/tags/": "撤回远端 tag",
        "git tag -d": "本地删 tag",
        "同名 tag 重打": "禁止同名重打的警示",
    }
    missing = [k for k in needed if k not in text]
    if missing:
        rep.bad("回滚步骤", f"docs/release.md 缺少 {[needed[m] for m in missing]}")
    else:
        rep.ok("回滚步骤", "删除 Release / 撤回远端 tag / 本地删 tag 三步齐备且含重打警示")

    yml = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
    if 'tags:' in yml and '"v*"' in yml:
        rep.ok("回滚与触发一致", "release.yml 由 v* tag 触发，与回滚撤回的 tag 同名")
    else:
        rep.bad("回滚与触发一致", "release.yml 未找到 v* tag 触发配置")


def main() -> int:
    parser = argparse.ArgumentParser(description="rustmark 离线预发布演练（S9-06）")
    parser.add_argument("--tag", default=None, help="待发布 tag，默认取 Cargo.toml 版本加 v 前缀")
    parser.add_argument("--dist", default="dist", help="产物目录，默认 dist")
    args = parser.parse_args()

    tag = args.tag
    if tag is None:
        v = cargo_version(ROOT / "Cargo.toml")
        if v is None:
            print("::error::无法从 Cargo.toml 推断版本，请显式传 --tag", file=sys.stderr)
            return 2
        tag = f"v{v}"

    print(f"离线预发布演练 · tag={tag} · dist={args.dist}")
    print("-" * 60)
    rep = Report()
    check_version_guard(rep, tag)
    check_checksums(rep, ROOT / args.dist)
    check_release_notes(rep, tag)
    check_no_secrets(rep)
    check_rollback_docs(rep)
    print("-" * 60)
    print(rep.summary())
    return 1 if rep.failures else 0


if __name__ == "__main__":
    sys.exit(main())
