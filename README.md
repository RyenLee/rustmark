# rustmark · 原生轻量级 Markdown 编辑器

Rust 2024 + egui（wgpu 后端）的桌面 Markdown 编辑器：单文件产物、零遥测、
GBK/GB18030 无损读写、分屏实时预览。

- 许可：MIT OR Apache-2.0（双许可，见 `LICENSE-MIT` / `LICENSE-APACHE`）
- 当前版本：v0.1.0（v1.0 发布就绪中，见 `CHANGELOG.md`）
- 平台：Windows / macOS / Linux

---

## 1. 功能清单（状态与代码同步核对）

| 功能 | 优先级 | 状态 |
|---|---|---|
| 语法高亮（增量，变更行级） | P0 | ✅ 已实现 |
| 撤销 / 重做（连续输入合并） | P0 | ✅ 已实现 |
| 查找 / 替换（正则 / 大小写 / 整词 / 全文搜索有界） | P0 | ✅ 已实现 |
| 分屏实时预览 + 双向滚动同步（按块映射，防抖 150ms） | P0 | ✅ 已实现 |
| Markdown 渲染：标题 / 列表 / 引用 / 表格（GFM）/ 任务列表（点击回写）/ 链接（**预览内可点击**：外部地址交系统默认程序，`#锚点` 页内跳转） | P0 | ✅ 已实现 |
| 图片显示（本地 / 远程 URL / data URI） | P0 | ✅ 已实现 |
| 新建 / 打开 / 保存（原子写：临时文件 + rename）/ 另存为 | P0 | ✅ 已实现 |
| 导出 HTML（目录 / 内联图片 / KaTeX 与 Mermaid 资源按需注入） | P0 | ✅ 已实现 |
| 快捷键系统（全键可自定义，冲突检测） | P0 | ✅ 已实现 |
| 多标签编辑 | P1 | ✅ 已实现 |
| 数学公式（LaTeX）/ Mermaid 图表 | P1 | ✅ 已实现（**编辑器内预览直接渲染**：自研 LaTeX 子集排版引擎 + graph/flowchart 分层布局；内置数学兜底字体覆盖常用符号（无方框）；子集外写法回退源码卡；导出 HTML 后由浏览器端 KaTeX/Mermaid 渲染） |
| 导出 PDF | P1 | ✅ 已实现（Windows 走无头 Edge/Chrome `--print-to-pdf` 静默落盘真实 PDF；无头内核不可用时回退系统打印对话框；v1.1 V11-04 / ADR-004 修订） |
| 最近文件 | P1 | ✅ 已实现 |
| 格式工具栏 | P1 | ✅ 已实现 |
| 深色 / 浅色主题（含「跟随系统」） | P1 | ✅ 已实现 |
| 大纲跳转 | P1 | ✅ 已实现 |
| 等宽字体选择（内置 JetBrains Mono / Fira Code / IBM Plex Mono + 项目 `fonts/` 目录 + 常见系统等宽字体；设置页下拉，切换即时生效） | P1 | ✅ 已实现（2026-10-13 发现机制，2026-10-14 内置三款，v1.1 V11-21） |
| 拖拽打开（文件 / 文件夹混拖，目录安全忽略） | P2 | ✅ 已实现 |
| 自定义主题（TOML：8 项核心语义色 + 10 项语法高亮色可选覆盖） | P2 | ✅ 已实现 |
| 文件树侧边栏（新建 / 重命名 / 删除，删除带二次确认） | P2 | ✅ 已实现 |
| 拼写检查（仅英文） | P2 | ✅ 已实现（内置词表 + 编辑距离近邻门槛 + 波浪线 + 右键替换建议） |
| 图片导出（PNG：视口截图 / 整篇长图） | P2 | ✅ 已实现 |
| 编码支持：UTF-8 / UTF-8 BOM / UTF-16 LE·BE / GBK / GB18030 读写无损 | 增补 | ✅ 已实现 |
| 自动保存（默认停顿 30s）+ 异常退出草稿恢复 | 增补 | ✅ 已实现 |
| 命令面板 / 符号插入 / 文档统计 / 文件对比 | 增补 | ✅ 已实现 |
| Markdown 格式检查（lint） | P2 | ✅ 已实现（4 规则 + 开关 + 侧栏「检查」标签跳转） |

> 状态核对基准：`REQUIREMENTS.md` §3、`docs/archive/stage-reports/p1p2-implementation-log.md`、
> `.agent/tasks/`（各任务单元验收输出）。

## 2. 安装与卸载

### 安装

| 平台 | 产物 | 安装方式 |
|---|---|---|
| Windows | `rustmark-v*-windows-x64.zip` | 解压即用（便携），运行 `rustmark.exe` |
| macOS | `rustmark-v*-macos.dmg` / `.app` zip | dmg 拖入「应用程序」，或解压 zip 直接运行 |
| Linux | `rustmark-v*-linux-x64.AppImage` / `.tar.gz` | `chmod +x` 后直接运行 |

下载后请校验：`sha256sum -c SHA256SUMS`（发布页附带）。

**未签名提示**（当前版本未购买签名证书）：

- Windows：首次运行 SmartScreen 提示「已保护你的电脑」→ 点「更多信息 → 仍要运行」；
- macOS：Gatekeeper 拦截 → 右键 App → 打开；或
  `xattr -d com.apple.quarantine /Applications/Rustmark.app`；
- Linux：无签名机制，按发行版惯例处理。

**系统依赖**（Linux）：Vulkan 或 OpenGL（`libvulkan1` / mesa）；
wgpu 通过系统图形栈渲染，无窗口管理器外的额外依赖。

### 卸载

- Windows：删除 exe 与 `%APPDATA%\rustmark\`；
- macOS：删除 Rustmark.app 与 `~/Library/Application Support/rustmark/`；
- Linux：删除 AppImage 与 `~/.config/rustmark/`。

配置目录内含个人设置与草稿，删除即完全清除（无残留服务、无注册表项）。

## 3. 快捷键（默认绑定，全部可自定义）

| 动作 | 快捷键 | 动作 | 快捷键 |
|---|---|---|---|
| 新建 | `Ctrl+N` | 打开 | `Ctrl+O` |
| 保存 | `Ctrl+S` | 全部保存 | `Ctrl+Alt+S` |
| 另存为 | `Ctrl+Alt+Shift+S` | 关闭标签 | `Ctrl+Alt+W` |
| 导出 | `Ctrl+E` | 撤销 / 重做 | `Ctrl+Z` / `Ctrl+Shift+Z` |
| 查找 / 替换 | `Ctrl+F` / `Ctrl+H` | 全局搜索 | `Ctrl+Shift+F` |
| 粗体 / 斜体 | `Ctrl+B` / `Ctrl+I` | 删除线 | `Ctrl+Shift+X` |
| 行内代码 | `Ctrl+Shift+C` | 插入链接 | `Ctrl+K` |
| 代码块 | `Ctrl+Shift+K` | 表格 | `Ctrl+Shift+T` |
| 引用 | `Ctrl+Shift+Q` | 无序 / 有序列表 | `Ctrl+Shift+8` / `Ctrl+Shift+7` |
| 提升标题 | `Ctrl+=` | 降级标题 | `Ctrl+-` |
| 缩进 / 反缩进 | `Ctrl+]` / `Ctrl+[` | 任务勾选切换 | `Ctrl+Shift+Enter` |
| 预览开关 | `Ctrl+P` | 大纲 | `Ctrl+Shift+O` |
| 侧栏开关 | `Ctrl+\` | 文档统计 | `Ctrl+Shift+S` |
| 命令面板 | `Ctrl+Shift+P` | 符号插入 | `Ctrl+Shift+E` |
| 文件对比 | `Ctrl+Alt+D` | 设置 | `Ctrl+,` |
| 下 / 上一个标签 | `Ctrl+Shift+]` / `Ctrl+Shift+[` | | |

**自定义方法**：设置（`Ctrl+,`）→ 快捷键 → 点击要改的动作 → 按下新组合键。
冲突的组合会被**当场拒绝**并提示与哪个动作冲突；修改保存在
`preferences.toml`，随设置迁移。

## 4. 已知限制（与未实现项）

1. **PDF 导出优先无头浏览器，回退系统打印对话框**：Windows 上优先调用随系统预装的
   Edge（无则 Chrome）以 `--print-to-pdf` **静默落盘真实 PDF**（无需任何对话交互）；
   无头内核不可用时回退到系统打印对话框（此时产物不落在应用内，`bytes` 为 0，属预期），
   界面会据「是否真落盘」给出不同提示（v1.1 V11-04 / ADR-004 修订）。
2. **拼写检查已增强（v1.1 V11-07）**：仍**仅英文**（中文与其它语言直接跳过）；
   内置词表已扩充，并在带波浪线的错词上右键可弹出「你可能想输入」候选建议并一键替换。
   生僻词 / 专有名词仍依赖词表覆盖（误报 0 的保守策略）；建议算法覆盖编辑距离 ≤2。
3. **公式与 Mermaid 编辑器内只支持子集**：预览区现已**直接渲染**常见 LaTeX
   （分数 / 根式 / 上下标 / 大运算符 / 矩阵 / 定界符自适应等）与 Mermaid `graph` /
   `flowchart`（节点形状 + 有向边 + 链式 + 标签）；**子集外**的写法
   （如 `sequenceDiagram` / `classDiagram` / `gantt` 等其它图种、`subgraph`）
   **不猜测**，统一回退为等宽源码卡。导出 HTML 后由浏览器端 KaTeX / Mermaid
   完整渲染（导出件已按需注入 CDN 资源）。
4. **自定义主题已扩展（v1.1 V11-09）**：8 项核心语义色为必填；另有 10 项语法高亮色
   （标题 / 加粗 / 斜体 / 代码 / 链接 / 引用 / 标记符 / 元信息 / 表格 / 其它）可**按项覆盖**。
   未覆盖的语法色**跟随主题**（按主背景亮度自动取内置深色或浅色同类值），
   旧自定义主题文件加载后所有语法色即为「跟随」。
5. **长图导出的宽度 = 预览区宽度**：超出预览区宽度的内容不在产物内；
   超过 256 屏或 16384px 高度上限时建议改用 HTML 导出。
6. **Markdown 格式检查（lint）已实现（v1.1 V11-08）**：4 条规则（标题层级跳跃 /
   列表符号混用 / 行尾空格 / 未闭合代码围栏），可在「设置 → 语法检查」逐条开关，
   结果显示在侧栏「检查」标签，点击问题可跳转到对应行。规则刻意精简，
   且**跳过围栏代码块内部**（代码里的行尾空格、`#` 注释不算问题）。
7. **macOS / Linux 未经真机全面回归**：构建与产物脚本就绪，真机验收
   （启动、字体路径、打包体积实测）见 `docs/07-*` 手工清单与 `tasks/09` 状态段。
8. **状态栏已显示文档实际编码（v1.1 V11-03）**：底层按文件自动识别并写回原编码
   （UTF-8 / UTF-16 / GBK / GB18030），状态栏现已显示真实编码名；未落盘文档显示 `—`
   （不谎称 UTF-8）。写作目标进度条亦已在 v1.1 V11-05 打通（设置填目标字数 → 状态栏百分比 +
   细进度条 + 达成提示）。
9. **超大文档（1MB 档）编辑性能为「尽力而为」**：≤100KB 文档对编辑响应 16ms
   （`NFR-PERF-04`）有 ≥9× 余量；**1MB 档 p50 已达标**（14.35ms）、**p95 贴 16ms
   线未稳定达标**（15.6~17.3ms，2026-10-19 同机 A/B 5 次复测，历程与数据见
   `docs/metrics.md` §3.2.13）。继续下压需绕过 egui `TextEdit` 的 galley 契约
   （自绘只读视图 / 视口虚拟化），成本远超收益，本期不做（V11-41 口径 A 裁定；
   「自绘 vs 修订口径」议题 T5 已由 §3.2.13 的排版层「原地补丁」落地闭环，
   4.57 → 0.241ms）。日常文档（<1MB）不受影响。

## 5. 故障排查

见 [docs/troubleshooting.md](docs/troubleshooting.md)，覆盖：配置目录位置、
重置设置、找回草稿、崩溃报告的提交方式（本地写盘、无遥测、不含文档内容）。

## 6. 从源码构建

```bash
# 工具链：Rust 1.99.0（rust-toolchain.toml 已固定）
cargo build --release --bin rustmark   # 产物 target/release/rustmark*

# 全量验证链（贡献者必过）
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test  --all-targets
cargo run   --bin check-arch
```

> 性能门禁说明（V11-13）：`tests/integration/large_file_test.rs` 里的耗时断言
> 只在 **release** 构建生效，debug 下仅打印指标、不报错，因此 `cargo test` 全量
> 无需 `--skip`。release 性能门禁由 CI 的 `perf` job 以
> `cargo test --release --test integration large` 执行。

可选的本地门禁（与 CI 同款，纯标准库 Python，无额外依赖）：

```bash
cargo llvm-cov --all-targets --lcov --output-path lcov.info
python tools/coverage_gate.py lcov.info   # 覆盖率门禁：整体 ≥70%、领域 ≥90%
python tools/release_dryrun.py            # 离线发布演练：版本守卫 / 校验和 / Notes 归组
```

打包：`scripts/package-windows.ps1` / `package-macos.sh` / `package-linux.sh`，
发布流程见 [docs/release.md](docs/release.md)。

## 7. 开发者文档

- 开发计划与阶段进度：[docs/PROGRESS.md](docs/PROGRESS.md)（原 README 索引）
- 下一阶段（v1.1）任务规划：[docs/v1.1-backlog.md](docs/v1.1-backlog.md)
- 编码规范：`.agent/conventions.md`；架构与分层：`docs/adr/`
- 测试体系与覆盖率门禁：[docs/testing.md](docs/testing.md)
- 需求基线：[REQUIREMENTS.md](REQUIREMENTS.md)
- 贡献指南：[CONTRIBUTING.md](CONTRIBUTING.md)

## 8. 许可

双许可：`MIT OR Apache-2.0`，可任选其一。第三方组件清单见 [NOTICE](NOTICE)。
