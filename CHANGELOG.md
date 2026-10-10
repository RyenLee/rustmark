# Changelog

本项目的版本遵循[语义化版本](https://semver.org/lang/zh-CN/)。
commit message 中的 `[S{阶段}-{编号}]` 对应 `.agent/tasks/` 的任务编号，
Release Notes 依此自动归组。

## [未发布]

### 阶段 09 · 测试体系、打包与发布 [S9-*]

- 测试金字塔目标显式化：`[[test]]` 声明全部分层测试目标（S9-01）。
- 覆盖率门禁：整体 ≥70%、领域 ≥90%，排除项书面化（S9-02）。
- 崩溃日志：panic hook 写盘 `crash/`，无遥测，保留最近 10 份（S9-08）。
- 三平台打包脚本（Windows zip / macOS .app+dmg / Linux AppImage+tar.gz）
  与 release workflow（tag 驱动、SHA256、Notes 归组）（S9-04/S9-06）。
- 未签名发布路径：证书成本列为可选，SmartScreen / Gatekeeper 绕过提示写入
  README 与 release 文档，私钥排除规则核对（S9-05）。
- 发布流程演练工具化：`tools/gen_release_notes.py`（Notes 改为**按阶段聚合**，
  组名标注编号区间）、`tools/release_dryrun.py`（离线演练 13/13，红路径 exit 1），
  并接入 CI `release-dryrun` job（S9-06）。
- 用户向 README、双许可（MIT OR Apache-2.0）、NOTICE、故障排查文档（S9-07）。
- 属性测试：Rope / 撤销栈 / 行列往返 3 组 × 256 例（固定种子，零新依赖）（S9-03）。

### v1.1 · P1 收官（本轮「先排除阶段 9 任务」口径）

- **V11-12 规范 15 机械门禁**：`lib.rs` 开启 `clippy::unwrap_used` / `expect_used` / `panic`
  （warn + CI `-D warnings`），测试豁免 `#[cfg_attr(test, allow(...))]`；逐处裁决 14 处非测试
  `unwrap`/`expect`，全部改写为非 panic 形态（锁损坏 `PoisonError::into_inner`、选项 `?`/`and_then`/
  `let…else`），零 `#[allow]` 残留，红路径演练通过。
- **V11-03 状态栏编码接线（DEF-05）**：共享内核纯展示型 `TextEncoding`（不污染领域层），
  基础设施探测的编码经 `DocumentSnapshot::encoding` 上报、应用层旁路表暴露、状态栏每帧同步；
  未落盘文档显示 `—` 而非谎称 UTF-8。
- **V11-04 PDF 导出开放（方案 B）**：开放 PDF 入口，明确提示「经系统打印对话框产出，请选择『另存为 PDF』」；
  打印副作用抽为可注入 `PrintDispatcher`，CI 无需真弹打印框（`bytes` 恒为 0 属预期）。
- **V11-05 写作目标进度条**：领域新增 `WritingGoal(u32)` 值对象（饱和上限 100 万、百分比截断 100、达成判定），
  `EditorSettings.writing_goal` 经 `#[serde(default)]` 兼容旧配置；状态栏显示百分比 + 细进度条，
  达成时提示（按「向上跨越」去重，切文档 / 改目标重置）。种子配置含 `writing_goal = 1000` 示例。

## [0.1.0] · 阶段 01–08（开发期）

### 功能（对齐需求 §3.1）

- 文档编辑：打开/保存/另存为、多标签、自动保存与草稿恢复、撤销/重做（合并窗口）。
- 编码：UTF-8 / UTF-8 BOM / UTF-16 LE·BE / GBK / GB18030 无损读写（含四字节增补汉字）。
- Markdown 预览：语法高亮、目录、图片（本地/远程/data URI）、公式与 Mermaid 结构化卡片。
- 查找替换：全文字数有界、正则、大小写、整词、替换预览。
- 导出：HTML（TOC/内联图片/KaTeX/Mermaid 资源注入）、Markdown 源码、图片（视口截图 / 整篇长图）。
  PDF **未开放**：基础设施（系统打印管线）已就绪，但产物需经系统打印对话框中转，界面未提供入口（v1.1 决策）。
- 工作区：文件树、大纲、全局搜索、最近文件。
- 拼写检查：内置 3779 词表 + 近邻门槛 + 波浪线标记（仅英文，演示级）。
- 设置：主题（含自定义 8 语义色）、快捷键自定义、编辑器参数、TOML 持久化（损坏自动备份回退）。
- 拖拽打开：文件与文件夹混拖，目录安全忽略。

### 质量

- 1281 自动化测试（分层金字塔），fmt/clippy/check-arch/测试四道机械门禁（覆盖率
  整体 85.27% / 领域 91.69%）。
- release 产物 ≤10MB 红线门禁（CI 强制）与性能冒烟基准。
- 架构分层（DDD 四层）由 `check-arch` 在 build.rs 构建期强制。

[未发布]: https://example.invalid/rustmark/compare/v0.1.0...HEAD
[0.1.0]: https://example.invalid/rustmark/releases/tag/v0.1.0
