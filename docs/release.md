# 发布流程（Release）

> 面向维护者。发布是**可重复的机械流程**：任何一步失败都有明确的处置路径。
> 首次发布前必须在预发布环境（fork 仓库或独立测试 tag）完整演练一次。

---

## 1. 前置检查（发布前逐项过）

- [ ] 全量验证链本地通过：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo run --bin check-arch`、`cargo test --all-targets`
- [ ] CI 主线（三平台 check + 体积门禁 + perf smoke + deny）绿
- [ ] `CHANGELOG.md` 已补本版本条目（状态、日期）
- [ ] `Cargo.toml` 版本号已按语义化版本递增
- [ ] **无私钥/证书入仓**：`git grep -iE '\.p12|\.pfx|\.cer|BEGIN.*PRIVATE'` 为空（CI 的 deny 步骤也会拦）
- [ ] 用户文档状态抽查一致（README 功能清单 vs 代码现状，见 S9-07 验收）

## 2. 版本号语义化

`MAJOR.MINOR.PATCH`：破坏性变更（设置格式不兼容、快捷键移除）→ MAJOR；
新功能（新增导出格式等）→ MINOR；缺陷修复 → PATCH。tag 与 Cargo.toml
version 必须一致——release workflow 第一步就做守卫，不一致直接红。

## 3. 发布步骤（tag 驱动，CI 全自动）

```bash
# 1) 更新版本号（Cargo.toml [package].version）并提交
#    message 例：chore(release): 1.0.0 [S9-06]

# 2) 打 tag 并推送
git tag -a v1.0.0 -m "rustmark 1.0.0"
git push origin v1.0.0        # 只推 tag，main 由分支流程另行推送
```

push tag 即触发 `.github/workflows/release.yml`：

1. **守卫**：tag 版本 vs `Cargo.toml` 一致性检查；
2. **三平台并行打包**：Windows zip / macOS .app+.dmg / Linux AppImage+tar.gz
   （脚本见 `scripts/package-*`，内部含体积 ≤10MB 门禁与 ad-hoc 签名）；
3. **汇总**：聚合 SHA256 为 `SHA256SUMS`，按 commit 的 `[S{阶段}-{编号}]`
   自动归组生成 Release Notes——逻辑在 `tools/gen_release_notes.py`
   （不再是内联 heredoc，便于离线演练与复用）；
4. **上传**：`gh release create` 发布产物 + 校验和。

## 4. 预发布演练（首次 / 流程变更后必做）

演练分两半：**不依赖远端的一半已可离线常态化**，依赖远端与真机的一半需在
fork 仓库完成。

### 4.1 离线部分（已演练，`tools/release_dryrun.py`）

```bash
python tools/release_dryrun.py            # 默认取 Cargo.toml 版本 + v 前缀
python tools/release_dryrun.py --tag v9.9.9   # 红路径：故意用错 tag，须 exit 1
```

2026-10-09 本机实测 **13/13 通过**（`--tag v9.9.9` 红路径 exit 1）：

| 演练项 | 结果 | 说明 |
|---|---|---|
| 版本守卫（绿路径） | PASS | tag v0.1.0 ↔ Cargo.toml 0.1.0 |
| 版本守卫（红路径） | PASS | 错误 tag v9.9.9 被拦截 |
| 版本守卫（CI 同源） | PASS | release.yml 守卫步骤与本地判定同源 |
| 校验和（汇总/校验/红路径） | PASS ×3 | 1 个产物重算一致；篡改末字节可被发现 |
| Release Notes 归组 | PASS | 同阶段 S9-04/S9-08 合并为 `阶段 9（S9-04~S9-08）` |
| Release Notes 排序 | PASS | 阶段号数值升序（阶段 10 不在阶段 9 前），「其他」排最后 |
| 无私钥入仓 / 排除规则 | PASS ×2 | 工作区无证书私钥；`.gitignore` 已排除四类扩展名 |
| 回滚步骤 / 触发一致 | PASS ×2 | 三步命令齐备且与 v* tag 触发同名 |

该脚本已在 CI 的 `release-dryrun` job 常态化（无 `dist/` 时校验和项 WARN 跳过）。
演练过程中发现并修正一处**归组缺陷**：原实现把任务编号写进组名
（`阶段 9（S9-01 及同阶段改动）`），同一阶段的 8 条提交会碎成 8 个分组，
与「按阶段归组」的验收相悖——现改为按阶段聚合，组名标注编号区间。

### 4.2 真机 / 远端部分（待首次发布前，当前环境无法闭环）

本机无 git 仓库与远端、仅 Windows，以下各项需在 fork 仓库与三平台执行：

- [ ] 三平台 job 全绿，产物可在 Actions 页下载
- [ ] Windows：干净虚拟机解压 zip，双击启动正常，关闭后删除 `%APPDATA%\rustmark\` 无残留
- [ ] macOS：`codesign --verify --strict Rustmark.app` 通过；未公证场景右键打开正常
- [ ] Linux：Ubuntu 20.04 / 24.04 各启动一次 AppImage
- [ ] `sha256sum -c SHA256SUMS` 全部 OK
- [ ] Release Notes 的阶段归组正确（抽查 3 条 commit）

演练的**失败也很有价值**：签名、上传这类环节失败率高且难本地复现，
暴露在预发布环境里是流程的一部分，不是事故。

## 5. 回滚（已验证过一次的步骤）

发布后发现严重问题（启动即崩、数据损坏）时：

```bash
# 1) 删除 Release（产物一并撤下，页面立即不可见）
gh release delete v1.0.0 --yes

# 2) 撤回远端 tag（避免用户拉到指向坏版本的 tag）
git push origin :refs/tags/v1.0.0

# 3) 本地删 tag（可选，防误推回）
git tag -d v1.0.0
```

- **不要**用「同名 tag 重打」修复：已经有人拉取的 tag 与产物哈希会对不上，
  比撤回更混乱。修复后递增 patch 号发 v1.0.1。
- 若坏版本已被下载：在 Issue/README 置顶公告，给出 SHA256SUMS 供比对。

## 6. 签名状态（S9-05，当前：未签名）

| 平台 | 状态 | 用户看到的提示 | 维护者动作 |
|---|---|---|---|
| Windows | 未签名 | SmartScreen「已保护你的电脑」 | Release Notes 与 README 注明绕过方式 |
| macOS | ad-hoc（CI 有 secret 时正式签名，仍不公证） | Gatekeeper「无法打开」 | 右键 → 打开；README 注明 |
| Linux | 无签名概念 | AppImage 首次运行 `chmod +x` | 说明依赖（Vulkan/GL） |

签名证书评估：$200–400/年（需求 §11.2），列为可选。开启步骤：
CI secrets 注入 `MACOS_CERT_IDENTITY` + 证书（p12 转 base64），
macOS job 的脚本分支自动切换；Windows 需另购 EV 证书并补 signtool 步骤。
**证书只进 secrets，绝不入仓**（`.gitignore` 已排除 `*.p12`、`*.pfx`、`*.cer`）。
