#!/usr/bin/env bash
# 体积门禁（S8-07 ①）：release 产物不得超过红线，越过预警线只告警。
#
# 从 ci.yml 里抽出来的原因：判定规则只有一份，CI 与本地跑的是同一条，
# 不会「CI 上的规则和我在本机看的不一样」。
#
# 用法（仓库根执行）：
#   bash scripts/size_gate.sh                     # 用当前平台的默认红/预警线
#   SIZE_LIMIT_BYTES=5242880 bash scripts/size_gate.sh   # 调低阈值验证门禁会红
#
# 环境变量：
#   SIZE_LIMIT_BYTES  红线（字节，二进制口径）；未设时按平台取默认值
#   SIZE_WARN_BYTES   预警线（字节）；未设时按平台取默认值
#   SIZE_GATE_SKIP_BUILD  设为 1 时跳过自动构建（默认会先 cargo build --release 以保证产物新鲜）
#
# 平台分档（2026-11-01）：Linux 独有 `x11` + `wayland` + Vulkan 后端栈，二进制比
#   Windows/macOS 大一个量级（首次 CI 实测 13,371,056 B = 12.752 MiB）。原 11 MiB
#   红线是按 Windows 数据定的（metrics §2.8 的基线全部是 x86_64-pc-windows-msvc），
#   Linux 从未复测（§2.7 备注）。经维护者裁定按平台分档：
#     Linux                 13.5 MiB 红线 / 13.0 MiB 预警（实测 + 约 0.75 MiB 窗口）
#     Windows / macOS       11 MiB 红线 / 10.5 MiB 预警（不变）
#   未设环境变量时由 `uname -s` 决定；CI 不再把 11 MiB 写进全局 env，否则 Linux 会
#   被无条件按 Windows 口径判红（workflow_dispatch 的 size_limit_bytes 留空即走默认）。
#   注意：这是**二进制口径**；安装包口径（tar.gz / AppImage / dmg / zip）的红线
#   另见 threshold 基线 `package_size_limit_*`，不受本分档影响。
#
# 历史：2026-10-25 由 10 MiB / 8 MiB 上调——V11-62 批次 2（state 6,682 行迁入
#   `rustmark-ui`）致 +2,560 B，旧 10 MiB 红线余量（512 B）耗尽越线；经维护者决策，
#   红线上调至 11 MiB、预警线设为 10.5 MiB（保留约 512 KiB 可行动窗口；
#   10 MiB 的旧红线语义由 NFR 历史承诺承载，不再作为告警线）。
#   GITHUB_STEP_SUMMARY  存在时额外写入 Job Summary（CI 自动提供）
#
# 退出码：0 = 未越红线；1 = 越红线或未找到产物。

set -u

# 平台默认值（Linux 分支的数值即 NFR-PERF-01 的 Linux 分档，改动须同步
# docs/spec/metrics.md §2.9 与 docs/spec/thresholds.baseline.yaml）。
case "$(uname -s)" in
  Linux)
    DEFAULT_LIMIT=14155776
    DEFAULT_WARN=13631488
    ;;
  *)
    DEFAULT_LIMIT=11534336
    DEFAULT_WARN=11010048
    ;;
esac

LIMIT="${SIZE_LIMIT_BYTES:-$DEFAULT_LIMIT}"
WARN="${SIZE_WARN_BYTES:-$DEFAULT_WARN}"

# ---------------------------------------------------------------------------
# 先构建，再测量（2026-10-27 修复）
#
# 原实现只 `find` 现成的 release 产物量字节数，**从不触发编译**。于是：
# 改了源码但没人跑过 `cargo build --release` 时，脚本会照常量到**陈旧产物**
# 并报「通过 / 与基线一致」——一个静默的假绿。R-06 批次就踩中了它：实施者
# 报「体积零变化」，实测重编后其实是 +512 B。
#
# 故此处让脚本自己负责新鲜度：先 `cargo build --release`（增量，CI 上因前一步
# 已构建几乎不额外耗时），再测量。这样「本地跑的」和「CI 跑的」不只是判定
# 规则同源，**被测对象也同源**。
#
# 逃生口：确需只量不建（例如产物已由外部流水线产生）时设
#   SIZE_GATE_SKIP_BUILD=1
# ---------------------------------------------------------------------------
if [ "${SIZE_GATE_SKIP_BUILD:-0}" != "1" ]; then
  cargo build --release || exit 1
fi

# 产物路径随是否指定 --target 而不同：显式 target 时为
# target/<triple>/release/，否则为 target/release/。此处两者都找。
BIN=$(find target -type f \( -name 'rustmark' -o -name 'rustmark.exe' \) \
      -path '*release*' -not -path '*/deps/*' | head -1)

if [ -z "$BIN" ]; then
  echo "::error::未找到 release 产物，体积门禁无法判定（先跑 cargo build --release）"
  exit 1
fi

SIZE=$(wc -c < "$BIN")
MB=$(awk "BEGIN{printf \"%.3f\", $SIZE/1048576}")
LIMIT_MB=$(awk "BEGIN{printf \"%.3f\", $LIMIT/1048576}")
WARN_MB=$(awk "BEGIN{printf \"%.3f\", $WARN/1048576}")

report() { echo "$1"; }

report "release 产物：$BIN"
report "字节数：${SIZE}（${MB} MB）｜预警线 ${WARN_MB} MB，红线 ${LIMIT_MB} MB"

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  {
    echo "### 体积门禁"
    echo ""
    echo "- 产物：$BIN"
    echo "- 字节数：**$SIZE**（${MB} MB）"
    echo "- 预警线 ${WARN_MB} MB / 红线 ${LIMIT_MB} MB"
  } >> "$GITHUB_STEP_SUMMARY"
fi

if [ "$SIZE" -gt "$LIMIT" ]; then
  echo "::error::release 产物 ${MB} MB 超过红线 ${LIMIT_MB} MB（$SIZE > $LIMIT 字节）"
  exit 1
fi

if [ "$SIZE" -gt "$WARN" ]; then
  echo "::warning::release 产物 ${MB} MB 已超过预警线 ${WARN_MB} MB，距红线仅剩 $((LIMIT - SIZE)) 字节"
fi
