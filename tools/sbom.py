#!/usr/bin/env python3
"""SBOM 生成（DEP-06）：从 `cargo metadata` 产 CycloneDX 1.5 JSON。

为什么自己写而不引 `cargo cyclonedx`：那条路要给发布流程装一个额外的
cargo 子命令（版本漂移、CI 装包失败即红），而 CycloneDX 需要的字段
（name / version / purl / licenses / 依赖边）`cargo metadata` 全都有。
本脚本零第三方依赖，与本仓库其它工具（`check_thresholds.py` /
`gen_release_notes.py`）同一风格。

口径：
- **范围** = 发布二进制的**实际依赖图**（从 package `rustmark` 沿 resolve
  可达的所有节点），不是整个 workspace 的 dev 依赖——SBOM 要描述的是
  「这个产物里有什么」，不是「开发时要编译什么」。
- **purl** 用 `pkg:cargo/<name>@<version>`；workspace 成员无 registry source，
  但仍给 purl（类型 cargo），以便按名字对齐。
- **licenses** 直接取包声明的 SPDX 表达式（`license` 字段）。缺失时写
  `NOASSERTION` 而不是猜——SBOM 猜许可比不写更危险。
- **确定性**：不写 `metadata.timestamp`（同一棵依赖树两次生成必须逐字节
  相同，`tools/release_dryrun.py` 会断言这一点），components 与 dependsOn
  全部排序。发布时如需时间戳，由 CI 在外层注入，不要改本脚本。

用法：
    python tools/sbom.py                       # → dist/sbom.cdx.json
    python tools/sbom.py --output /tmp/x.json
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections import deque
from pathlib import Path

SPEC_VERSION = "1.5"
ROOT_PACKAGE = "rustmark"


def load_metadata(cargo: str) -> dict:
    """`cargo metadata`（含 resolve，排除 dev-only 依赖靠可达性而不是 flag）。"""
    out = subprocess.run(
        [cargo, "metadata", "--format-version", "1", "--locked"],
        capture_output=True,
        text=True,
    )
    if out.returncode != 0:
        print(f"::error::cargo metadata 失败：{out.stderr.strip()}", file=sys.stderr)
        raise SystemExit(2)
    return json.loads(out.stdout)


def reachable_ids(meta: dict) -> list[str]:
    """从根包沿 resolve 的 deps 做 BFS，返回发布产物实际依赖的包 id。"""
    root = next(
        (p for p in meta["packages"] if p["name"] == ROOT_PACKAGE and p.get("source") is None),
        None,
    )
    if root is None:
        print(f"::error::依赖图里找不到根包 {ROOT_PACKAGE}", file=sys.stderr)
        raise SystemExit(2)

    nodes = {n["id"]: n for n in meta.get("resolve", {}).get("nodes", [])}
    seen: set[str] = set()
    order: list[str] = []
    queue = deque([root["id"]])
    while queue:
        current = queue.popleft()
        if current in seen:
            continue
        seen.add(current)
        order.append(current)
        for dep in nodes.get(current, {}).get("deps", []):
            queue.append(dep["pkg"])
    return order


def purl(name: str, version: str) -> str:
    """CycloneDX 的 cargo purl（名字里的 `_` 不转义，purl 规范允许）。"""
    return f"pkg:cargo/{name}@{version}"


def component_of(pkg: dict) -> dict:
    """单个包 → CycloneDX component。字段顺序稳定，便于 diff。"""
    ref = purl(pkg["name"], pkg["version"])
    item: dict = {
        "type": "library",
        "bom-ref": ref,
        "name": pkg["name"],
        "version": pkg["version"],
        "purl": ref,
        "scope": "required",
    }
    license_expr = pkg.get("license")
    if license_expr:
        item["licenses"] = [{"expression": license_expr}]
    else:
        # 不猜：缺失即 NOASSERTION，交给评审判断。
        item["licenses"] = [{"license": {"name": "NOASSERTION"}}]
    repo = pkg.get("repository")
    if repo:
        item["externalReferences"] = [{"type": "vcs", "url": repo}]
    return item


def build_bom(meta: dict) -> dict:
    root = next(p for p in meta["packages"] if p["name"] == ROOT_PACKAGE and p.get("source") is None)
    by_id = {p["id"]: p for p in meta["packages"]}
    ids = reachable_ids(meta)

    components = [component_of(by_id[i]) for i in ids if i != root["id"]]
    components.sort(key=lambda c: (c["name"], c["version"]))

    nodes = {n["id"]: n for n in meta.get("resolve", {}).get("nodes", [])}
    ref_of = lambda pid: purl(by_id[pid]["name"], by_id[pid]["version"])  # noqa: E731
    dependencies = []
    for pid in ids:
        deps = sorted({ref_of(d["pkg"]) for d in nodes.get(pid, {}).get("deps", []) if d["pkg"] in by_id})
        dependencies.append({"ref": ref_of(pid), "dependsOn": deps})
    dependencies.sort(key=lambda d: d["ref"])

    root_ref = purl(root["name"], root["version"])
    return {
        "bomFormat": "CycloneDX",
        "specVersion": SPEC_VERSION,
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "bom-ref": root_ref,
                "name": root["name"],
                "version": root["version"],
                "purl": root_ref,
                "licenses": [{"expression": root.get("license") or "NOASSERTION"}],
            }
        },
        "components": components,
        "dependencies": dependencies,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="生成 rustmark 的 CycloneDX SBOM")
    parser.add_argument("--output", default="dist/sbom.cdx.json", help="输出路径（默认 dist/sbom.cdx.json）")
    parser.add_argument("--cargo", default="cargo", help="cargo 可执行文件（默认 PATH 上的 cargo）")
    args = parser.parse_args()

    bom = build_bom(load_metadata(args.cargo))
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    # 缩进 + 固定换行：人可读、git 友好、生成结果可逐字节比对。
    out.write_text(json.dumps(bom, ensure_ascii=False, indent=2, sort_keys=False) + "\n", encoding="utf-8")
    print(f"SBOM: {out}（{len(bom['components'])} 个组件，spec {SPEC_VERSION}）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
