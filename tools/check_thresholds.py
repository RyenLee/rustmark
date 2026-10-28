#!/usr/bin/env python3
"""阈值基线校验（开发期门禁，2026-10-25）。

`docs/spec/thresholds.baseline.yaml` 是**开发期基线**：它不参与程序运行
（文件头 `runtime_consumed: false`），但要求与源码逐条一致。本脚本把
「改了常量没改基线」「基线行号漂移」「数值被悄悄调走」变成可执行检查。

设计取舍：
- **纯标准库**：不引入 PyYAML（CI 与本地零依赖，和 coverage_gate 同风格）。
  只解析本仓库生成器产出的固定子集（modules → thresholds → 字段）。
- **锚点定位**：每条带 `anchor`（源码行里必须出现的片段）。行号漂移可被发现，
  `--fix` 只回写行号，不改值——值的变化必须是有意修改。
- **别名解析**：消费「单一真源」的条目（如 `PREVIEW_DEBOUNCE_MS` 引用
  `shared::constants::DEBOUNCE_MS`）用 `via` 指向定义文件，数值在定义处校验。

退出码：0 = 全部通过（行号漂移默认 WARN）；1 = 数值不符 / 锚点丢失 / 歧义。
`--strict-lines` 把行号漂移升级为失败；`--fix` 回写漂移的行号。
"""

from __future__ import annotations

import argparse
import ast
import json
import re
import sys
from pathlib import Path

NUM_EXPR = re.compile(r"(?<![\w.])(\d[\d_]*(?:\.\d+)?(?:\s*[*/+\-]\s*\d[\d_]*(?:\.\d+)?)*)")


def parse_scalar(raw: str):
    """解析 YAML 标量：双引号串按 JSON 解码；数字转 int/float；其余原样。"""
    raw = raw.strip()
    if raw.startswith('"') and raw.endswith('"'):
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            return raw[1:-1]
    if raw == "false":
        return False
    if raw == "true":
        return True
    if raw in ("null", "~"):
        return None
    try:
        return int(raw)
    except ValueError:
        pass
    try:
        return float(raw)
    except ValueError:
        return raw


def parse_baseline(path: Path):
    """解析生成器产出的 YAML 子集，返回 (meta, entries)。

    entries: [{module, key, value, source, anchor, via?}]
    """
    meta: dict[str, object] = {}
    entries: list[dict[str, object]] = []
    module = None
    key = None
    current: dict[str, object] | None = None
    in_modules = False

    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.rstrip()
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        text = line.strip()

        if indent == 0:
            in_modules = text == "modules:"
            if ":" in text and not in_modules:
                name, value = text.split(":", 1)
                meta[name.strip()] = parse_scalar(value)
            continue
        if not in_modules:
            continue
        if indent == 2 and text.endswith(":"):
            module = text[:-1]
            continue
        if indent == 4:
            continue
        if indent == 6 and text.endswith(":"):
            if key and current is not None:
                entries.append({"module": module, "key": key, **current})
            key = text[:-1]
            current = {}
            continue
        if indent == 8 and current is not None and ":" in text:
            field, value = text.split(":", 1)
            current[field.strip()] = parse_scalar(value)

    if key and current is not None:
        entries.append({"module": module, "key": key, **current})
    return meta, entries


def eval_number(expr: str):
    """只允许数字与 + - * / 的表达式求值。"""
    expr = expr.replace("_", "")
    try:
        node = ast.parse(expr, mode="eval").body
    except SyntaxError:
        return None

    def walk(n):
        if isinstance(n, ast.Constant) and isinstance(n.value, (int, float)):
            return float(n.value)
        if isinstance(n, ast.BinOp):
            left, right = walk(n.left), walk(n.right)
            if isinstance(n.op, ast.Add):
                return left + right
            if isinstance(n.op, ast.Sub):
                return left - right
            if isinstance(n.op, ast.Mult):
                return left * right
            if isinstance(n.op, ast.Div) and right != 0:
                return left / right
        raise ValueError("unsupported expression")

    try:
        return walk(node)
    except (ValueError, TypeError):
        return None


ENUM_VALUES = {
    "TabSize::Two": 2.0,
    "TabSize::Four": 4.0,
    "TabSize::Eight": 8.0,
}


def numbers_in(line: str) -> list[float]:
    """提取一行里的数值：算术表达式 + 少数以枚举承载的阈值。"""
    out = []
    for match in NUM_EXPR.finditer(line):
        value = eval_number(match.group(1))
        if value is not None:
            out.append(value)
    for token, value in ENUM_VALUES.items():
        if token in line:
            out.append(value)
    return out


def same_number(a, b) -> bool:
    return abs(float(a) - float(b)) <= max(1e-9, abs(float(b)) * 1e-9)


def find_anchor(lines: list[str], anchor: str) -> list[int]:
    return [i for i, line in enumerate(lines) if anchor in line]


def split_source(source: str):
    path, _, line = source.rpartition(":")
    return path, int(line)


def check(root: Path, entries, strict_lines: bool):
    errors: list[str] = []
    drifts: list[tuple[str, int, int]] = []
    fixed_candidates: dict[str, int] = {}
    checked = skipped = 0

    for entry in entries:
        key = str(entry["key"])
        source = str(entry.get("source", ""))
        anchor = str(entry.get("anchor", ""))
        value = entry.get("value")
        if not source or not anchor:
            errors.append(f"{key}: 缺少 source 或 anchor")
            continue

        rel, recorded = split_source(source)
        path = root / rel
        if not path.exists():
            errors.append(f"{key}: 源文件不存在 {rel}")
            continue
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()

        index = recorded - 1
        if not (0 <= index < len(lines)):
            errors.append(f"{key}: 行号越界 {source}")
            continue

        if anchor not in lines[index]:
            hits = find_anchor(lines, anchor)
            if len(hits) > 1:
                # 同值多处（如两个 30 分钟超时）：按记录行号就近消歧，
                # 距离超过 40 行说明基线已经不可信，直接报错。
                chosen = min(hits, key=lambda i: abs(i - index))
                if abs(chosen - index) > 40:
                    where = ", ".join(str(i + 1) for i in hits[:5])
                    errors.append(f"{key}: 锚点不唯一且距记录行过远（{rel}: {where}）")
                    continue
                hits = [chosen]
            if len(hits) == 1:
                new_line = hits[0] + 1
                if new_line != recorded:
                    drifts.append((key, recorded, new_line))
                    fixed_candidates[key] = new_line
                index = hits[0]
            else:
                errors.append(f"{key}: 锚点丢失 {anchor!r}（{rel}）")
                continue

        if isinstance(value, str) and not isinstance(value, (int, float)):
            skipped += 1
            continue

        checked += 1
        via = entry.get("via")
        if via:
            via_path = root / str(via)
            if not via_path.exists():
                errors.append(f"{key}: via 文件不存在 {via}")
                continue
            via_lines = via_path.read_text(encoding="utf-8", errors="replace").splitlines()
            # 排除注释行：模块文档里也会出现 150ms / 0.5pt 等字样。
            matches = [
                i
                for i, line in enumerate(via_lines)
                if not line.strip().startswith("//")
                and any(same_number(v, value) for v in numbers_in(line))
            ]
            if len(matches) == 1:
                continue
            if not matches:
                errors.append(f"{key}: via 文件中找不到值 {value}（{via}）")
            else:
                where = ", ".join(str(i + 1) for i in matches[:5])
                errors.append(f"{key}: via 文件同值多处（{via}: {where}）")
            continue

        if any(same_number(v, value) for v in numbers_in(lines[index])):
            continue

        # 同一行没有再找：允许「值被移到附近」的情况，但必须唯一，否则判失配。
        window = lines[max(0, index - 80) : index + 81]
        hits = [
            index - min(index, 80) + i
            for i, line in enumerate(window)
            if any(same_number(v, value) for v in numbers_in(line))
        ]
        if len(hits) == 1:
            new_line = hits[0] + 1
            drifts.append((key, recorded, new_line))
            fixed_candidates[key] = new_line
            continue
        errors.append(f"{key}: 数值不符，期望 {value}（{rel}:{recorded}）")

    if drifts:
        for key, old, new in drifts:
            print(f"[drift] {key}: 行号 {old} → {new}")
        if strict_lines:
            errors.append(f"行号漂移 {len(drifts)} 处（--strict-lines）")

    return errors, checked, skipped, fixed_candidates


def rewrite_sources(yaml_path: Path, fixes: dict[str, int]) -> int:
    """只改 `source:` 的行号，保留注释、顺序与其余字段。"""
    lines = yaml_path.read_text(encoding="utf-8").splitlines()
    current = None
    changed = 0
    for i, line in enumerate(lines):
        indent = len(line) - len(line.lstrip(" "))
        text = line.strip()
        if indent == 6 and text.endswith(":"):
            current = text[:-1]
            continue
        if indent == 8 and text.startswith("source:") and current in fixes:
            raw = text.split(":", 1)[1].strip()
            path, _, _line = raw.strip('"').rpartition(":")
            replacement = f'        source: "{path}:{fixes[current]}"'
            if lines[i] != replacement:
                lines[i] = replacement
                changed += 1
    yaml_path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    return changed


def main() -> int:
    parser = argparse.ArgumentParser(description="校验阈值基线与源码一致")
    parser.add_argument("--yaml", default="docs/spec/thresholds.baseline.yaml")
    parser.add_argument("--root", default=None, help="仓库根（默认脚本上一级）")
    parser.add_argument("--strict-lines", action="store_true", help="行号漂移也判失败")
    parser.add_argument("--fix", action="store_true", help="回写漂移的 source 行号")
    args = parser.parse_args()

    root = Path(args.root).resolve() if args.root else Path(__file__).resolve().parent.parent
    yaml_path = (root / args.yaml).resolve()
    if not yaml_path.exists():
        print(f"ERROR: 基线文件不存在：{yaml_path}")
        return 1

    meta, entries = parse_baseline(yaml_path)
    if meta.get("runtime_consumed") is not False:
        print("ERROR: 基线必须声明 runtime_consumed: false（开发期上下文，不参与运行）")
        return 1
    if len(entries) < 100:
        print(f"ERROR: 条目数异常（{len(entries)}），疑似解析失败")
        return 1

    errors, checked, skipped, fixes = check(root, entries, args.strict_lines)

    if args.fix and fixes:
        changed = rewrite_sources(yaml_path, fixes)
        print(f"[fix] 回写 {changed} 处行号")

    print(f"thresholds: {len(entries)}（9 模块）｜数值校验 {checked}｜非数值跳过 {skipped}")

    if errors:
        for message in errors:
            print(f"FAIL: {message}")
        return 1

    if not args.fix and fixes:
        print("提示：运行 `python tools/check_thresholds.py --fix` 回写行号")
    print("OK: 基线与源码一致")
    return 0


if __name__ == "__main__":
    sys.exit(main())
