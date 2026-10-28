#!/usr/bin/env bash
# 生成 rustmark workspace 依赖图快照。
#
# R-09 纪律（见 docs/architecture-remediation-plan-2026-10-27.md §遗留处置）：
# docs/{current,target}_dependency_graph.html 曾由一次性 `cargo depgraph` 生成、无脚本，
# 存在腐化风险。本脚本让快照可复现：
#
#   1. docs/workspace-topology.expected.txt —— 保留手写注释头（基线历史），只重写机器主体：
#      基于始终可用的 `cargo tree` 归一化输出覆盖「注释头之后的机器部分」，与
#      tests/workspace_topology_test.rs 的 CI 拓扑门禁对账（本地依赖边 / crate 依赖面）。
#   2. docs/{current,target}_dependency_graph.html —— `cargo depgraph` 生成的 SVG 可视化；
#      需 `cargo install cargo-depgraph`，未安装时跳过并提示，不影响 txt 快照。
#
# 用法：bash scripts/gen_dep_graph.sh   （需在 workspace 根目录上下文中运行）
set -euo pipefail
cd "$(dirname "$0")/.."

EXPECTED=docs/workspace-topology.expected.txt

# --- 1. 拓扑期望基线：保留注释头，重写机器主体 ---
echo "== 生成 $EXPECTED（保留注释头）=="
# 注释头结束于第一个非 `#` 起始行之前；保留其前的全部内容（含与主体间的空行）
first_body=$(grep -n '^[^#]' "$EXPECTED" | head -1 | cut -d: -f1)
if [ -z "$first_body" ]; then
  echo "   错误：未在 $EXPECTED 找到机器主体起始行" >&2
  exit 1
fi
head -n "$((first_body - 1))" "$EXPECTED" > /tmp/dep_head.txt
cargo tree --workspace --depth 1 --no-dedupe \
  | sed -E 's| \([^)]*\)||g' \
  > /tmp/dep_body.txt
cat /tmp/dep_head.txt /tmp/dep_body.txt > "$EXPECTED"
echo "   注释头（前 $((first_body - 1)) 行）保留，机器主体已重写"

# --- 2. HTML 可视化（需 cargo-depgraph）---
if cargo depgraph --version >/dev/null 2>&1; then
  echo "== 生成 docs/current_dependency_graph.html（SVG 可视化）=="
  cargo depgraph --workspace --format svg > docs/current_dependency_graph.html
  echo "   已写入 docs/current_dependency_graph.html"
  echo "   （target 依赖图为目标态快照，需手动对照 ADR-013 外移触发条件后覆盖）"
else
  echo "== 跳过 HTML 可视化 =="
  echo "   未安装 cargo-depgraph（运行：cargo install cargo-depgraph）。"
  echo "   docs/{current,target}_dependency_graph.html 为 2026-10-27 冻结快照；"
  echo "   需重新可视化时请安装后运行本脚本覆盖。"
fi

echo "完成。"
