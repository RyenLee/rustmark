# 原型行为契约与缺陷免疫清单
>
>
> **已归档（2026-10-14）**：历史一次性文档（阶段报告 / 修复方案 / 实施日志 / 审计快照），结论仍有效但不再维护；归档索引与理由见 `docs/archive/README.md`。

> 来源：`../markdown-editor-prototype/`（Vue 3 Web 高保真原型，**只读**）
> 依据：ADR-005（原型 = 行为契约来源 + 缺陷疫苗库，非代码来源）
> 关联任务：S1-06（本文件）、S7-02（对齐矩阵）、S7-03（回归验证）

---

## A. 行为契约（Rust 侧必须对齐的可观测行为）

> 标注 ⚠ 的条目需在 S7-02 与原型实现核对后确认具体数值/算法。

### A-1 编辑行为

| # | 契约 | 原型出处 | Rust 实施点 |
|---|---|---|---|
| A-01 | 语法高亮 + 行号栏 + 当前行高亮 | `EditorPane.vue` | S6-03 / S6-04 |
| A-02 | `Tab` / `Shift+Tab` 缩进与反缩进，宽度取 `tabSize` 设置 | `EditorPane.vue:209/211` | S3-05（参数化，根治 2.2） |
| A-03 | `Alt+↑` / `Alt+↓` 移动当前行 | `EditorPane.vue` | S3-05 |
| A-04 | `Enter` 自动延续列表标记（有序/无序/任务） | `EditorPane.vue` | S3-05 |
| A-05 | 括号自动配对 | `EditorPane.vue` | S3-05 |
| A-06 | 复制/删除当前行 | `EditorPane.vue` | S3-05 |
| A-07 | 撤销合并窗口 400ms（同一文档内连续输入合并） | `stores/editor.ts` | S3-08（按文档隔离）⚠ 值待校准 |
| A-08 | 撤销/重做**恢复选区** | 原型 1.2 修复项 | S3-08 |
| A-09 | 自动换行开关 | `SettingsDialog.vue` | S6-11 |

### A-2 预览与渲染

| # | 契约 | 原型出处 | Rust 实施点 |
|---|---|---|---|
| A-10 | 预览防抖 150ms | `PreviewPane.vue` | S4-06（应用层可测） |
| A-11 | 代码块语言标签 + 复制按钮 | `PreviewPane.vue`、`CodeDialog.vue`（20 种语言） | S6-05、S5-03 |
| A-12 | 任务列表点击回写文档内容 | `PreviewPane.vue` | S6-05 |
| A-13 | 图片三种来源可见（本地/远程/data URI） | `useImageInsert.ts`、`sanitize.ts` | S5-11、ADR-003 |
| A-14 | 脚注：`[^id]` 引用 + 文末自动生成脚注区 | `lib/markdown.ts` `processFootnotes` | S5-02（GFM 特性核对） |
| A-15 | 渲染失败给出**可辨识**的提示（不得被清洗剥色） | `lib/markdown.ts:10` | S6-05（根治 2.5） |

### A-3 数值口径与默认值

| # | 契约 | 原型出处 | Rust 实施点 |
|---|---|---|---|
| A-16 | 统计字段：字数/字符数（含空格、不含空格）/行数/段落/标题/代码块/链接/图片/任务完成度/预计阅读时长 | `StatsDialog.vue` | S4-01 `StatsView` ⚠ 算法待核对 |
| A-17 | 搜索命中上限：单文件 50、总量 300 | `lib/search.ts` | S4-07 |
| A-18 | 最近文件上限 20 | `stores/workspace.ts` | S3-11 |
| A-19 | 图片内联体积上限（原型 512KB/张，仅 warn） | `lib/assets.ts:2` | ADR-003 改为**硬约束** |
| A-20 | 写作目标进度（0 表示不启用），达标提示 | `stores/settings.ts`、`StatusBar.vue` | S6-11（P2，视工期） |

### A-4 交互与无障碍

| # | 契约 | 原型出处 | Rust 实施点 |
|---|---|---|---|
| A-21 | 多标签：脏标记、中键/右键关闭、关闭其他/全部、全部保存 | `EditorTabs.vue` | S6-02、S4-07 |
| A-22 | 起始页：三个主操作 + 最近文件 + 快捷键提示 | `WelcomeView.vue` | S6-11 |
| A-23 | 状态栏：类型/编码/行列/字数（点击开统计）/保存状态/拼写/主题，均可点击 | `StatusBar.vue` | S6-11 |
| A-24 | 命令面板 `Ctrl+Shift+P` 模糊检索全部命令 | `CommandPalette.vue`、`lib/commands.ts` | S6-12 ✅ |
| A-25 | 快速打开 `Ctrl+O`（`dialog` + `combobox` + `aria-activedescendant`） | `QuickOpen.vue` | S6-11f |
| A-26 | 符号面板 `Ctrl+Shift+E`，网格 `roving tabindex` | `SymbolPicker.vue`、`lib/a11y.ts` | S6-11f |
| A-27 | 专注模式：隐藏全部外围 UI，`Esc` 退出，浮动退出按钮 | `WorkbenchView.vue` | S6-09（功能键可绑定） |
| A-28 | 对话框：打开时焦点落入、Tab 循环、关闭后焦点归还、背景 `aria-hidden` | `BaseModal.vue`、`lib/a11y.ts` | S6-12（空转焦点归还机制已移除，见注） |
| A-29 | 快捷键一览弹窗与起始页显示**跟随自定义** | `WorkbenchView.vue:308`、`WelcomeView.vue:69` | S6-09（根治 1.1-C / 5.5） |

> **A-25 / A-26 的实施点已从 S6-12 调整到 S6-11f**（S6-11f 实施期间确认）。
> 原因：这两个浮层与统计 / 快捷键一览 / 关于 / 差异对比同属
> `overlays::PENDING_VIEWS` 消减批次，拆到 S6-12 会让
> 「六个待办浮层」在 S6-11f 结束时变成四个、S6-12 再变两个，
> 反而让待办状态更难核对。
>
> **A-24 已于 S6-12 落地**：`PENDING_VIEWS` 现为**空列表**，
> 全部九个浮层都有渲染实现。
>
> **A-28（焦点归还）状态更正**：S6-12 留下的 `DialogUiState::focus_return_id` /
> `set_focus_return` / `take_focus_return` 以及 `frame.rs::overlay_focus_id` 是一套
> **空转的半套机制**——写死的 `"main_view.overlay_focus_sink"` 控件标识从未在
> 任何视图里注册，`take_focus_return` 也从未被调用。阶段 06 收尾已整体移除，
> 行为与移除前完全一致（原本就不生效），不引入任何回归。A-28 其余项
> （打开时焦点落入、Tab 循环、背景 `aria-hidden`）由 egui 默认焦点管理覆盖。
>
> **阶段 06 收尾另修两处缺陷**：
> 1. **`UiAction::FindInFiles`（`Ctrl+Shift+F` 全局「在文件中查找」）曾静默失效**——
>    动作表里有、快捷键能触发，但 `execute` 无任何分支，落入 `_ => {}`。已接线为
>    `toggle_sidebar_tab(SidebarTab::Search)`，与 `ToggleOutline` 同一思路。
> 2. **编辑区查找命中无高亮**——`Theme::find_bg` / `find_current_bg` 此前全项目零使用。
>    现 `highlight_layout` 增加 `highlight_layout_with_find` 变体，按文档字节区间
>    在逐行 `Galley` 上叠加命中背景（当前命中更醒目），命中集并入缓存键
>    （`find_sig`）保证旧高亮不残留；编辑区 `editor_view` 从 `FindReplaceUiState`
>    取命中区间传入。
>
> **A-25 的一处偏差（已知且刻意）**：原型用 `aria-activedescendant`
> 让读屏软件跟随组合框当前项；egui 无此语义，本实现改为
> 「选中项可见 + 上下键移动 + `Enter` 确认」，视觉行为等价，
> 读屏提示另计。
>
> **A-24 的三处刻意偏差**（S6-12 实施期间确认，详见 `docs/06`）：
>
> 1. **不移植 `disabled`**（无脏文档时「全部保存」灰显）——与现有菜单
>    行为一致。菜单无 `disabled`，面板单独搞一套会造成「同一动作
>    菜单能点、面板不能点」的新不一致。
> 2. **不收录原型 `Alt+1..9` 标签跳转**——动作表里没有这 9 个动作，
>    补它们是新功能，超出 S6-12 范围。
> 3. **分组名不参与模糊匹配**——原型搜 `label + keywords + group`，
>    搜「格式」会命中一堆无关条目。

---

## B. 缺陷免疫清单（原型已踩的坑 → Rust 侧设计层根治）

> 原则：**从设计上消除，而不是打补丁**。每条给出根治手段与验收任务。

### B-1 P0 功能失效（8 项）

| # | 原型缺陷 | 证据 | Rust 根治手段 | 验收任务 |
|---|---|---|---|---|
| B-01 | `F8` 永不触发（匹配器拒绝纯单键） | `useHotkeys.ts:16` | 匹配层不拒绝功能键；冲突判定交给领域层 `Conflicts::detect` | S3-12、S6-09 |
| B-02 | `Ctrl+Shift+S` 声明但未注册 | `mock/shortcuts.ts:25`、`WorkbenchView.vue:148-185` | 单一 `Vec<ShortcutBinding>` 同时驱动注册与显示，注册遗漏在编译期不可发生 | S6-09 |
| B-03 | `view.escape` 可录制但硬编码绕过 | `WorkbenchView.vue:186-202`、`SettingsDialog.vue:54` | 无硬编码分支；`IGNORE_KEYS` 不含 `Escape`/功能键 | S6-09 |
| B-04 | 无冲突检测，先注册者静默吞掉后者 | `SettingsDialog.vue:55-63`、`useHotkeys.ts:37-45` | `SetShortcutCommand` 先调 `Conflicts::detect`，冲突返回 `Err` | S3-12、S4-08、S6-09 |
| B-05 | 撤销栈跨文件污染（A 的内容写入 B） | `editor.ts:50-52/144-152` | 撤销栈是 `Document` 的成员值对象，类型层面无全局栈 | S3-08（用例 1） |
| B-06 | `lastCommit` 全局，切换文件后合并窗口未重置 | `editor.ts:144-161` | 合并窗口状态 per-Document | S3-08（用例 2） |
| B-07 | `redo` 不恢复选区；`undo` 传同一值导致光标折叠 | `editor.ts:144-161` | 快照存 `{content, selection}`，undo/redo 均恢复 | S3-08（用例 3、4） |
| B-08 | 「快捷键一览」与起始页显示不跟随自定义 | `WorkbenchView.vue:308`、`WelcomeView.vue:69` | 三处显示统一取 `ShortcutView` | S6-09（用例 5） |

### B-2 安全与正确性（2 项已修于原型，转为 Rust 验收标准）

| # | 原型缺陷 | 证据 | Rust 根治手段 | 验收任务 |
|---|---|---|---|---|
| B-09 | 未知标签拆壳后子节点跳过清洗 → `<script>` / `on*` 逃逸 | `sanitize.ts:50-58`（修复前） | 清洗器对搬移节点递归清洗；`svg`/`math`/`video`/`audio`/`source` 进 `DROP_TAGS` 直接丢弃 | S5-09（8 条用例） |
| B-10 | `safeUrl` 未放行 `data:image/*` → 粘贴的图在预览/导出/打印全不可见 | `sanitize.ts:25-26`（修复前） | 精确白名单 `data:image/(png\|jpeg\|jpg\|gif\|webp\|bmp);base64,`；`data:text/html` 拒绝 | S5-09、S5-11 |

### B-3 逻辑缺陷（5 项）

| # | 原型缺陷 | 证据 | Rust 根治手段 | 验收任务 |
|---|---|---|---|---|
| B-11 | 持久化失败静默（配额溢出丢草稿且零提示） | `editor.ts:37-43/169/202` | 保存失败返回 `Err`、**不清除脏标记**、UI 提示；图片体积硬约束 | S4-04、S5-04、ADR-003 |
| B-12 | `tab-size` 硬编码 4，与 `tabSize` 设置脱节 | `EditorPane.vue:423-432` | 缩进宽度参数化；测量与渲染同源 | S3-05 |
| B-13 | 滚动时全文 split + 强制同步重排 | `EditorPane.vue:67-80` | `Rope` 按行索引 O(log n)；行偏移缓存；不在滚动路径做布局读写 | S5-01、S6-04 |
| B-14 | 导出 TOC 未转义且拼接在清洗之后 | `ExportDialog.vue:87-92`（修复前） | 顺序固定：先拼 TOC（转义）→ 再整体清洗 | S5-09 |
| B-15 | 渲染失败提示的 `style` 被自身清洗剥除 | `markdown.ts:10` | 失败提示用类名/文本表达，不依赖内联 `style` | S6-05 |

### B-4 工程化教训（2 项，转为流程约束）

| # | 原型教训 | Rust 侧约束 | 验收任务 |
|---|---|---|---|
| B-16 | 测试覆盖空洞：sanitize / undo / 快捷键 `match()` 覆盖为 0，导致三个真实缺陷溜进产物 | 每任务单元强制「主路径 + 错误路径 + 边界」三类用例；S5-09 直接以原型 8 条用例为验收 | S3-14、S5-09、S9-02 |
| B-17 | README 声称已实现但实际失效（7 处文档与实现不符） | 文档状态由阶段 07 对齐矩阵驱动，不手写；S9-07 抽查 10 项 | S7-02、S9-07 |

---

## C. 使用方式

- 阶段 03/05/06 实施时：按「验收任务」列把对应用例写进该任务单元的验收标准；
- 阶段 07：S7-03 逐条验证 B 清单 17 项，其中 ≥6 项转为自动化断言（B-05/06/07 撤销 3 条、B-01/04 快捷键 2 条、B-09 sanitize 1 条）；
- ⚠ 标记项在 S7-02 核对原型实现后回填本文件。
