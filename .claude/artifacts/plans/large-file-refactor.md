# DDD 大文件专项拆分重构 Implementation Plan

> Status: APPROVED（带 5 项保留意见，见 Critic verdict）
> Source: 用户请求（无既有 spec；范围清晰，5 条明确要求）
> Mode: --deliberate（全项目跨层重构；用户明确要求分阶段风险点与回滚预案）
> Iterations: 1 / 3
> Author: ryenlii
> Last updated: 2026-10-11
> Baseline commit: `674ce65`（分支 `main`，working tree clean）

---

## Requirements summary

当前 DDD 分层项目（rustmark，egui Markdown 编辑器）存在多个单文件体量过大的问题。需要一套**纯机械搬迁、零业务逻辑变更**的专项重构方案：全项目扫描并按 DDD 分层分类标记超阈值文件（重点领域层聚合根/实体/领域服务），按文件类型适用不同拆分维度（领域服务按业务能力、聚合/值对象按内聚边界、应用服务按用例、基础设施按技术能力），增量迭代、每个文件拆分后立即跑分层测试，输出路线图、逐文件拆分方案、目标目录结构、回归验证标准、风险点与回滚预案，全程不影响业务正常运行。

## Acceptance criteria

- **AC-1（行数达标）**：扫描发现的 31 个 >500 行文件，拆分后 facade 与每个子文件均 ≤500 行（子文件目标 150–450 行）；用与基线相同的行数统计命令复核。
- **AC-2（零逻辑变更）**：每个 commit 的 diff 只允许出现四类变更：①代码块整体移动；②`mod`/`use`/`pub use` 声明；③可见性关键字（私有 → `pub(crate)`/`pub(super)`）；④`include_bytes!` 相对路径字符串。**禁止**任何表达式、分支、执行顺序、错误信息、返回值的修改；禁止顺手格式化未搬迁代码（`cargo fmt` 产生的全文件格式化必须在同 commit 内可辨识、或单独 commit）。
- **AC-3（路径零破坏）**：所有既有模块路径保持可解析。现有测试源码（`tests/**`）与 `src/**` 内部的 `use rustmark::...` / `use crate::...` **零修改**（除非编译器证明必须且仅加 `use` 行）；`cargo check --all-targets` 通过。
- **AC-4（测试结果逐字节一致）**：全量测试结果与基线完全相同——**1572 passed / 1 failed / 1 ignored**，且失败用例名必须仍是 `infrastructure::pdf_exporter_test::real_export_writes_a_real_pdf_to_the_chosen_path`（沙箱无头 Edge 环境限制），ignored 必须仍是 `integration::golden_fixture::generate_large_fixture`。任何新增失败、失败名变化、ignore 变化均阻断。
- **AC-5（门禁全绿）**：每个 commit 后 `cargo run --bin check-arch` 退出码 0、`cargo clippy --all-targets -- -D warnings` 干净、`cargo fmt --check` 干净。
- **AC-6（golden 快照不变）**：`tests/data/golden/*.expected` 与两个 `*_parses_to_stable_snapshot` 等快照零改动、零重新生成。
- **AC-7（覆盖率不降）**：`python tools/coverage_gate.py lcov.info` 维持整体 ≥70%、`src/domain/**` ≥90%。
- **AC-8（发布级验证）**：`cargo build --release` 通过且产物不超过 10 MB 红线（预期体积几乎不变）；波次 5/6 后执行 `docs/manual-qa-windows.md` 手工清单。
- **AC-9（可回滚）**：每个原始文件 = 1 个独立 commit，可单独 `git revert`；任一 commit 后测试不绿即回滚该 commit，不得带病前进。

---

## 1. 现状扫描结果（基线实测，2026-10-11）

### 1.1 阈值定义

| 等级 | 行数 | 处置 |
|---|---|---|
| 红线 | **>500 行** | 本次必须拆分，31 个文件 |
| 黄线 | 300–500 行 | 仅登记为 follow-up，本次默认不动（23 个，见 §1.4） |
| 正常 | <300 行 | 不处理 |

统计口径：`[System.IO.File]::ReadAllText` 后按 `` `n `` 计数（PowerShell `Measure-Object -Line` 统计偏小，已弃用）。

### 1.2 红线文件按 DDD 分层分类（31 个）

分层映射依据 [02-工程骨架与分层边界.md](file:///d:/data/WorkBuddyCache/workspace/rustmark/docs/02-%E5%B7%A5%E7%A8%8B%E9%AA%A8%E6%9E%B6%E4%B8%8E%E5%88%86%E5%B1%82%E8%BE%B9%E7%95%8C.md) 与 [arch_check.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/tools/arch_check.rs)（`app/` 为组合根；presentation 即用户接口层；依赖方向 presentation → application → domain ← infrastructure）。

#### 用户接口层 · 组合根 app/（1 个，最高优先级单点）

| 文件 | 行数 | 臃肿性质 |
|---|---|---|
| [frame.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/app/frame.rs) | **3607** | `EditorApp` 一个巨型 `impl`（L460-3354，~2900 行）混合对话框/截图/工作区/标签页/保存/恢复/拖拽/查找/设置/格式/动作路由 13 类职责 |

#### 用户接口层 · presentation/（17 个）

| 文件 | 行数 | 臃肿性质 |
|---|---|---|
| [mermaid.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/mermaid.rs) | 3057 | 内置 Mermaid 引擎：graph/sequence/class/gantt 4 种图的解析+布局+绘制 + 455 行内联测试 |
| [math.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/math.rs) | 2069 | 内置 LaTeX 子集引擎：符号表+Parser(~550)+layout_node(~450) + 284 行测试 |
| [preview_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/preview_view.rs) | 1872 | 预览入口+15 种 block 绘制器+图片缓存+排版 |
| [mapper.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/theme/mapper.rs) | 1307 | 拼写波浪线+高亮缓存+高亮布局+StyleMapper+主题 apply + 225 行测试 |
| [registry.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/shortcuts/registry.rs) | 1184 | `UiAction` 大枚举+按键模型+KeyCombo 解析+ShortcutRegistry+命令面板模糊匹配 |
| [find_replace_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/find_replace_ui_state.rs) | 1156 | **内含一个迷你正则引擎**（Pattern/Node/编译/匹配）+ Finder 扫描 + UI 状态；DEF-01 所在文件 |
| [diff_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/diff_ui_state.rs) | 807 | diff 模型+LCS 算法 + 253 行测试 |
| [sidebar_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/sidebar_ui_state.rs) | 766 | 文件树/搜索两组状态 + 196 行测试 |
| [sidebar_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/sidebar_view.rs) | 763 | 侧栏 4 个 tab（文件/大纲/lint/搜索）+行内重命名 |
| [symbol_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/symbol_ui_state.rs) | 694 | ~407 行静态符号数据表（EMOJI/SYMBOLS/SNIPPETS） |
| [settings_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/settings_view.rs) | 688 | 设置窗 5 个分页+调色板编辑+颜色转换 |
| [builtin.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/theme/builtin.rs) | 595 | `Theme` 大结构+Metrics+主题解析 |
| [toolbar_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/toolbar_view.rs) | 586 | 应用菜单+工具条控件+标签栏 |
| [editor_ops.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/editor_ops.rs) | 570 | 编辑文本操作纯函数 + 237 行测试 |
| [welcome_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/welcome_view.rs) | 568 | 欢迎页品牌区/主动作/最近文件/快捷键提示 |
| [export_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/export_ui_state.rs) | 566 | 导出格式选择枚举 + 218 行测试 |
| [editor_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/editor_view.rs) | 564 | 单个 `show()` 函数 324 行（L43-366）+拼写菜单+行号槽 |

#### 应用服务层 application/（3 个）

| 文件 | 行数 | 臃肿性质 |
|---|---|---|
| [markdown/service.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/application/markdown/service.rs) | 1030 | 3 个缓存结构（Render/Spell/Highlight）+9 个用例方法+5 个 DTO 映射 fn |
| [document/service.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/application/document/service.rs) | 919 | 生命周期/编辑/查询/草稿 4 组用例 + 统计/映射自由 fn（L573-916） |
| [dto.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/application/dto.rs) | 723 | 跨 4 个上下文的 30+ DTO 结构（表现层唯一可见数据契约） |

#### 领域层 domain/（4 个，用户重点）

| 文件 | 行数 | 类型 | 臃肿性质 |
|---|---|---|---|
| [preferences/value_objects.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/domain/preferences/value_objects.rs) | 1423 | 值对象 | 5 个内聚簇：主题(18-293)/编辑器设置(294-542)/按键模型(543-887)/ActionId(888-1112)/快捷键表(1113-1423) |
| [markdown/value_objects.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/domain/markdown/value_objects.rs) | 746 | 值对象 | block/table/inline/render/outline 5 组值对象 |
| [document/services.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/domain/document/services.rs) | 689 | **领域服务** | `EditingService` 30+ 纯函数横跨坐标/标题/行操作/组合输入 4 种业务能力 + TextBuffer/BufferFactory trait |
| [markdown/lint.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/domain/markdown/lint.rs) | 522 | 领域服务 | 4 条 lint 规则函数（含 127 行内联测试） |

> 聚合根专项核查：最大聚合根 [document/aggregate.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/domain/document/aggregate.rs) 为 **442 行（黄线）**，`Document` 聚合 + `impl` 单一内聚体，未超红线；workspace/preferences/export/markdown 聚合均 <240 行。结论：**本项目臃肿集中在值对象与领域服务，而非聚合根本身**；aggregate.rs 是否顺手拆为 follow-up 决策项（见 Open questions），本次默认不动。

#### 基础设施层 infrastructure/（6 个）

| 文件 | 行数 | 技术能力 |
|---|---|---|
| [parsing/pulldown_parser.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/parsing/pulldown_parser.rs) | 1418 | 选项/图片尺寸脱糖/OpenBlock 栈/事件映射/parse 主循环 |
| [persistence/encoding.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/persistence/encoding.rs) | 918 | UTF-8/UTF-16/GBK/GB18030 四套解码器+编码器+两张 `include_bytes!` 表 + 355 行测试 |
| [export/html_sanitize.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/export/html_sanitize.rs) | 562 | 白名单策略/HTML tokenizer/属性过滤渲染 |
| [export/system_pdf_exporter.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/export/system_pdf_exporter.rs) | 551 | 后端探测/浏览器调度/Exporter 实现 |
| [persistence/asset_loader.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/persistence/asset_loader.rs) | 547 | 限制策略/加载器/HTTP 分块+base64+百分号解码 |
| [export/viewport_capture.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/export/viewport_capture.rs) | 521 | 裁剪/PNG 写入/长图拼接 + 256 行测试 |

### 1.3 测试基线（实测于本工作区，2026-10-11）

`cargo test --all-targets --no-fail-fast`（debug，Windows 沙箱）：

| test target | 结果 |
|---|---|
| domain | 277 passed |
| application | 191 passed |
| infrastructure | 277 passed，**1 failed：`pdf_exporter_test::real_export_writes_a_real_pdf_to_the_chosen_path`**（无头 Edge 在沙箱不产出 PDF，环境限制，[docs/testing.md](file:///d:/data/WorkBuddyCache/workspace/rustmark/docs/testing.md) V11-15 已登记） |
| integration | 79 passed，**1 ignored：`golden_fixture::generate_large_fixture`** |
| presentation | 488 passed |
| app | 49 passed |
| shared | 143 passed |
| e2e / arch_violation_fixture / startup_smoke 等 | 全绿 |
| **合计** | **1572 passed / 1 failed / 1 ignored** |

注意：[docs/testing.md](file:///d:/data/WorkBuddyCache/workspace/rustmark/docs/testing.md) 记载的「7 failed」是 2026-010 旧快照，当前 main 仅剩上述 1 个环境型失败；**一律以本次实测为准**。

### 1.4 黄线清单（300–500 行，本次不动，登记 follow-up）

[status_bar_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/status_bar_view.rs)(499)、[workspace/service.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/application/workspace/service.rs)(487)、[document/aggregate.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/domain/document/aggregate.rs)(442，聚合根)、[status_bar_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/status_bar_ui_state.rs)(432)、[simple_checker.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/spell/simple_checker.rs)(400)、[command_palette_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/command_palette_view.rs)(398)、[fs_document_repository.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/persistence/fs_document_repository.rs)(393)、[syntect_highlighter.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/highlighting/syntect_highlighter.rs)(388)、[export_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/export_view.rs)(386)、[diff_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/diff_view.rs)(376)、[document/value_objects.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/domain/document/value_objects.rs)(369)、[pulldown_html_exporter.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/export/pulldown_html_exporter.rs)(361)、[quick_open_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/quick_open_ui_state.rs)(340)、[settings_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/settings_ui_state.rs)(340)、[preferences/service.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/application/preferences/service.rs)(333)、[dialog_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/dialog_ui_state.rs)(331)、[bootstrap.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/app/bootstrap.rs)(330)、[quick_open_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/quick_open_view.rs)(310)、[find_replace_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/find_replace_view.rs)(308)、[scroll_sync_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/scroll_sync_state.rs)(306)、[rope_text_buffer.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/text/rope_text_buffer.rs)(304)、[command_palette_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/command_palette_ui_state.rs)(301)。

---

## 2. RALPLAN-DR

### Principles

1. **零行为变更优先于结构美观**：搬迁不动字符；任何「顺手优化」的冲动都视为违规（baseline：外科手术式改动）。
2. **路径稳定即兼容稳定**：利用 Rust 2018「`foo.rs` + `foo/` 目录共存」facade 模式，所有公开路径保持不变；仓库已有此惯例（如 [document.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/domain/document.rs) + `document/`）。
3. **私有子模块单一入口**：子模块一律 `mod x;`（私有），外部只能经 facade re-export 访问，杜绝同一类型出现两条可达路径。
4. **叶子优先、风险递增**：纯数据值对象 → 应用服务 → 基础设施 → UI state → UI 视图 → 组合根；每波独立可交付、可回滚。
5. **最小代码**：不新增抽象、不新增 trait、不调整模块以外的任何文件；不为「以后更好拆」预埋结构。

### Decision drivers

1. 业务零退化是硬约束（用户要求 #2/#4），优先级 > 拆分后文件的美感。
2. 31 个文件、约 90 个新子文件，review 负担要求每个 diff 是「可肉眼验证的代码移动」。
3. 项目已有机器化分层门禁（[arch_check.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/tools/arch_check.rs)）与分层测试目标，可直接复用为逐提交护栏。
4. 桌面应用 v0.1.0、`publish = false`，无外部 crate 消费者；兼容面 = 仓库内路径 + 测试。

### Viable options

**Option A：facade 保活拆分（选定）** — 臃肿 `foo.rs` 变为同名 facade（`mod` 声明 + `pub use`/`pub(crate) use` 全量再导出）+ 私有子目录 `foo/*.rs`；结构体/trait 留在 facade，`impl` 块可分散到子文件（`impl super::Foo`），方法调用路径不变。

- 改动文件：仅被拆文件自身 + 其新建子文件；**调用方与测试零修改**。
- Pros：逐文件增量；每 commit 可独立 revert；分层门禁自动适用新文件；diff 纯移动易审。
- Cons：facade 成为 barrel 文件；可见性需收窄裁决（私有 → `pub(crate)`/`pub(super)`）；子文件间 `use super::` 样板增加。

**Option B：规范化迁移 + 全仓路径改写** — 把条目搬到「最终规范位置」，用机械改写更新全部 `use`（含 `tests/**`），不保留 facade。

- Pros：终态无 barrel。
- Cons：一个文件的拆分会同时 diff 几十个调用点与测试文件，移动与路径改写混在一起无法肉眼验证，review/ blame/ revert 全部恶化；违背「增量迭代、每文件一验证」。**Rejected**：收益（少一层 re-export）不抵风险，且与 decision driver #1/#2 冲突。

**Option C：只拆 Top N + region 注释** — 仅拆 frame.rs/mermaid.rs 等前 5 个，其余用 `// region` 分区。

- Cons：不满足 AC-1 与用户要求 #1/#3/#5（31 个红线文件仍在）。**Rejected**。

**invalidation rationale**：B 的「终态更干净」在本项目无实际消费者价值（`publish=false`、私有模块即可防路径分叉）；C 直接不满足范围。A 是唯一同时满足零路径破坏与增量验证的方案。

### 拆分总模式（所有文件统一手法）

```text
拆分前：                          拆分后：
src/.../foo.rs (900行)            src/.../foo.rs        ← facade：模块文档 + mod 声明 + re-export + 主体结构
                                  src/.../foo/a.rs      ← 私有子模块（mod a;，外部不可命名）
                                  src/.../foo/b.rs
```

固定规则：

1. 子模块一律私有声明（`mod a;`，**不写** `pub mod a;`）；需要保持原路径的条目在 facade `pub use a::X;`（原 `pub`）或 `pub(crate) use a::x;`（原私有但跨文件/测试需要）。
2. 跨子文件使用的私有条目升为 `pub(super)`（仅父模块树可见）或 `pub(crate)`；**只升可见性，不改签名/函数体**。
3. `impl Foo` 块整体搬迁到子文件，开头 `use super::Foo;`，块体一个字符不动；结构体定义留 facade。
4. `include_bytes!`/`include_str!` 随代码进子文件时相对路径加 `../`（如 encoding 子文件 → `include_bytes!("../gbk_table.bin")`）；`.bin`/`words.txt` 等数据文件**不移动**。
5. 内联 `#[cfg(test)] mod tests` 优先原样留 facade（`use super::*` 经 re-export 可见）；若导致 facade >500 行（mermaid/math/frame），外置为 `#[cfg(test)] mod tests;` → `foo/tests.rs`，测试文本零修改。
6. **自下而上提取**：先搬文件中最靠后的分组，保证先前行号在操作过程中不漂移。
7. 每个原始文件 = 1 个 commit；commit message 前缀 `refactor: split <path> by <维度> (no logic change)`。
8. 新文件首行保留/补 `//!` 模块说明（仓库 S2-02 约定）。

---

## 3. Implementation steps（逐文件方案，行号基于基线 `674ce65`）

> 每个 commit 完成后执行 §5.1 的「单文件四连验证」；每波结束执行 §5.2「波次门禁」。

### 波次 0 · 基线与护栏（无代码搬迁）

1. 从 `main`（clean）建分支：`git switch -c refactor/large-file-split`（worktree 可选，见 §4）。
2. 保存基线证据：`cargo test --all-targets --no-fail-fast` 全量输出存档（记录 1572/1/1 及两个用例名）；`git rev-parse HEAD` 记录基线 commit。
3. **试点**：选最小的 [lint.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/domain/markdown/lint.rs)（522 行）做首个拆分（即步骤 D4），用来验证三件事：①`pub(crate) use` 在本仓库 lint 配置下不触发 unused 告警；②内联测试留 facade 的可见性；③四连验证全套可行。试点失败则停下来修订本模式，不推广。

### 波次 1 · 领域层（4 commit，纯内存逻辑，领域覆盖门禁 90%）

**D1 — [preferences/value_objects.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/domain/preferences/value_objects.rs)（1423，按内聚属性/行为边界）** → `value_objects/` 五子文件：

- `theme.rs`：ThemeKind+impl(18-66)、CustomThemePalette+Default(67-156)、LintSettings+default_lint_on+impls(157-206)、Theme+`FONT_SIZE_RANGE`/`LINE_HEIGHT_RANGE`+impl+Default(207-293)
- `editor.rs`：TabSize(294-338)、WritingGoal(339-384)、default_show_line_numbers(385-390)、EditorSettings+impl+Default(391-542)
- `keys.rs`：Modifiers(543-615)、NamedKey(616-743)、Key(744-782)、KeyCombo+impl+Display+Serialize(783-887)
- `action.rs`：ActionId+impl(888-1112)
- `shortcut_table.rs`：ShortcutBinding(1113-1144)、ConflictKind(1145-1173)、Conflict(1174-1222)、ShortcutTable+`RESERVED_COMBOS`+impl+Default(1223-1423)
- facade：仅模块文档 + 5 个私有 `mod` + 全量 `pub use`。外部路径 `domain::preferences::value_objects::{ActionId, KeyCombo, ...}` 不变（[preferences_service_test.rs:15](file:///d:/data/WorkBuddyCache/workspace/rustmark/tests/application/preferences_service_test.rs#L15) 等直接钉住）。

**D2 — [markdown/value_objects.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/domain/markdown/value_objects.rs)（746）** → `value_objects/`：`block.rs`(BlockId 18-39、BlockKind+impl 319-464)、`table.rs`(CellAlignment 40-66、ListItem 67-149、DefinitionBody 150-178、DefinitionEntry 179-233、TableData 234-318)、`inline.rs`(InlineStyle 465-495、InlineSpan 496-538、SpanStyle 608-655、SyntaxSpan 656-679)、`render.rs`(RenderedBlock 539-607、Misspelling 735-746)、`outline.rs`(OutlineItem 680-734)。

**D3 — [document/services.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/domain/document/services.rs)（689，领域服务按业务能力）** → `services/`：facade 留 TextBuffer trait(22-75)、`EditingService` 单元结构(76-77)、BufferFactory trait(685-689)；

- `coordinates.rs`：char_floor/char_ceil/align_range/line_start/line_end/line_range/expand_to_full_lines(83-154)
- `selection.rs`：wrap_selection(155-204)
- `headings.rs`：heading_level_at/promote_heading/demote_heading/retarget_heading/atx_prefix_len(205-323)
- `lines.rs`：toggle_line_prefix/indent_lines/outdent_lines(324-384)、move_line_up/down/duplicate_line/delete_line(411-513)、transform_lines(604-629)、block_line_count(671-684)
- `compose.rs`：continue_list_on_newline(385-410)、toggle_task/insert_link/insert_code_block/insert_table(514-603)、list_marker_of/next_list_marker(630-670)

私有 helper（list_marker_of 等）升 `pub(super)`；`EditingService::*` 调用路径不变。

**D4 — [markdown/lint.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/domain/markdown/lint.rs)（522，试点）** → facade 留 LintRule(28-78)、LintRuleSet(79-117)、LintIssue(118-160)、MarkdownLinter::lint(161-201) + 测试(395-522)；`rules.rs`：fenced_lines/opens_fence/closes_fence(202-240)、heading_jump/atx_level(241-287)、list_marker_mixed/list_marker(288-341)、trailing_whitespace(342-367)、unclosed_fence(368-394)。

波次门禁：`cargo test --test domain`、`cargo test --test application`（应用层大量引用领域 VO）、覆盖率门禁（domain ≥90%）、全量基线比对。

### 波次 2 · 应用服务层（3 commit，按用例场景）

**A1 — [application/dto.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/application/dto.rs)（723）** → `dto/`：`document.rs`(RangeView/DocumentView/StatsView 17-110、OutlineItemView/OutlineView/SearchHitView 111-144)、`preferences.rs`(CustomThemePaletteView 145-272、ThemeView/EditorSettingsView/LintRulesView/LintIssueView/GoalProgressView/ShortcutView/SettingsView 273-458)、`markdown.rs`(CellAlignView…SpanView 459-608)、`workspace.rs`(ExportResultView…FormatResultView 609-723)。facade 全量 `pub use`（presentation 与所有测试经 `application::dto::*` 访问）。

**A2 — [markdown/service.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/application/markdown/service.rs)（1030）** → `service/`：facade 留常量 `TRUNCATE_THRESHOLD_BYTES`(43)/`DEFAULT_RENDER_CACHE_CAPACITY`(67)/`HIGHLIGHT_CACHE_CAPACITY`(458)、MarkdownAppService 结构(434-457)、new/with_spell_checker/spell_cache/cache/now/should_render/clear_cache(462-542)、lock_or_recover(76-86)、send-sync 断言(1028-1029)；

- `render_cache.rs`：DebouncePolicy(87-111)、CachedRender/RenderCache(112-173)
- `spell_cache.rs`：CachedSpell/SpellCache(174-268)
- `highlight_cache.rs`：HighlightCache/HighlightPace(269-433)
- `preview.rs`：render_preview(543-578)+fetch(815-821)+parse_or_cache(822-846)
- `highlight.rs`：highlight/highlight_cache_stats/highlight_snippet(579-648)
- `navigation.rs`：map_scroll(649-687)+rebuild_outline(688-715)
- `spell.rs`：spell_check(716-758)+spell_suggestions(796-814)
- `lint.rs`：lint_document(759-795)+lint_rule_set(49-66)
- `view_mapper.rs`：to_block_view/spans_to_view/inline_style_str/block_kind_str/span_style_str(847-1027)

缓存类型（SpellCache/RenderCache/HighlightCache…）在 facade `pub use`，保持 `application::markdown::service::SpellCache` 等路径（frame.rs 经 `spell_cache()` 使用，测试 [markdown_service_test.rs:13](file:///d:/data/WorkBuddyCache/workspace/rustmark/tests/application/markdown_service_test.rs#L13) 钉住）。

**A3 — [document/service.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/application/document/service.rs)（919）** → `service/`：facade 留结构(36-52)+new/provider(55-77)+send-sync 断言(917-918)；

- `lifecycle.rs`：open/encoding_of/note_encoding_after_first_write/new_document/save/save_to(78-223)、undo/redo(358-390)、close(391-421)、new_document_with_content(524-538)
- `editing.rs`：insert_text/replace_range/format/toggle_ordered_list/set_selection(224-357,422-435)+TABLE_ROWS/COLS(573-574)
- `queries.rs`：get_document/get_content/get_stats/get_selection/list_open(436-492)
- `drafts.rs`：save_draft/load_draft/clear_draft/pending_recoveries(493-523)
- `mapping.rs`：to_document_view(632-658)、apply_edit/shift_selection(581-620)、derive_save_as_path(904-916)
- `stats.rs`：compute_stats(659-724)、leading_list_number/is_atx_heading/fence_token_of/task_marker_of/count_links_and_images/link_tail_len/count_words/is_cjk(621-631,725-903)
- `support.rs`：lock_read/lock_write/drain_doc_events/publish_all(539-572)

`compute_stats`/`to_document_view` 为 `pub fn`，facade 原样 `pub use`（[document_service_test.rs:11](file:///d:/data/WorkBuddyCache/workspace/rustmark/tests/application/document_service_test.rs#L11) 钉住）。

波次门禁：`--test application`、`--test domain`、`--test integration`（跨层用例）、全量比对。

### 波次 3 · 基础设施层（6 commit，按技术能力）

**I1 — [persistence/encoding.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/persistence/encoding.rs)（918）** → `encoding/`：facade 留 Encoding+impls(41-91)、UTF8_BOM(92)、decode/decode_to_string/utf8_multibyte_counts(95-197)+测试(563-918)；`utf16.rs`(decode_utf16 198-222)、`gbk.rs`(decode_gbk/decode_gb18030_four_byte/lead_index/is_gbk_trail/GBK_TABLE/GBK_* 常量/gbk_lookup 223-374，路径改 `include_bytes!("../gbk_table.bin")`)、`gb18030.rs`(GB18030_RANGES 及其全部转换 fn 375-521，路径 `include_bytes!("../gb18030_ranges.bin")`)、`writer.rs`(encode 522-562)。

**I2 — [export/html_sanitize.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/export/html_sanitize.rs)（562）** → `html_sanitize/`：`policy.rs`(全部白名单常量+VOID_ELEMENTS/is_void_element/is_safe_url/is_safe_data_image 35-170)、`tokenizer.rs`(Attribute/Tokenizer/Token/unescape_html 171-391)、`writer.rs`(filter_attributes/render_attributes 392-462)；facade 留 sanitize_html(463-末) 并 `pub use policy::is_safe_url`（外部测试直接调用）。

**I3 — [parsing/pulldown_parser.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/parsing/pulldown_parser.rs)（1418）** → `pulldown_parser/`：facade 留 parser_options(49-72)、PulldownMarkdownParser+new/raw_parser(73-98)；`image_size.rs`(99-221)、`stack.rs`(OpenBlock 全部与栈操作 222-642)、`event_map.rs`(643-879)、`parse.rs`(`impl MarkdownParser for PulldownMarkdownParser` 880-1418 整块)。`desugar_image_sizes`/`parser_options` 等 pub 项 facade 再导出。

**I4 — [persistence/asset_loader.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/persistence/asset_loader.rs)（547）** → `asset_loader/`：`policy.rs`(AssetLimits/AssetError/LoadedAsset/mime_from_extension/alt_of 38-160)、`coding.rs`(split_http_body/decode_base64/percent_decode 462-547)；facade 留 AssetLoader+impl(161-461)。

**I5 — [export/system_pdf_exporter.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/export/system_pdf_exporter.rs)（551）** → `system_pdf_exporter/`：`backend.rs`(PdfBackend/CommandProbe/PathProbe/which/PrintOutcome 50-166)、`dispatch.rs`(PrintDispatcher/SystemPrintDispatcher/chromium_candidates/pick_browser/file_url/headless_print/shell_print_command/shell_print_to 167-366)；facade 留 SystemPdfExporter+impls+PATH_PROBE+`impl Exporter`+backend_hint(367-551)。

**I6 — [export/viewport_capture.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/infrastructure/export/viewport_capture.rs)（521）** → `viewport_capture/`：`image.rs`(crop/write_png/stitch 123-183,203-264)；facade 留 CaptureError/MAX_*常量/capture_region_to_png(39-122,184-202)+两个测试 mod(265-521)。

波次门禁：`--test infrastructure`（golden/编码/导出全覆盖）、`--test integration`（snapshot 字节不变）、全量比对。

### 波次 4 · 表现层 UI State（5 commit，headless 逻辑覆盖好）

**P1 — [state/symbol_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/symbol_ui_state.rs)（694）** → `symbol_ui_state/`：`catalog.rs`(SymbolGroup/SymbolItem/EMOJI 55-171/SYMBOLS 172-367/items_of 462-471)、`snippets.rs`(SNIPPETS 368-461)；facade 留 SymbolUiState+Default+impl+search(472-577)+测试(578-694)。

**P2 — [state/export_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/export_ui_state.rs)（566）** → `export_ui_state/choices.rs`(ExportFormatChoice 14-117、PageSizeChoice 118-148)；facade 留 ExportUiState+impl+测试(149-347,348-566)。

**P3 — [state/diff_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/diff_ui_state.rs)（807）** → `diff_ui_state/`：`model.rs`(MAX_CELLS+DiffOp/DiffRow/DiffStat/CharSegKind/CharSeg/DisplayRow 29-205)、`algorithm.rs`(exceeds_budget/diff_lines/diff_stat/diff_chars/to_display_rows 206-492)；facade 留 DiffUiState(493-553)+测试(554-807)。

**P4 — [state/sidebar_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/sidebar_ui_state.rs)（766）** → `sidebar_ui_state/`：`search.rs`(SearchOptions/SearchToggle/SEARCH_TOGGLES/SearchGroup/SearchHitRow/group_hits/locate_in_snippet 14-101,126-153,517-569)、`tree.rs`(TreeRow/NameEdit/tree_rows/is_under_root/depth_of/visible_ancestors 102-125,154-195,434-516)；facade 留 SidebarUiState 两个 impl(196-433)+测试(570-766)。

**P5 — [state/find_replace_ui_state.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/state/find_replace_ui_state.rs)（1156）** → `find_replace_ui_state/`：facade 留 FindOptions/PatternError/OptionKind/FIND_INPUT_ID/REPLACE_INPUT_ID/FindReplaceUiState(25-72,901-907,908-1156)；`pattern.rs`(Node/ClassItem/Regex+impl/compile/parse_alternatives/parse_sequence/read_class_atom/chars_eq/fold_lower 82-534)、`engine.rs`(Match/Finder/ReplaceSummary/matches_with/find_from/find_literal/find_literal_ignore_case/scan_literal_ignore_case_all/word 边界 535-900)。
**专项约束**：`find_literal_ignore_case`(767-817) 即 [large-file-report.md](file:///d:/data/WorkBuddyCache/workspace/rustmark/docs/archive/stage-reports/large-file-report.md) 登记的 DEF-01 O(N×M) 代码，**逐字符原样搬迁，禁止修算法、禁止"顺手优化"**；`large_*` 性能用例是行为门禁。

波次门禁：`--test presentation`、`--test app`（frame 使用这些 state）、全量比对。

### 波次 5 · 表现层视图/主题/快捷键（12 commit，UI 像素行为仅手工 QA 兜底）

- **V1 [welcome_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/welcome_view.rs)(568)** → `welcome_view/{brand,actions,recent,hints}.rs`（分段见扫描：brand 179-217+479-503；actions 70-133,218-301,504-519；recent 302-401,520-536；hints 402-478），facade 留 consts+show+测试。
- **V2 [settings_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/settings_view.rs)(688)** → `settings_view/{appearance,editor_prefs,shortcuts,about}.rs`（203-399 / 400-474 / 506-601 / 602-645），facade 留 show/body/nav/content/footer/field/action_of。
- **V3 [toolbar_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/toolbar_view.rs)(586)** → `toolbar_view/{menu_bar,controls}.rs`（94-246 / 261-541），facade 留 consts/show/draw_tab_strip/draw_breadcrumb。
- **V4 [sidebar_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/sidebar_view.rs)(763)** → `sidebar_view/{files,outline,lint,search}.rs`（139-457 / 458-539 / 540-603 / 604-763），facade 留 show/draw/tab_bar。
- **V5 [editor_ops.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/editor_ops.rs)(570)** → `editor_ops/{cursor,transform,line}.rs`（32-49+250-280 / TextEdit+diff_replace+transform_insert / move_line+list_marker），facade 仅留 237 行测试 + mod/re-export。
- **V6 [editor_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/editor_view.rs)(564)** → `editor_view/{spell,gutter}.rs`（spell_menu_for/draw_spell_menu 367-505；paint_gutter/current_line_index 506-564）；facade 留 **324 行 `show()` 函数整体不拆**（函数体内部提取 helper 属高风险动作，明确禁止；facade 约 390 行达标）。
- **V7 [preview_view.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/preview_view.rs)(1872)** → `preview_view/{inline,code,list,table,media,blocks,toc}.rs`（389-661 / 786-898 / 899-1047+1452-1575 / 1048-1312 / 1313-1362+1576-1773 / 662-785+1363-1380+1774-1824 / 1380-1451），facade 留图片缓存(49-168)+show/pane_header/draw_block(169-388)+测试。
- **V8 [math.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/math.rs)(2069)** → `math/{symbols,model,parser,layout,paint,tests}.rs`（符号表 187-394；model 103-186+974-1056；Parser 395-973；layout 1057-1657；paint 1684-1784；测试 1785-2069 经 `#[cfg(test)] mod tests;` 外置，文本零修改），facade 留 error/consts/Measure/layout 入口。
- **V9 [mermaid.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/views/mermaid.rs)(3057)** → `mermaid/{model,graph_parse,graph_layout,primitives,sequence,class,gantt,tests}.rs`（93-157+439-595 / 158-550 / 656-889 / 890-1614 / 1615-1989 / 1990-2331 / 2332-2601 / 2603-3057），facade 留 error/Measure/consts/layout 入口/MermaidStyle+paint+finalize。
- **V10 [theme/builtin.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/theme/builtin.rs)(595)** → `builtin/{kind,metrics,resolve}.rs`（37-84 / 85-183 / 472-595），facade 留 Theme 结构+impl+字体常量(184-470)。
- **V11 [theme/mapper.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/theme/mapper.rs)(1307)** → `mapper/{squiggle,highlight_cache,highlight_layout}.rs`（44-233 / 234-471 / 472-769），facade 留 StyleMapper+apply+测试(770-1307)。
- **V12 [shortcuts/registry.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/presentation/shortcuts/registry.rs)(1184)** → `registry/{action,keys,combo,palette}.rs`（48-646 / 647-827 / 828-931 / 1080-1184），facade 留 ShortcutRegistry+impl(932-1079) 并 re-export `UiAction/Key/Modifiers/NamedKey`（[keymap.rs:15](file:///d:/data/WorkBuddyCache/workspace/rustmark/tests/support/keymap.rs#L15)、[frame_test.rs:10](file:///d:/data/WorkBuddyCache/workspace/rustmark/tests/app/frame_test.rs#L10) 钉住）。注意：按键模型与 domain 层同名类型是刻意重复（presentation 禁依赖 domain），搬迁时不得合并。

波次门禁：`--test presentation`（488 条全绿）、`--test app`、`--test e2e`、手工 QA（§5.4）、全量比对。

### 波次 6 · 组合根 frame.rs（1 commit，最后做）

**F1 — [app/frame.rs](file:///d:/data/WorkBuddyCache/workspace/rustmark/src/app/frame.rs)(3607)** → `frame/`：facade 留 `EditorApp` 结构(163-300)、new/theme/dispatcher(465-535)、`impl eframe::App`(3453-3503)、run/install_fonts(3504-3563)；测试外置 `frame/tests.rs`(3564-3607)。13 个私有子文件按 impl 簇整体搬迁：

| 子文件 | 搬迁方法（行号） |
|---|---|
| `dialog.rs` | PendingDialog(84-106)；request_dialog/dialog_busy/apply_dialog_outcome/apply_delete_confirm/display_name/report_dialog_failure(536-658) |
| `capture.rs` | PendingCapture/LongImageState/MAX_LONG_IMAGE_SEGMENTS/compute_long_image_skip(359-443)；save_document_to/export_current_document/preview_rect/begin_*/deliver_capture/pump_capture/take_screenshot_event(659-1043,3368-3377) |
| `workspace.rs` | WorkspaceEntries/WorkspaceSearch(107-162)；active_document_dir/title_stem/suggested_save_name(1044-1112)；workspace_root…refresh_workspace(1126-1306)；parent_of/group_search_hits(3378-3416) |
| `outline.rs` | current_outline/lint_current_document/goto_outline_line/goto_byte_offset/replace_spelling/search_workspace(1307-1422) |
| `tabs.rs` | editor_id…close_all_documents(1423-1655)；cycle_document/goto_document/persist_split_ratio(2150-2223) |
| `files.rs` | run_save/run_new_document/set_status_*/current_document_path/theme_kind_label/insert_at_cursor/read_workspace_file/new_document/save_document/run_autosave(1656-1872)；current_document_dirty/kind/recent_files/open_path/handle_dropped_files(1949-2083) |
| `recovery.rs` | render_recovery_prompt/recover_all_drafts/discard_all_drafts/pending_recoveries(1873-1948) |
| `editor.rs` | IME 常量(444-459)；cursor_line_col…current_stats(2084-2149)；flush_edit…outdent_selection(2224-2382)；apply_format/run_format/move_line(2773-2922)；apply_pending_editor_state(2923-3069) |
| `find.rs` | sync_find…replace_match(2383-2531) |
| `settings.rs` | settings_snapshot…reload_shortcut_registry(2532-2772)；theme_view/refresh_system_theme/cycle_theme(3070-3129) |
| `actions.rs` | open_overlay/execute/collect_actions(3130-3354)；domain_action/openable_overlay(3355-3367,3417-3426) |
| `drag_drop.rs` | classify_dropped/is_openable_text(3427-3452) |

专项约束：①**只搬整方法/整 impl 块，不提取新 helper、不调整 update() 内调用顺序**；②遇 borrow checker 报错，处理手段仅限「把整个方法退回 facade」，禁止改写函数体；③`app/` 在 arch_check 中无层级禁令（[arch_check.rs:116-130](file:///d:/data/WorkBuddyCache/workspace/rustmark/tools/arch_check.rs#L116-L130)），但仍须跑 check-arch。

波次门禁：`--test app`(49)、`--test integration`、`--test e2e`、`--test startup_smoke`、release 构建、手工 QA 全清单、全量比对。

### 波次 7 · 收口

1. 全量行数复核（§5.5），确认 31 文件 facade+子文件全部 ≤500。
2. 覆盖率门禁（§5.3）；release 体积门禁（§5.3）。
3. 逐波 PR 合入 main（或整分支 squash 保留 31 commits 历史——见 Open questions）。
4. 更新 [docs/testing.md](file:///d:/data/WorkBuddyCache/workspace/rustmark/docs/testing.md) 基线数字（1572/1/1）——这是唯一允许修改的 docs 文件，且只改数字。

---

## 4. Workspace setup

- 已核实：当前分支 `main` @ `674ce65`，`git status --short` 干净。
- 本 plan 会新增 ~90 个源文件、修改 31 个既有文件、零逻辑变更——**实施前必须切分支**（main 上禁止直接动工）：
  - 推荐：`git switch -c refactor/large-file-split`（单 worktree 即可）。
  - 若需与日常开发并行：`git worktree add -b refactor/large-file-split ../rustmark-large-file-split`。
- 每个原始文件一个 commit，不合并不同文件的搬迁；不夹带任何邻近代码清理。
- **本 artifact 是 plan，不含代码改动；开工需用户确认。**

---

## 5. Verification steps（回归测试验证标准）

### 5.1 单文件四连验证（每个 commit 后，约 10-60 秒）

```powershell
cargo fmt --check                              # 不符则先 cargo fmt（同 commit）
cargo run --bin check-arch                     # 分层边界，必须 exit 0
cargo clippy --all-targets -- -D warnings      # 与 CI 同款（CI 设 RUSTFLAGS="-D warnings"）
cargo test --test <所属层>                     # domain/application/infrastructure/presentation/app
```

额外按文件追加目标：dto.rs → `--test application --test presentation --test app`；markdown/document service → 加 `--test integration`；encoding/parser → `--test integration`（golden）；frame.rs → `--test app --test integration --test e2e --test startup_smoke`；registry/mapper/views → `--test presentation --test app`。

### 5.2 波次门禁（每波结束）

`cargo test --all-targets --no-fail-fast` → 逐 target 与 §1.3 基线比对：passed/failed/ignored 计数与两个具名用例完全一致；`git diff main...HEAD --stat` 人工确认无 `tests/data/golden/`、无 `.expected` 改动。

### 5.3 全局门禁（波次 7 / 合入前）

```powershell
cargo test --all-targets --no-fail-fast                              # 1572/1/1 具名一致
cargo llvm-cov --all-targets --lcov --output-path lcov.info -- ; python tools/coverage_gate.py lcov.info
cargo build --release                                                # 产物 <10MB（CI size gate 同款）
cargo test --release --test integration -- --test-threads=1 measure large   # 与 CI perf job 同款
```

### 5.4 手工 QA（波次 5、波次 6 后各一次）

按 [manual-qa-windows.md](file:///d:/data/WorkBuddyCache/workspace/rustmark/docs/manual-qa-windows.md) 执行：启动/建文档/编辑保存/预览（代码块、表格、数学、mermaid、图片）/查找替换/设置改主题与快捷键/导出 HTML 与 PDF/侧栏四 tab/拖拽打开/崩溃恢复。重点目测 mermaid/math/preview/主题（headless 测试无像素断言，这是唯一防线）。

### 5.5 行数复核命令（波次 7）

```powershell
Get-ChildItem -Path src -Recurse -Filter *.rs | ForEach-Object {
  $n = ([System.IO.File]::ReadAllText($_.FullName) -split "`n").Count
  if ($n -gt 500) { "{0,5}  {1}" -f $n, $_.FullName.Replace((Get-Location).Path + '\','') }
}
# 期望输出：空
```

---

## 6. Risks & mitigations

| # | 风险 | 触发信号 | 缓解 |
|---|---|---|---|
| R1 | 搬迁中误改逻辑（分支/顺序/错误串） | diff 出现非移动性改动；测试计数变化 | 自下而上整段剪切；每文件 1 commit；review 用 `git diff -w -M` 侧重非移动行；四连验证 |
| R2 | 可见性提升（私有→pub(crate)）扩大耦合面 | clippy dead_code/暴露警告 | 优先 `pub(super)`；子模块私有（外部不可命名子路径）；仅在需要处提升，不整文件 pub(crate) |
| R3 | facade re-export 遗漏导致路径断裂 | 编译错误 E0432/E0603 | 编译器逐个点名，按原可见性补 `pub use`；现有深路径测试即是路径钉（如 `service::compute_stats`、`registry::NamedKey`） |
| R4 | `pub(crate) use` 触发 unused 类告警致 `-D warnings` 红 | 试点 commit clippy | 波次 0 试点先验证；若告警，仅对 facade re-export 项保留（外部经测试使用即不告警），不使用的改私有路径引用 |
| R5 | `include_bytes!` 相对路径错误导致编码表错乱 | infrastructure 编码测试失败（GBK/GB18030 全量覆盖） | 路径加 `../`、数据文件不移动；编码 golden 用例为硬门禁 |
| R6 | DEF-01 查找算法被误"优化"，性能/行为漂移 | large_* release 用例变化；diff 触达 find 函数体 | P5 专项约束：函数体逐字符不动；release perf 门禁 |
| R7 | frame.rs borrow checker 诱发"顺手改写" | 搬迁中开始编辑函数体 | 硬规则：报错只整方法退回 facade；frame 放最后独立 PR；`impl eframe::App`/update 顺序不动 |
| R8 | mermaid/math/preview 像素级退化无 headless 断言 | 手工 QA 才发现 | 波次 5 后强制手工 QA；纯函数布局有大量内联单测随迁随跑 |
| R9 | 长分支与 main 漂移冲突 | 波次间 main 前进 | 叶子优先、每波独立可合入；每波结束 rebase main；冲突只可能在 facade mod 列表 |
| R10 | 基线既有失败（PDF 环境型）掩盖新失败 | 出现 >1 失败或失败名变化 | 按**具名用例**比对而非计数；PR 模板写明基线 1572/1/1 及失败名 |
| R11 | 内联测试外置（mod tests;）后 `use super::*` 可见性变化 | 测试编译失败 | tests.rs 中子项经 facade re-export 可见；不改测试文本；试点验证 |

---

## 7. Pre-mortem（deliberate）

1. **Scenario：编码重构后 GBK 文件打开乱码但测试全绿**（若有人同时移动了 `.bin` 或改错索引）。
   **Trigger**：波次 3 合入后用户反馈 GBK 文件乱码；`encoding.rs` 子文件 include 路径指向错误但碰巧编译通过（文件存在、表错位不可能——实际编译期即失败；更现实的 trigger 是误改 decode 分支顺序）。
   **Mitigation**：R5——`.bin` 不移动、只改路径字符串；infrastructure 目标含 GBK/GB18030 全量用例；AC-2 禁改分支；手工 QA 含 GBK 文件打开（[manual-qa](file:///d:/data/WorkBuddyCache/workspace/rustmark/docs/manual-qa-windows.md)）。

2. **Scenario：frame.rs 拆分后启动即崩/事件丢失（update 路由顺序被扰动）**。
   **Trigger**：波次 6 后 startup_smoke 红，或手工 QA 中快捷键/保存/对话框失灵。
   **Mitigation**：R7——只搬整方法、`impl eframe::App` 与 `execute/collect_actions` 保持在 facade/actions 簇整块移动；startup_smoke + e2e + app 49 条 + 手工 QA 四重门禁；单 commit 可直接 `git revert` 波次 6。

3. **Scenario：31 个 commit 进行到第 N 个时发现拆分模式有系统性问题（如 R4 告警普遍化）**。
   **Trigger**：波次 1 内连续 2 个文件需要非常规处理。
   **Mitigation**：波次 0 试点先行；模式修订后只重跑未完成文件，已合入波次独立成立（每个波次结束即全绿可交付，不依赖后续波次）；不允许在同一 commit 里换模式。

4. **Scenario：评审人无法在 3000 行移动 diff 中确认"零逻辑变更"**。
   **Trigger**：mermaid/frame PR review 卡住。
   **Mitigation**：diff 策略 `git diff -M --color-moved=zebra`（Git 把移动块着色为移动而非增删）；每子文件独立查看；CI 全绿 + golden 不变 + 手工 QA 作为客观补偿；frame 可按波次 6 子簇拆成 draft PR 内多 commit，但每个 commit 必须独立四连绿（若中间态无法满足，则维持单 commit）。

5. **Scenario：重构期间线上发现紧急缺陷需在 main 抢修**。
   **Trigger**：main 需要 hotfix 而分支进行中。
   **Mitigation**：分支不碰任何逻辑，hotfix 可直接在 main 发布，不受重构影响；回合冲突仅可能在 facade 的 `mod` 声明区；每波独立合入使分支寿命可控（预计 7 波 × 0.5-1 天）。

---

## 8. Expanded test plan

- **Unit**：每层独立 test target 随波次执行（domain 277 / application 191 / infrastructure 277 / presentation 488 / app 49 / shared 143）；内联测试原样保留并随构建执行；不新增、不删除、不修改测试。
- **Integration**：`tests/integration/`（alignment/crash_recovery/drag_open/large_file/preview_debounce/regression，79 条 +1 ignored）每波末跑；golden 快照（`tests/data/golden/`）与 snapshot 用例零改动是搬迁正确性的最强证明。
- **E2E / 启动**：`tests/e2e/` headless 帧循环 + `tests/startup_smoke.rs` 在波次 4/6 及合入前跑（守"能编译能过测试但启动即崩"）。
- **Observability / 运行期证据**：release `measure_*`/`large_*` 指标与重构前 `[metrics]` 输出同量级（CI perf job 同款命令）；产物 <10MB；手工 QA 录屏/清单勾选存档；崩溃恢复日志路径行为不变（crash_recovery 用例）。

---

## 9. 回滚预案（分级）

| 级别 | 情形 | 动作 |
|---|---|---|
| L1 提交内 | 四连验证任一不绿且当场无法用"补 re-export/调可见性"解决 | `git restore --staged --worktree .` 丢弃该文件搬迁，回到上一绿 commit；不带病前进 |
| L2 单文件 | commit 后波次内发现问题 | `git revert <commit>`（每文件独立 commit，revert 不影响其他文件） |
| L3 波次 | 波次门禁发现跨文件问题 | 该波次整体 revert（波次内 commits 连续）；已合入的前序波次保持有效 |
| L4 已合入 | main 上发现重构引入问题 | 按 PR 整体 revert；因零行为/零数据/零 API 变更，revert 无附带迁移，可立即发布 |
| L5 紧急抢修 | 重构进行中线上出 P0 | 直接在 main 修；重构分支 rebase 续做（冲突仅限 mod 声明区） |

每级回滚后都重跑对应门禁确认恢复；禁止"回滚一半 + 手工修补"的混合状态。

---

## 10. ADR

- **Decision**：采用 facade 保活的文件→私有子目录拆分（Option A），31 个 >500 行文件按 7 波次增量搬迁，facade `pub use` 保持全部既有路径，结构体/trait 留 facade、impl 块按业务/技术内聚簇分散到私有子文件。
- **Drivers**：业务零退化（决定性）> review 可验证性 > 终态结构纯洁性；仓库已有的 Rust 2018 facade 惯例、分层门禁与分层测试目标使方案零新增工具。
- **Alternatives considered**：A chosen；B（全仓路径改写）rejected——移动与改写混合不可审、不可增量回滚；C（只拆 Top N）rejected——不满足范围。
- **Why chosen**：唯一能同时满足「测试源码零修改」「每文件独立验证与回滚」「diff 纯移动可肉眼审计」的方案；私有子模块 + facade re-export 不产生第二条可达路径，终态可维护性不逊于 B。
- **Consequences**：①31 个 facade barrel 文件与 ~90 个子文件成为新结构，新增条目须先入子文件再按需 re-export（需写入 CONTRIBUTING 约定——列为 follow-up，不本次做）；②跨子文件私有项升 `pub(super)`/`pub(crate)`，crate 内可见面略增但不逃出 crate（`publish=false`、子模块私有）；③ blame 在拆分 commit 处断层一次，`git log --follow`/`git blame -M` 可跨越；④不产生任何运行时/性能/体积变化（纯模块拓扑调整）。
- **Follow-ups（本次明确不做）**：23 个黄线文件（含 442 行的 `Document` 聚合根）的拆分评估；DEF-01 算法修复（归属 S6-08，与本重构解耦）；facade 维护约定写入 [CONTRIBUTING.md](file:///d:/data/WorkBuddyCache/workspace/rustmark/CONTRIBUTING.md)；editor_view `show()` 324 行函数的函数级拆分（需行为护栏充分后单独立项）；testing.md 基线数字更新已列入波次 7。

---

## 11. Review trail

- **Planner draft v1**：提出 facade 拆分，初版子模块拟用 `pub(crate) mod`；31 文件按 7 波叶子优先排序；行号引用覆盖全部红线文件。
- **Architect challenge v1**：steelman 成立——若子模块公开，同一类型将出现 facade/子路径两条可达路径，未来导入会绕过 facade 使耦合重新发散；另指出 frame 单 commit ~2900 行 impl 搬迁 review 粒度过粗、每文件一 commit 在中间态可能与 clippy `-D warnings` 冲突。**采纳的综合修正**：子模块一律私有 `mod`（外部只能经 facade）；测试外置采用 `#[cfg(test)] mod tests;`；frame 坚持单 commit 但给出 `-M --color-moved` review 法与"borrow 报错只整方法退回"硬规则；波次 0 增加试点验证 re-export 告警行为。
- **Critic verdict v1：APPROVED with 5 reservations**：
  1. frame.rs 单 commit 回滚粒度粗，且 2900 行搬迁最易诱发顺手改写——靠"只退不改"硬规则+末位排序+四重门禁补偿，仍保留意见；
  2. `pub(crate) use` 的告警行为在首次真实编译前是推断而非证据——已降为波次 0 试点硬门槛，通过后才推广；
  3. 文档基线（7 failed）与实测基线（1 failed）不一致，已以实测具名用例为准并要求 PR 描述写明；
  4. mermaid/math/preview 像素行为无自动化断言，手工 QA 是唯一防线，不得跳过波次 5 QA；
  5. 31 commit 全部四连验证在 Windows 本机预计耗时较长（clippy 全量），若执行中发现单轮 >10 分钟，允许 clippy 仅在波次门禁跑全量、单文件跑 `cargo check --all-targets` 加速，但 check-arch 与分层 test 不得省——此让步需在第一个波次前由用户确认。
- **Final iterations**：1 / 3（Architect 修正已在落稿前合入；Critic 保留意见均已有对应缓解，无需第 2 轮）。

## Open questions（实施前请用户拍板）

1. 黄线的 `document/aggregate.rs`（442 行，唯一接近红线的聚合根）是否纳入本次？默认**不纳入**（follow-up）。
2. 合入方式：每波一个 PR（7 个 PR，推荐）还是 31 commits 单 PR？
3. Critic 保留意见 5 的验证提速让步是否预先批准？
