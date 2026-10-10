# P1 / P2 未交付功能 · 逐项分析与修复方案

> 编写：2026-10-08（基于代码实测，非凭记忆）  
> 范围：数学公式/Mermaid、图片（PNG）导出、自定义主题、拖拽打开文件、拼写检查  
> 状态：**5 项全部已实施**（均零依赖）；整篇长图 PNG 亦已于 2026-10-09 补齐实施（复用截图回路 + 分段滚动拼接，见实施日志第 5 项）。同日另修复四项交付后遗留缺陷（拖拽目录误开、GB18030 四字节解码、保存未写回原编码、导出 HTML 未注入公式/图表渲染资源），逐步骤记录见 `docs/p1p2-implementation-log.md`。
> 约束：保持现有技术选型（Rust 2024 + egui 0.33 + wgpu 后端，ADR-001）；**不引入未经确认的新依赖或架构调整**；表现层不得反向依赖配置读写（须经 application 层）。

---

## 0. 总览与优先级建议

| # | 功能             | 等级  | 现状                                        | 是否需新依赖                    | 建议顺序   |
| - | -------------- | --- | ----------------------------------------- | ------------------------- | ------ |
| 1 | 数学公式 / Mermaid | P1  | ✅ **已完成**（零依赖，编辑器内结构化卡片 + 导出保留） | 路线①零依赖（已采用）；②/③需 wasm/node | 3 ✅  |
| 2 | 图片导出 / 视口截图  | P2  | ✅ **已完成**（零依赖，egui 官方截图 API + 预览区裁剪） | **零依赖**（已采用，官方 API 双后端已实现） | 5 ✅ |
| 3 | 自定义主题          | P2  | ✅ **已完成**（零依赖，8 核心语义色可配）         | **零依赖**（已采用）              | 2 ✅  |
| 4 | 拖拽打开文件         | P2  | ✅ **已完成**（eframe 原生拖放）                 | **零依赖**（已采用）              | 1 ✅  |
| 5 | 拼写检查           | ✅ **已完成**（零依赖，内置 3779 词 + 近邻门槛 + 波浪线） | 路线①零依赖（已采用）；②/③需 C/系统库 | 4 ✅  |

**原则**：零依赖优先、独立可测优先；每项均有「零依赖降级版」可先交付。凡需引入依赖的路线一律显式标注「待裁定」，不自行决定。

**当前进度**：**全部 5 项已完成，均零依赖，未引入任何新依赖**。第 2 项按用户裁定交付的是**视口截图**子集（整篇长图属后续，见第 2 节）。逐步骤记录见 `docs/p1p2-implementation-log.md`。

> 各节「需裁定」小节保留**原始问题原文**以便追溯；实际裁定答案见每节末尾的「✅ 实施状态」，汇总见第 7 节。

---

## 1. 数学公式 / Mermaid（P1）

### 现状（代码定位）

- **解析层**：`src/infrastructure/parsing/pulldown_parser.rs:466` 把 `Event::InlineMath / DisplayMath` 当作纯文本 `push_content` 塞进段落，**未识别为独立块**；Mermaid 以 ` ```mermaid ` 围栏存在，被解析成 `BlockKind::CodeBlock { lang: "mermaid" }`（当普通代码块渲染）。
- **领域层**：`src/domain/markdown/value_objects.rs:150` 的 `BlockKind` 枚举**无 Math/Diagram 变体**，仅 `Raw { marker }` 作预留位（注释明示"阶段 06 以后按 ADR 追加"）。
- **预览层**：`src/presentation/views/preview_view.rs:241` 按 `block.kind` 字符串分发，`draw_raw`（650 行）把未识别块等宽原样显示。
- **导出层（bug）**：`pulldown_html_exporter.rs` 用 `html::push_html` 输出 `<math>` 标签，但 `src/infrastructure/export/html_sanitize.rs:82` 的 `DROP_TAGS` 含 `math`（连同 `svg` 等），**math 在导出 HTML 时被静默丢弃子树**——即"写了公式，导出后消失"。

### 修复目标

编辑器内结构化呈现公式/图表；导出 HTML/PDF 时正确保留并可由目标渲染引擎呈现。

### 技术路线（三选一，需裁定）

- **路线①（推荐，零新依赖，编辑器内"结构化占位"）**
  - 解析层新增 `BlockKind::Math { display: bool }` 与 `BlockKind::Diagram { kind: String }`（mermaid）；`pulldown_parser` 把 math 事件与 ` ```mermaid ` 代码块映射成对应变体。
  - 预览层新增 `draw_math` / `draw_diagram`：画带"公式 / 图表"标注的代码卡片（显示源码 + 复制按钮 + 提示"导出为 HTML/PDF 时由 KaTeX/Mermaid 渲染"）。
  - 导出层把 `math` 从 `DROP_TAGS` 移除，输出 `<span class="math">…</span>` / `<div class="mermaid">…</div>` 占位（交由浏览器/打印管线渲染）。
  - **代价**：编辑器内不渲染像素级公式（仅结构化源码卡），但修复了"导出丢失"这一真 bug，且零依赖。
- **路线②（编辑器内真渲染，需引入依赖）**：嵌入 KaTeX/Mermaid 的 WASM 并在桌面跑 wasm 运行时（wasmtime）或调 node 子进程——**重依赖、跨平台复杂，违背 ADR-001 轻量定位**，不推荐。
- **路线③（导出时用外部 CLI）**：HTML/PDF 导出时调用系统 `node` + katex/mermaid-cli 生成 SVG 嵌入——与现有 PDF（依赖系统打印）同源，但要求用户装 node。

### 预估改动文件（路线①）

`value_objects.rs`（枚举 + 映射逻辑）、`pulldown_parser.rs`（math→Math、mermaid 识别）、`dto.rs`（`BlockView` 加 kind 字符串）、`preview_view.rs`（draw_math/draw_diagram）、`html_sanitize.rs`（math 移出 DROP）、`pulldown_html_exporter.rs`（输出占位标记）。

### 验收

- 解析层单测：math 行内/块级 → `BlockKind::Math{display}`；` ```mermaid ` → `BlockKind::Diagram`。
- 预览层单测：draw_math/draw_diagram 不 panic 且标注正确（可复用 DEF-04 的 `highlighted_code_job` 式纯函数测试法）。
- 导出单测：导出 HTML 含 `<span class="math">`/`<div class="mermaid">` 且**未被 sanitize 丢弃**（修复回归）。

### ⚠️ 需裁定（原文保留，答案见下）

1. 选哪条路线；若②/③，是否接受引入 node/wasm 依赖（突破现行约束）。
2. 路线①下，mermaid 是否也走"结构化占位"（同 math），还是优先 mermaid 走真渲染（mermaid 纯 SVG，比 LaTeX 易在 Rust 侧处理）。

### ✅ 实施状态（2026-10-08 已完成，路线①，零依赖）

采用**路线①**并已实施完毕，步骤详见 `docs/p1p2-implementation-log.md` 第 3 项。实施中对本节的两处判断做了修正，记录如下：

1. **「导出丢弃 math」并非真 bug**。核查 pulldown-cmark 0.13.4 `src/html.rs:112-121`，公式实际输出为 `<span class="math math-inline">` / `<span class="math math-display">`，而 `span` 与 `class` 均在清洗白名单内——**不会**被 `DROP_TAGS` 丢弃。真实缺口只是解析器与导出器的 `Options` 都未开启 `ENABLE_MATH`（故 `$...$` 被当普通文本，且不报错）。因此**未从 `DROP_TAGS` 移除 `math`**（该项仅对手写原生 `<math>` 生效，保留更安全），改以导出单测锁定公式不丢。
2. **行内公式不独立成块**。`$...$` 在事件流里位于段落内部，若拆成独立块会把一句话分成三块、割裂段落语义。改为补回 `$` 分隔符并入段落 `content`（大纲/搜索仍可命中）；仅当整段内容**整体**是 `$$...$$` 时才升级为独立 `Math { display: true }` 块。

**仍待处理（不在本项范围）**：导出 HTML 只输出 `math` / `math-display` 类名，**未注入 KaTeX / Mermaid 资源**，浏览器打开时公式仍是源码形态。若要开箱即渲染，需另立一项处理「导出模板资源注入 + 可选离线资源包」，涉及外链策略需先确认。

---

## 2. 图片导出（P2）· 已交付**视口截图**子集

### 现状

`src/infrastructure/export/routing_exporter.rs` 的 `Image` 分支返回明确 `Err`。
像素来自 **GPU 对当前帧的回读**，而非 Markdown 源文本，故不经内容导出器。

### 裁定结果（3 处）

1. **范围**：先交付**视口截图**（截当前预览区所见），非整篇长图。整篇长图需离屏重建完整布局，属另一个量级。
2. **技术路线**：改用 egui **官方截图 API**（`ViewportCommand::Screenshot` / `Event::Screenshot`），
   wgpu 与 glow 两后端均已实现，**零新增依赖**。
3. **原路线①作废**：本节原推荐的「复用 eframe wgpu `RenderState` 离屏渲染」**不需要**——
   `RenderState` 由 eframe 在 `update` 期间持有，**拿不到**（这是当时判断需离屏的根因）；
   而官方 API 提供了更短的等价路径（回读已渲染帧），保真度也更高。

### ⚠️ 方案误判修正

上一轮结论「egui 0.33 不存在截图 API」**错误**——当时只搜 `request_screenshot` 一个方法名，
未搜到便下「不存在」的结论。正确入口见下：

```
ctx.send_viewport_cmd(ViewportCommand::Screenshot(user_data))   // 帧 N 发出
   → eframe 帧末交后端 → wgpu CaptureState 全帧回读 / glow read_screen_rgba
   → Event::Screenshot { image: Arc<ColorImage> }               // 帧 N+1 收到
```

因此**已删除**写完的自研 CPU 软件光栅化器（约 350 行）：官方回读的是**已光栅化完成的帧**，
语法高亮的图片纹理、抗锯齿、emoji 字体回退全部天然正确，而自研需为每一项单独实现并各自出错。

### 实际改动文件

`infrastructure/export/viewport_capture.rs`（新增纯函数）、`application/export/service.rs`
（新增 `complete_deferred`）、`app/frame.rs`（`begin_viewport_capture` / `pump_capture`）、
`presentation/views/main_view.rs`（记录预览区矩形）、`export_view.rs`、`export_ui_state.rs`。

### 验收

已达成：导出 PNG 为**预览区在窗口中的可见部分**；1217 测试全绿；四项门禁全绿。
未达成（属后续）：整篇长图。详见 `docs/p1p2-implementation-log.md` 第 5 项。

---

## 3. 自定义主题（P2）· 零依赖，推荐优先

### 现状

`src/domain/preferences/value_objects.rs:18` 的 `ThemeKind` 仅 `Light/Dark/System`；`src/presentation/theme/builtin.rs:121` 的 `Theme` 是 30+ 个 `Color32` 字段的固定构造（`dark()`/`light()`）。无自定义入口。

### 修复目标

用户可自定义核心配色并持久化。

### 方案（两档）

- **第一版（推荐）**：新增 `ThemeKind::Custom`；设置页提供核心色编辑（建议 ~8 个：bg_primary / bg_secondary / text_primary / text_secondary / accent / code_bg / border / selection_bg），持久化为 TOML；`builtin.rs` 增 `Theme::from_palette(palette)` 构造。
  - **零新依赖**，改动集中在偏好与主题构造。
  - **arch 注意**：presentation 经 application 取主题，不直接读配置（遵守分层）。
- **第二版（后续）**：导入 VS Code `.json` 主题（解析 `tokenColors` 映射到项目语义色），工作量更大。

### 预估改动文件

`domain/preferences/value_objects.rs`（枚举 + 序列化）、`application/preferences/service.rs`（读写 custom 调色板）、`presentation/theme/builtin.rs`（from_palette）、`presentation/views/settings_view.rs`（调色 UI）、`src/app/frame.rs`（应用主题切换）。

### 验收

切换 custom 后编辑区/预览配色即时变化；重启后保持（TOML 持久化生效）；单元测 palette→Theme 映射；arch-check 不报反向依赖。

### ⚠️ 需裁定（原文保留，答案见下）

1. 第一版核心色集合是否就这 8 个，还是要更细（语法高亮色也开放）。
2. 是否要第二版导入（影响工作量评估）。

> **状态（2026-10-08）**：第一版已实施（推荐方案，零依赖）。8 个核心色集合按方案落地；持久化经既有 `PreferencesRepository` 自动生效；`cargo test` 新增 5 例（3 例 `resolve` + 2 例服务回读）全过；`clippy -D warnings` / `check-arch` 全绿。详见 `docs/p1p2-implementation-log.md` 第 2 项。第二版（VS Code 主题导入）仍待裁定。

---

## 4. 拖拽打开文件（P2）· 零依赖，最干净，建议先做

### 现状

全仓无文件拖放处理。eframe 0.33 桌面后端经 winit 提供 `ctx.input(|i| i.raw.dropped_files)`（项目已用 `ctx.input` 读事件，见 `command_palette_view.rs:73`）。现有"打开文件"流程已通过 `rfd` 0.17 实现（`Cargo.toml` 已依赖），可复用。

### 修复目标

把 `.md` 文件拖入窗口即打开。

### 方案

在 `src/app/frame.rs` 的 `update` 开头检测 `ctx.input(|i| i.raw.dropped_files)`，对每个 `dropped_file.path` 调用现有"打开文档"逻辑（复用 workspace service 的打开/新建流程）。`app/` 是组合根，可直接调 application 服务，不违反分层。

### 预估改动文件

`src/app/frame.rs`（检测 + 调打开）、可能涉及 `src/app/bootstrap.rs` 暴露打开路径的入口（若现有打开逻辑未以可复用函数暴露）。

### 验收

拖入 `.md` 文件 → 打开该文档；headless 难测真实拖放事件，加集成测试注入 `dropped_files` 或手动 + Stage 09 真机验证。

### ⚠️ 需裁定（原文保留，答案见下）

1. 拖入非 `.md` 文件的行为：拒绝 / 尝试按文本打开 / 弹窗询问。
2. 拖入多个文件：批量打开还是只开第一个。

### ✅ 实施状态（2026-10-08 已完成，零依赖）

采用零依赖方案并已实施完毕，步骤详见 `docs/p1p2-implementation-log.md` 第 1 项。两处裁定的实际答案：

1. **非 markdown 家族的处理**：不是简单按 `.md` 判定，而是引入 `is_openable_text` 接受 `.md` / `.markdown` / `.txt`（大小写不敏感）——纯文本本就适合在编辑器里打开，一并拒绝会让用户无法查看 README 之类。**其余类型拒绝**并用 `set_status_notice` 明确提示（不静默丢弃，那会让人以为程序没收到文件）。
2. **多文件**：**批量打开全部可识别项**，而非只开第一个。拖多个文件本身就是批量意图，只开一个会让其余文件无声消失。

补充决策：无本地路径的虚拟文件（部分平台拖入 URL/内存数据时会出现）直接忽略；拖入目录属边缘情况，当前会被 `open_path` 当文件尝试并报错，**未专门处理**。

---

## 5. 拼写检查（演示级 → 生产级）

### 现状

`spell_check` 是 `bool` 字段 + 设置项 + 状态栏显示（`status_bar_view.rs:98` 等），`infrastructure` 下**无任何词典/算法模块**；egui 0.33 的 `TextEdit` **无原生拼写下划线**。

### 修复目标

开启后，编辑区英文错词显示红色波浪线（中文不做）。

### 技术路线（三选一）

- **路线①（推荐第一版，零大依赖）**：内置小型英文词表（vendored 数万词或 `words` crate）+ 简单空白/标点分词 + 未匹配词标记。中文按字无空格，不属拼写范畴，跳过。
- **路线②（生产级）**：`hunspell` 绑定（需 libhunspell 系统库或 vendored C 代码，如 `hunspell-rs`）——引入 C 依赖，跨平台编译复杂。
- **路线③（系统 API）**：Windows `ISpellChecker` / macOS `NSSpellChecker` / Linux `enchant`——平台分支多，工作量大。
- **关键难点（不论哪条路线）**：egui `TextEdit` 不直接支持拼写波浪线，需用 `TextEdit::layouter` 自定义或在底层 rope 上标注后覆盖绘制——需单独评估改动量，是本项目落地拼写的最大不确定性。

### 预估改动文件

`infrastructure/spell/`（词典 + 检查，新增）、`presentation/views/editor_view.rs`（layouter/波浪线绘制）、`application`（接入开关到检查流程）、设置项已具备。

### 验收

英文错词显示红色波浪线；中文不受影响；大文档（≥1MB）开启后编辑帧不显著退化（防抖已就位，可复用）。

### ⚠️ 需裁定（原文保留，答案见下）

1. 路线选择；若②/③，是否接受引入 C/系统库依赖。
2. 仅英文还是也要支持其他语言（影响词表规模）。
3. 波浪线绘制方案（自定义 layouter vs 覆盖绘制）的改动量是否可接受。

### ✅ 实施状态（2026-10-08 已完成，路线①，零依赖）

采用**路线①**并已实施完毕，步骤详见 `docs/p1p2-implementation-log.md` 第 4 项。三处裁定的实际答案：

1. **路线 → ①**：内置词表 + 近邻门槛，**零依赖**（路线②需 libhunspell 系统库，路线③需三平台分支）。
2. **语言范围 → 仅英文**：中文按字节扫描天然不产出候选，行为已在测试中固化。其他语言需各自的词表与形态规则，属独立工作项。
3. **波浪线方案 → 复用既有自定义 `layouter`**：在它内部拿到本帧真实 `Galley` 后就地用 `painter` 绘制。改动量比预想小，因为编辑区**本来就**有自定义 layouter（用于语法高亮），属搭车而非新增机制。`squiggles_for` 抽为**纯函数**，使跨行拆分、相位错开、越界丢弃都能无头断言。

实施中的最大意外：初版在真实技术文档上**误报 53 个**，绝大多数是 `colors`/`dropped`/`running` 这类屈折形态——词表存原形、文本是变形，找不到近邻。补词干还原（`stems()`）后降到 14，再补词表降到 **0**（对 26 份项目文档逐一验证）。这段修正比主算法本身更关键。

---

## 6. 实施顺序与门禁

建议顺序（零依赖优先、独立可测优先）：

1. **拖拽打开**（#4）——最小最干净，先建立"P 级功能开始交付"的节奏。
2. **自定义主题**（#3）——零依赖、独立、可单测。
3. **数学公式/Mermaid 路线①**（#1）——零依赖，且修复"导出丢 math"真 bug。
4. **拼写检查路线①**（#5）——零依赖英文可用。
5. **图片导出**（#2）——已按裁定交付**视口截图**子集（官方截图 API，零依赖）。

五项**全部已完成**。每项实施时沿用既有门禁：`cargo fmt --check`、`clippy -D warnings`、`check-arch`、`cargo test --all-targets`；跨平台相关（拖放、截图）转 Stage 09 真机补测。

## 7. 待用户裁定汇总（不自行决定）

**全部已裁定完毕**，五项均已实施：

- #1 路线 → ①零依赖（已交付）。
- #2 路线 → **视口截图子集 + egui 官方截图 API**（已交付；原推荐的 wgpu 离屏路线作废，理由见第 2 节）。
- #3 核心色集合 → 8 项核心语义色（已交付）。
- #4 非 .md / 多文件 → 拒绝非 markdown 家族（已交付）。
- #5 路线 → ①零依赖英文 + 自定义 layouter 波浪线（已交付）。
