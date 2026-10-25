#!/usr/bin/env bash
# 体积门禁（S8-07 ①）：release 产物不得超过红线，越过预警线只告警。
#
# 从 ci.yml 里抽出来的原因：判定规则只有一份，CI 与本地跑的是同一条，
# 不会「CI 上的规则和我在本机看的不一样」。
#
# 用法（仓库根执行）：
#   bash scripts/size_gate.sh                     # 用默认红线 11 MiB
#   SIZE_LIMIT_BYTES=5242880 bash scripts/size_gate.sh   # 调低阈值验证门禁会红
#
# 环境变量：
#   SIZE_LIMIT_BYTES  红线（字节，二进制口径），默认 11534336（11 MiB）
#   SIZE_WARN_BYTES   预警线（字节），默认 11010048（10.5 MiB）
#
# 历史：2026-10-25 由 10 MiB / 8 MiB 上调——V11-62 批次 2（state 6,682 行迁入
#   `rustmark-ui`）致 +2,560 B，旧 10 MiB 红线余量（512 B）耗尽越线；经维护者决策，
#   红线上调至 11 MiB、预警线设为 10.5 MiB（保留约 512 KiB 可行动窗口；
#   10 MiB 的旧红线语义由 NFR 历史承诺承载，不再作为告警线）。
#   GITHUB_STEP_SUMMARY  存在时额外写入 Job Summary（CI 自动提供）
#
# 退出码：0 = 未越红线；1 = 越红线或未找到产物。

set -u

LIMIT="${SIZE_LIMIT_BYTES:-11534336}"
WARN="${SIZE_WARN_BYTES:-11010048}"

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
report "字节数：$SIZE（${MB} MB）｜预警线 ${WARN_MB} MB，红线 ${LIMIT_MB} MB"

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
