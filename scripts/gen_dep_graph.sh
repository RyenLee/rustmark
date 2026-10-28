#!/usr/bin/env bash
# 生成 rustmark workspace 依赖图快照。
#
# R-09 纪律（见 docs/remediation/r01-r11/remediation-plan-2026-10-27.md §遗留处置）：
# docs/{current,target}_dependency_graph.html 曾由一次性 `cargo depgraph` 生成、无脚本，
# 存在腐化风险。本脚本让快照可复现：
#
#   1. docs/spec/workspace-topology.expected.txt —— 保留手写注释头（基线历史），只重写机器主体：
#      基于始终可用的 `cargo tree` 归一化输出覆盖「注释头之后的机器部分」，与
#      tests/workspace_topology_test.rs 的 CI 拓扑门禁对账（本地依赖边 / crate 依赖面）。
#   2. target/dependency-graph.generated.svg —— `cargo depgraph` 生成的机器图，
#      落到 target/（不入库）供临时查看。需 `cargo install cargo-depgraph`，
#      未安装时跳过并提示，不影响 txt 快照。
#
# **本脚本不写 docs/ 下的任何图**。`docs/spec/current_dependency_graph.html` 是**人工维护**的
# 架构示意图（分层布局 + 中文说明，信息量高于 cargo depgraph 的原始输出）；早先版本用
# `cargo depgraph > docs/spec/current_dependency_graph.html` 整体覆盖，会把这幅手绘图抹掉——
# 2026-10-28 已改为输出到 target/，两者不再互斥。
#
# 用法：bash scripts/gen_dep_graph.sh   （需在 workspace 根目录上下文中运行）
set -euo pipefail
cd "$(dirname "$0")/.."

EXPECTED=docs/spec/workspace-topology.expected.txt

# --- 1. 拓扑期望基线：保留注释头，重写机器主体 ---
echo "== 生成 $EXPECTED（保留注释头）=="
# 注释头 = 直到**最后一个** `#` 起始行；与机器主体之间的空行**不保留**。
#
# 为什么不能按「第一个非 `#` 起始行」切：CI 用 `grep -v '^#'` 剥掉注释头后与
# `cargo tree` 输出做 diff，空行不以 `#` 开头、`grep -v` 会留下来，于是 diff 恒不等、
# 拓扑断言常红（2026-10-28 实测：文件第 90 行的空行让 CI 该步骤一直红，而本地
# 七门禁不含这一步，故长期无人发现）。按最后一个 `#` 行切可让本脚本幂等：
# 重跑产生的字节与 CI 比对口径完全一致。
last_comment=$(grep -n '^#' "$EXPECTED" | tail -1 | cut -d: -f1)
if [ -z "$last_comment" ]; then
  echo "   错误：未在 $EXPECTED 找到注释头" >&2
  exit 1
fi
head -n "$last_comment" "$EXPECTED" > /tmp/dep_head.txt
cargo tree --workspace --depth 1 --no-dedupe \
  | sed -E 's| \([^)]*\)||g' \
  > /tmp/dep_body.txt
cat /tmp/dep_head.txt /tmp/dep_body.txt > "$EXPECTED"
echo "   注释头（前 $last_comment 行）保留，机器主体已重写"

# --- 2. 机器图（需 cargo-depgraph，落 target/ 不入库）---
GEN_SVG=target/dependency-graph.generated.svg
if cargo depgraph --version >/dev/null 2>&1; then
  echo "== 生成 $GEN_SVG（机器图，不入库）=="
  mkdir -p "$(dirname "$GEN_SVG")"
  cargo depgraph --workspace --format svg > "$GEN_SVG"
  echo "   已写入 $GEN_SVG"
  echo "   注意：docs/spec/current_dependency_graph.html 是人工维护的示意图，本脚本**不覆盖**它。"
else
  echo "== 跳过机器图 =="
  echo "   未安装 cargo-depgraph（运行：cargo install cargo-depgraph）。"
  echo "   txt 拓扑基线不受影响；docs/ 下的示意图本就由人工维护，无需重新生成。"
fi

echo "完成。"
