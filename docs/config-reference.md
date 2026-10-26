# 配置与主题格式参考（preferences.toml）

> 面向用户与维护者的**配置面 API 文档**（发布合规项 DOC-03）。
> 字段、类型与默认值均取自代码真源，出处标注在各节；
> 修改优先走应用内设置页，本文档用于了解结构、手改与排障。
> 真源代码：`crates/rustmark-core/src/domain/preferences/`（结构与校验）、
> `crates/rustmark-core/src/infrastructure/persistence/toml_preferences_repository.rs`（读写与回退）。

---

## 1. 文件位置

配置目录下的 `preferences.toml`（同目录还有 `recent.json` 最近文件、
`crash/` 崩溃报告、`.rustmark-draft/` 草稿，见 `PRIVACY.md` §4）：

| 平台 | 路径 |
|---|---|
| Windows | `%APPDATA%\rustmark\preferences.toml` |
| macOS | `~/Library/Application Support/rustmark/preferences.toml` |
| Linux | `~/.config/rustmark/preferences.toml` |

## 2. 加载与回退行为

出处：`toml_preferences_repository.rs::load`（L72–L111）。

| 情形 | 行为 |
|---|---|
| 文件不存在 / 内容为空 | 首次启动，直接用默认配置，不报错 |
| TOML 解析失败（含语法错误、缺必填字段、类型不符） | 原文件备份为 `preferences.toml.bak-<时间戳>`，回退默认配置 |
| `schema_version` 高于当前实现（现为 `1`） | 备份 + 尝试沿用（新版降级场景），结构不兼容才回退默认 |
| 解析成功但取值非法（字号/行高越界、自动保存 0 秒等） | 备份 + 回退默认配置 |
| 未知字段 | 忽略（不影响加载）——旧版配置在新版中可读 |

写入前同样做取值校验，非法配置**拒绝落盘**；写入为「临时文件 + rename」原子写。

> **注意**：顶层四个键（`schema_version` / `theme` / `editor` / `shortcuts`）
> 与 `theme` 的 `kind`、`font_family`、`font_size`、`line_height`，
> 以及 `editor` 的 `tab_size`、`word_wrap`、`spell_check`、`undo_merge_window_ms`
> **没有缺省值**——手改时删掉它们会导致整份配置回退默认（原文件留 `.bak`）。
> 其余字段均有 `#[serde(default)]`，缺失时按默认值读出。

## 3. 顶层结构

出处：`crates/rustmark-core/src/domain/preferences/aggregate.rs`（`PREFERENCES_SCHEMA_VERSION = 1`）。

```toml
schema_version = 1        # 配置 schema 版本（ADR-003），结构变更时递增
[theme]                   # 主题、字体、行高、自定义调色板
[editor]                  # 编辑器行为
[shortcuts]               # 快捷键绑定表
```

## 4. `[theme]` 主题与字体

出处：`crates/rustmark-core/src/domain/preferences/value_objects/theme.rs`（L175–L201）。
校验：字体族非空；字号 8–48；行高 1.0–3.0（越界即整份配置回退默认）。

| 键 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `kind` | 字符串 | `"system"` | `"light"` / `"dark"` / `"system"` / `"custom"`（`snake_case`，大小写不敏感） |
| `font_family` | 字符串 | `"system-ui"` | 编辑区字体族；空串非法 |
| `font_size` | 浮点 | `14.0` | 编辑区字号（px），合法区间 **8.0–48.0** |
| `line_height` | 浮点 | `1.6` | 行高倍数，合法区间 **1.0–3.0** |
| `custom` | 表（可选） | 无 | 自定义调色板；仅当 `kind = "custom"` 时生效，见 §5 |

### 4.1 `[theme.custom]` 自定义调色板

出处：`theme.rs::CustomThemePalette`（L46–L121）。
颜色一律 **`#RRGGBB`** 十六进制字符串（带 `#`、6 位、大写或小写均可）。

**8 项核心语义色（必填）**——缺任一项即解析失败、整份配置回退默认：

| 键 | 语义 | 默认值（首次切到「自定义」时的起点 = 内置深色主题核心色） |
|---|---|---|
| `bg_primary` | 主背景（编辑区） | `#14161B` |
| `bg_secondary` | 次背景（工具栏 / 状态栏） | `#1A1D24` |
| `text_primary` | 主文本色 | `#E8EAF0` |
| `text_secondary` | 次文本色（占位符 / 禁用态） | `#9AA4B4` |
| `accent` | 强调色（选中项 / 链接） | `#6AA9FF` |
| `code_bg` | 代码块背景 | `#1C212B` |
| `border` | 边框 / 分割线 | `#2B323D` |
| `selection_bg` | 选区背景 | `#6AA9FF` |

**10 项语法高亮色（可选覆盖）**——缺省 = **跟随内置主题**（随背景明暗自适应），
这是刻意设计：只调背景色时语法高亮不会糊在底色上。

| 键 | 语义 |
|---|---|
| `syn_heading` | 标题 |
| `syn_bold` | 加粗 |
| `syn_italic` | 斜体 |
| `syn_code` | 行内代码 / 代码块 |
| `syn_link` | 链接 |
| `syn_quote` | 引用块 |
| `syn_marker` | 标记符（`#` / `-` 等） |
| `syn_meta` | 元信息（围栏语言名等） |
| `syn_table` | 表格 |
| `syn_other` | 其它（默认正文色） |

> 其余颜色（尺寸、差异色等 30+ 项）刻意**不开放**，沿用内置主题同类值
> （见 `theme.rs` 模块文档：避免「全开放导致调出来不好看」）。

## 5. `[editor]` 编辑器设置

出处：`theme.rs` / `editor.rs::EditorSettings`（L159–L269）。

| 键 | 类型 | 默认 | 说明与约束 |
|---|---|---|---|
| `tab_size` | 字符串 | `"Four"` | 缩进宽度枚举：`"Two"` / `"Four"` / `"Eight"`（**变体名**，非数字） |
| `word_wrap` | 布尔 | `true` | 自动换行 |
| `auto_save_interval` | 整数或省略 | `30` | 自动保存间隔（秒）；**`0` 非法**（保存被拒）；键省略 = 关闭自动保存 |
| `spell_check` | 布尔 | `false` | 拼写检查（内置 3779 词英文词表，演示级） |
| `show_line_numbers` | 布尔 | `true` | 行号栏（缺省按 `true` 读出） |
| `undo_merge_window_ms` | 整数 | `400` | 撤销合并窗口（毫秒）——连续输入在此窗口内合并为一步 |
| `split_ratio` | 浮点 | `0.5` | 编辑区 / 预览区分割比例，合法区间 **0.2–0.8**（缺省按 `0.5`） |
| `sidebar_visible` | 布尔或省略 | 省略 | **三态**：键省略 = 用户从未决定过（按收起渲染，打开文件夹时可自动展开）；`true/false` = 用户的明确决定，此后一律尊重。省略时不写盘 |
| `sidebar_width` | 浮点或省略 | 省略 | 侧栏宽度（pt）；省略 = 用主题默认宽度。用户没拖过分割条时不写盘 |
| `writing_goal` | 整数 | `0` | 写作目标字数；`0` = 未设置；上限 **1,000,000**（超出饱和截断，不报错） |
| `max_open_documents` | 整数 | `12` | 标签条最多同时打开数，合法区间 **1–30**；手改写 `0` 会被收敛为 `1`；达限后新打开请求被拒绝（已开不受影响） |
| `[editor.lint]` | 表 | 全 `true` | 格式检查（lint）分规则开关，见 §5.1 |

### 5.1 `[editor.lint]` 检查规则开关

出处：`theme.rs::LintSettings`（L136–L173）；缺省一律 `true`（默认全开）。

| 键 | 规则 | 默认 |
|---|---|---|
| `heading_jump` | 标题层级跳跃 | `true` |
| `list_marker_mix` | 列表符号混用 | `true` |
| `trailing_whitespace` | 行尾多余空格 | `true` |
| `unclosed_fence` | 未闭合代码围栏 | `true` |

## 6. `[shortcuts]` 快捷键绑定

出处：`shortcut_table.rs`（L11–L305）、`keys.rs::KeyCombo`（L256–L351）。

结构为**数组表**：每条含 `action`（动作标识）与 `combo`（组合键字符串）：

```toml
[[shortcuts.bindings]]
action = "save"
combo = "Ctrl+S"
```

### 6.1 `combo` 组合键语法

- 以 `+` 分隔；片段大小写不敏感；顺序无关（`Ctrl+Shift+S` ≡ `Shift+Ctrl+s`）；
- 修饰键别名：`ctrl`/`control`、`alt`/`option`、`shift`、
  `meta`/`cmd`/`command`/`super`/`win`；
- 主键：单字符（`a`、`=`、`[`、`\`）或功能键名（`esc`/`escape`、`enter`/`return`/`cr`、
  `tab`、`f1`–`f12` 等，完整别名见 `keys.rs::NamedKey::parse`）；
- 无主键或多个主键 → 解析失败（配置回退默认）；
- **系统保留组合**（绑定即报冲突）：`Meta+Q`、`Meta+W`、`Alt+F4`、`Ctrl+Alt+Delete`；
- 冲突**只报告不拒绝**：设置页把重复 / 保留 / 裸字符键（提示级）列出来由用户处置。

### 6.2 `action` 动作标识（38 个，`snake_case`）

出处：`action.rs::ActionId::key()`（持久化契约，改名会使已存绑定失效）：

```
new_document  open_document  save  save_as  save_all  close_tab
undo  redo  find  replace  find_in_files
bold  italic  strikethrough  inline_code  insert_link  code_block
insert_table  toggle_quote  promote_heading  demote_heading
indent  outdent  toggle_task
toggle_preview  toggle_outline  toggle_sidebar  toggle_statusbar
command_palette  open_settings  export  focus_mode  close_overlay
document_stats  insert_symbol  compare_diff  next_tab  prev_tab
```

> 「可持久化动作」与「有默认键位的动作」不同：如 `toggle_statusbar`
> 可录制改键但默认不占键位（`action.rs::all()` 的说明）。

> **动作全集口径（代码实证，2026-10-26 复核）**：`UiAction` 共 **57** 个变体，其中
> 上表 **38** 个进入快捷键持久化（`UiAction::persisted_key()` 返回 `Some`）。
> 另有 **19** 个为仅 UI / 菜单动作不持久化（`persisted_key()` 返回 `None`）：
> `open_folder`、`revert`、`insert_image`、`toggle_list`、`toggle_ordered_list`、
> `move_line_up`、`move_line_down`、`goto_tab1`–`goto_tab9`、
> `shortcut_overview`、`about`、`toggle_theme`。
> 二者由 `action/impls.rs` 的 `persisted_key` 单一 `match` 守门，新增变体时编译即强制补全。

### 6.3 默认键位表

出处：`shortcut_table.rs::default_bindings()`（L150–L193）。

| 分类 | 默认绑定 |
|---|---|
| 文件 | `Ctrl+N` 新建 · `Ctrl+O` 打开 · `Ctrl+S` 保存 · `Ctrl+Alt+S` 全部保存 · `Ctrl+Alt+Shift+S` 另存为 · `Ctrl+Alt+W` 关闭标签 · `Ctrl+E` 导出 |
| 编辑 | `Ctrl+Z` 撤销 · `Ctrl+Shift+Z` 重做 · `Ctrl+F` 查找 · `Ctrl+H` 替换 · `Ctrl+Shift+F` 全局查找 |
| 格式 | `Ctrl+B` 加粗 · `Ctrl+I` 斜体 · `Ctrl+Shift+X` 删除线 · `Ctrl+Shift+C` 行内代码 · `Ctrl+K` 链接 · `Ctrl+Shift+K` 代码块 · `Ctrl+Shift+T` 表格 · `Ctrl+Shift+Q` 引用 · `Ctrl+=` 提级 · `Ctrl+-` 降级 · `Ctrl+]` 缩进 · `Ctrl+[` 反缩进 · `Ctrl+Shift+Enter` 任务勾选 |
| 视图 | `Ctrl+P` 预览 · `Ctrl+Shift+O` 大纲 · `Ctrl+\` 侧栏 · `Ctrl+Shift+S` 文档统计 · `Ctrl+Shift+P` 命令面板 · `Ctrl+Shift+E` 符号 · `Ctrl+Alt+D` 对比 · `Ctrl+Shift+]` / `Ctrl+Shift+[`前后标签 · `Ctrl+,` 设置 |
| 单键 | `F8` 专注模式 · `Esc` 关闭浮层 |

## 7. 完整示例

可直接保存为 `preferences.toml` 的全字段示例（值均为默认值 + 一处自定义演示）：

```toml
schema_version = 1

[theme]
kind = "system"            # light | dark | system | custom
font_family = "system-ui"
font_size = 14.0           # 8.0 – 48.0
line_height = 1.6          # 1.0 – 3.0

[editor]
tab_size = "Four"          # Two | Four | Eight
word_wrap = true
auto_save_interval = 30    # 秒；删除本键 = 关闭自动保存（0 非法）
spell_check = false
show_line_numbers = true
undo_merge_window_ms = 400
split_ratio = 0.5          # 0.2 – 0.8
writing_goal = 0           # 0 = 未设置；上限 1_000_000
max_open_documents = 12    # 1 – 30
# sidebar_visible / sidebar_width 故意省略：表示「用户从未决定过」，
# 打开文件夹时应用可自动展开侧栏；手动开合后由应用写入。

[editor.lint]
heading_jump = true
list_marker_mix = true
trailing_whitespace = true
unclosed_fence = true

[[shortcuts.bindings]]
action = "save"
combo = "Ctrl+S"

[[shortcuts.bindings]]
action = "find"
combo = "Ctrl+F"

[[shortcuts.bindings]]
action = "focus_mode"
combo = "F8"

[[shortcuts.bindings]]
action = "close_overlay"
combo = "Esc"
```

> `bindings` 只需列出**你关心的**绑定；未列出的动作在绑定表中即为「无绑定」，
> 可随时在设置页重新录制（设置页的「恢复默认」会重建 §6.3 的全表）。
> 也可以 `kind = "custom"` 并加一节 `[theme.custom]`（§5 的 8 项必填色起步）。

## 8. 修改建议

1. **优先用设置页**：改键有冲突检测与录制 UI，主题有取色器，校验由应用统一执行；
2. 手改文件时**在应用未运行时进行**（应用保存设置时会整份覆写）；
3. 改坏了不必慌：启动时自动备份 `.bak-<时间戳>` 并回退默认，见 §2；
4. 验证手段：改完启动应用 → 打开设置页逐项核对；解析层测试见
   `tests/infrastructure/preferences_repository_test.rs`、
   `tests/domain/preferences_test.rs`。
