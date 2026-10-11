# P1 / P2 功能实施步骤记录

> 编写：2026-10-08 起（按 `docs/p1p2-remediation-plan.md` 推荐方案与实施顺序）  
> 顺序：拖拽打开 → 自定义主题 → 数学公式/Mermaid → 拼写检查 → 长图 PNG  
> 门禁：`cargo fmt --check` 零差异 · `clippy --workspace --all-targets -D warnings` 0 告警 · `check-arch` OK · `cargo test --all-targets` 通过  
> 约定：每项记录「修改文件清单 / 关键步骤 / 验证 / 已知问题」。

---

## ✅ 第 1 项 · 拖拽打开文件（P2，零依赖，推荐方案）

**修改文件清单**

- `src/app/frame.rs`：新增自由函数 `is_openable_text` / `classify_dropped`；新增方法 `EditorApp::handle_dropped_files`；在 `update` 中加入步骤 2.5 调用；新增 `#[cfg(test)] mod tests`（3 例）。
- `tests/integration/drag_open_test.rs`（新增）：3 例端到端（打开 .md / 拒绝 .png / 批量混拖）。
- `tests/integration/main.rs`：注册 `mod drag_open_test`。

**关键步骤**

1. 扩展名分类纯函数：`is_openable_text` 接受 `.md` / `.markdown` / `.txt`（大小写不敏感）；`classify_dropped` 按扩展名分组为「可打开 / 拒绝」。
2. `handle_dropped_files`：每帧读 `ctx.input(|i| i.raw.dropped_files)`，提取本地路径 → 分类 → 逐个 `open_path` 打开；有拒绝项时 `set_status_notice` 提示。
3. `update` 在「后台对话框结果」之后、「快捷键」之前调用 `handle_dropped_files`（与对话框同级，避免残留按键干扰）。
4. 决策（推荐方案下的合理默认）：仅接受文本类；非文本拒绝并提示；一次多文件批量打开所有可识别项；无本地路径的虚拟文件忽略。

**验证**

- 单元：`classify_dropped_splits_by_extension` / `_is_case_insensitive` / `_empty_is_noop` 全过。
- 集成：`drag_drop_markdown_opens_document` / `drag_drop_non_text_is_rejected` / `drag_drop_mixed_batch_opens_only_text` 全过（headless 注入 `RawInput.dropped_files`）。
- `clippy -D warnings` 0 告警；`check-arch` OK。

**已知问题**

- 跨平台真实拖放手势需在 Stage 09 真机补测（headless 仅验证逻辑路径）。
- 拖入目录（非文件）当前会被 `open_path` 当作文件尝试打开而报错——属边缘情况，未专门处理。

---

## ✅ 第 2 项 · 自定义主题（P2，零依赖，推荐方案）

**修改文件清单**

- `src/domain/preferences/value_objects.rs`：领域 `ThemeKind` 新增 `Custom` 变体；新增 `CustomThemePalette`（8 个核心语义色，`#RRGGBB` 字符串存）；`Theme` 增加 `pub custom: Option<CustomThemePalette>`；`ThemeKind` 增加 `as_str()`；`CustomThemePalette` 增加 `Default`（默认深色调色板）。
- `src/application/dto.rs`：新增 `CustomThemePaletteView`（含 `Default` + 与领域互转 `From`）；`ThemeView` 增加 `custom: Option<CustomThemePaletteView>`（`#[serde(default)]` 兼容旧配置）；`SettingsView::fallback` 补 `custom: None`。
- `src/application/preferences/service.rs`：`to_theme` 识别 `"custom"` 并转调色板；`to_theme_view` 通过 `as_str()` + `palette()` 回写。
- `src/presentation/theme/builtin.rs`：新增 `Theme::from_custom_palette`（以深色为底覆盖 8 核心色 + 由主背景亮度决定明暗）；`resolve` 处理 `custom`（缺失调色板退回默认）；新增 `parse_hex_color` / `is_dark_color` 辅助；`#[serde]` 无关。
- `src/presentation/views/settings_view.rs`：`THEME_KINDS` 增加「自定义」；`appearance` 在 `kind=="custom"` 时渲染 8 行取色器（`custom_palette_editor` / `color_row` / hex 辅助）；`live_preview` 显示「自定义主题」标签。
- `src/app/frame.rs`：`cancel_settings` 快照回填 `ThemeView` 补 `custom: None`。
- 测试字面量补齐 `custom`：`tests/application/preferences_service_test.rs`（×4）、`tests/application/event_flow_test.rs`、`tests/presentation/theme_mapper_test.rs`（×3）、`tests/app/frame_test.rs` 无需（来自回读）。
- 新增测试：`theme_mapper_test.rs` 3 例（`overrides_core_colors` / `without_palette_falls_back_to_dark` / `light_background_is_light_mode`）；`preferences_service_test.rs` 2 例（`roundtrips_palette` / `without_palette_still_defaults`）。

**关键步骤**

1. 数据模型：领域只开放 8 个核心语义色，其余（语法高亮 / 差异 / 语义色）沿用内置深色值——避免把 30+ 颜色全部开放带来的「调出来不好看」风险。
2. 颜色以 `#RRGGBB` 字符串跨层传递（领域、DTO 均不依赖 egui）；表现层 `parse_hex_color` 完成 `#RRGGBB`→`Color32`，非法值退回兜底而非 panic。
3. 明暗模式由主背景亮度决定（`is_dark_color`），不再锁死 Dark——用户选浅色背景时整界面走浅色。
4. 缺调色板的新配置 / 异常路径退回默认深色调色板，保证首次切到 custom 不崩、不丢配色。
5. 设置页取色器用 egui `color_edit_button_srgb`，逐行编辑，任一改动后整体写回（一次 `apply_theme_field` = 一次落盘 + 一次预览刷新）。

**验证**

- 单元 / 集成：上述 5 例全过；`clippy -D warnings` 0 告警；`check-arch` OK（表现层 `builtin.rs` 经 `application::dto::ThemeView` 桥接，未引入反向依赖）。
- 全量 `cargo test --all-targets -- --skip large_full_text_find_is_bounded` 通过（统计中）。

**已知问题**

- 自定义主题仅覆盖核心色，语法高亮等 20+ 颜色仍沿用内置——属设计取舍（第一版范围），非缺陷。**（2026-10-10 部分解决：V11-09 已开放 10 项语法高亮色的按项覆盖，未覆盖项仍沿用内置同类值；本行为第一版交付时的状态记录）**
- 取色器改动需 Stage 09 真机目测确认观感；headless 无法验证 UI 渲染。

---

## ✅ 第 3 项 · 数学公式 / Mermaid（P1，零依赖，推荐方案）

**修改文件清单**

- `src/domain/markdown/value_objects.rs`：`BlockKind` 新增 `Math { display: bool }` 与 `Diagram { kind: String }` 两个变体（原`Raw` 注释里的「数学公式 / Mermaid 预留位」已兑现）。
- `src/infrastructure/parsing/pulldown_parser.rs`：`parser_options` 增 `ENABLE_MATH`；`block_kind_of` 识别 ` ```mermaid ` 围栏（含大小写不敏感）落到 `Diagram`；`Event::InlineMath` / `Event::DisplayMath` 分开处理并补回 `$` / `$$` 分隔符；新增纯函数 `math_body_if_standalone`；块收口处新增「整段公式升级为 `Math` 块」分支。
- `src/application/dto.rs`：`BlockView` 新增 `display: Option<bool>` 与 `diagram: Option<String>`。
- `src/application/markdown/service.rs`：`block_kind_str` 增 `math` / `diagram`；`to_block_view` 映射两个新字段。
- `src/presentation/views/preview_view.rs`：`draw_block` 增`math` / `diagram` 分派；新增 `draw_math`（行内随正文 / 块级居中成卡+ 复制）与 `draw_diagram`（图表卡 + 等宽源码 + 说明）；`draw_raw` 注释收窄为「原始 HTML」。
- `src/infrastructure/export/pulldown_html_exporter.rs`：`export_options` 增 `ENABLE_MATH`（与解析器对齐）。
- `tests/infrastructure/pulldown_parser_test.rs`：新增 5 例；`tests/infrastructure/exporter_test.rs`：新增 3 例；`tests/application/markdown_service_test.rs`：新增 2 例；`tests/integration/alignment_test.rs`：新增 2 例（对齐矩阵口径，验证导出保留 + 「公式 span 存活 / 原生 `<math>` 仍丢弃」双策略）。

**关键步骤**

1. **方案核实（修正了原方案的错误假设）**：方案文档称「导出时 `<math>` 被 `html_sanitize` 的 `DROP_TAGS` 静默丢弃」。实测 pulldown-cmark 0.13.4 的 `html.rs` 对公式输出的是 `<span class="math math-inline">` / `<span class="math math-display">`，而 `span` 与 `class` 均在清洗白名单内——**该bug 并不存在**。真实缺口只是两处 `Options` 都没开 `ENABLE_MATH`。因此保留 `DROP_TAGS` 中的 `math`（仅对手写原生 `<math>` 生效，留着更安全），改以**导出测试**锁定公式不丢。
2. **行内 vs 块级的结构决策**：行内 `$...$` 落在段落内部，**不独立成块**——独立成块会把一句话拆成三块、割裂段落语义；改为补回 `$` 分隔符并入段落 `content`，保证大纲与搜索仍能命中公式。
3. **块级公式升级规则**：pulldown 把 `$$...$$` 也放在段落里，故新增 `math_body_if_standalone`——仅当整段内容**整体**是 `$$...$$`（内部非空、不含嵌套 `$$`）时，才升级为独立 `Math { display: true }` 块，且`content` 存**公式本体**（表现层直接展示/复制，无需再剥分隔符）。混排公式（如「由 $x$ 可知」）保持段落不变。
4. **Mermaid 识别**：`Tag::CodeBlock` 上判`lang.to_ascii_lowercase() == "mermaid"`，命中则`return` `Diagram`（不落入`CodeBlock`）；刻意用**全等**而非前缀匹配，避免 ` ```mermaid-ish ` 被误判。
5. **零依赖渲染策略**：编辑器内**不内嵌 KaTeX / MathJax**（会引入 JS 运行时与网络依赖）。预览区给出结构化卡片——行内公式随正文基线用等宽着色字体、块级公式独立居中成卡并附复制按钮、图表卡标注引擎名与「导出后由浏览器渲染」。像素级排版交由导出 HTML 侧的浏览器运行时完成。
6. **`BlockView` 兼容性**：`BlockView` 全仓仅在 `to_block_view` 一处构造（已核验），新增字段不破坏任何字面量；领域类型仍不泄漏到表现层，`check-arch` 通过。

**验证**

- 新增 12 例全过：解析器 5 例（`inline_and_display_math_are_distinguished` / `display_math_mixed_with_text_stays_a_paragraph` / `math_without_delimiters_is_not_a_math_block` / `mermaid_fence_becomes_diagram_block` / `mermaid_detection_is_case_insensitive_and_lang_scoped`）、导出器 3 例（`math_survives_html_export` / `math_marks_survive_sanitization` / `mermaid_fence_is_exported_as_code_block`）、服务映射 2 例（`math_block_maps_display_flag_to_view` / `diagram_block_maps_engine_name_to_view`）、对齐 2 例（`align_math_and_mermaid_survive_export` / `align_sanitize_keeps_math_span_but_drops_native_math_tag`）。
- `cargo fmt --check` 零差异；`clippy --workspace --all-targets -- -D warnings` 0 告警；`check-arch` OK。
- 全量 `cargo test --all-targets -- --skip large_full_text_find_is_bounded` 通过：**1160 passed / 0 failed**（13 个测试二进制）。

**实施中的修正（记录以免后人重犯）**

- 初版把行内公式也设计成独立 `Math` 块，测试直接失败（实际产出 `Paragraph`）——**设计缺陷而非实现 bug**。已按第2 步改为「行内并入段落 + 块级升级成卡」。
- `open.content` 移动后再调`open.range(...)` 触发 E0382（部分移动后借用），已改为**先算区间再移内容**。
- `theme.syn_code_keyword` 字段不存在，实际字段名为 `syn_code`。

**已知问题**

- 编辑器内**不做公式排版**（零依赖的既定取舍）：只呈现可辨认的公式源码与结构标注，无分式、根号、积分号的可视化。要像素级效果需导出 HTML 由浏览器侧渲染。
- Mermaid 同理：预览区只给图表卡 + 源码，不出图。
- ~~导出 HTML 目前输出 `math` / `math-display` 类名但**未注入 KaTeX / Mermaid 的 CDN 资源**~~ → **2026-10-09 已修复**：`pulldown_html_exporter.rs` 按导出内容按需注入 KaTeX 0.16.47 / Mermaid 10.9.8 的 CDN 资源与带守卫的渲染脚本（`window.katex` / `window.mermaid` 判断 + try/catch），纯文本导出零外链（测试 `math_in_export_injects_katex_resources` / `mermaid_in_export_injects_mermaid_resources` / `plain_export_has_no_external_resources`）。
- `theme` 换色后公式卡的等宽着色取自 `syn_code`，未做公式语法着色。
- UI观感需 Stage 09 真机目测确认，headless 无法验证渲染效果。

---

## ✅ 第 4 项 · 拼写检查（演示级 → 生产级，P1，零依赖，推荐方案）

**修改文件清单**

- `src/domain/markdown/services.rs`：新增 `SpellChecker` trait（`check` + `is_available`）。
- `src/domain/markdown/value_objects.rs`：新增 `Misspelling`（`Copy + Eq`的错词区间，不带建议串）。
- `src/domain/markdown/errors.rs`：新增 `SpellCheckError`（**刻意不 impl `From<...> for AppError`**）。
- `src/infrastructure/spell.rs` + `src/infrastructure/spell/`（新增）：`dictionary.rs`（词表）、`simple_checker.rs`（分词 + 近邻判定 + 词干还原）、`words.txt`（**3779 词**数据文件）。
- `src/infrastructure.rs`：注册 `spell` 模块。
- `src/application/markdown/queries.rs`：新增 `SpellCheckQuery { doc_id, enabled }`。
- `src/application/markdown/service.rs`：新增 `SpellCache`、`with_spell_checker` builder、`spell_check` 用例。
- `src/app/bootstrap.rs`：以 builder 注入 `VendoredSpellChecker`。
- `src/presentation/theme/mapper.rs`：新增 `SpellMark` / `Squiggle` / `squiggles_for`（纯函数）/ `paint_squiggles`。
- `src/presentation/views/editor_view.rs`：接入查询；`layouter` 内就地绘制波浪线（用 `Theme::error` 色）。
- `tests/infrastructure/spell_checker_test.rs`（新增 17 例）、`tests/application/markdown_service_test.rs`（新增 7 例）、`tests/presentation/spell_squiggle_test.rs`（新增 8 例）、`tests/common/fakes.rs`（新增 `FakeSpellChecker`）。

**关键步骤**

1. **词表来源**：沙箱无法访问网络（curl TLS 被拦，exit 35），无法拉取公开词表，故**自行vendoring** 3779 词。分两部分：约 3400 高频英文 + 约 380 技术/项目术语（`serde` / `egui` / `wasm` / `utf` / `rect` 等），后者对一个 Markdown 编辑器是必需的。
2. **核心算法：分词 + 近邻门槛**。裸判定（不在词表 → 错）会把 `Kubernetes` / `serde` / 人名全部标红。改为 **hunspell 的核心思想**：只有存在**编辑距离 ≤ 1 的词表近邻**时才判错——`teh`/`recieve`/`seperate` 被抓，`Kubernetes` 找不到近邻故放过。
3. **近邻索引（删除变体）**：`VendoredSpellChecker::new` 预先为每个词表词建立「删一字符」的全集。判定拆三条，覆盖距离 1 的全部形态：候选删一字符即词表词（多字母）；候选本身 ∈ 索引（少字母，`mesage`←`message`）；候选的删除变体 ∈ 索引（替换/易位，`teh`←`the`）。
4. **词干还原（关键修正）**：初版在真实技术文档上误报 **53 个**，绝大多数是 `colors`/`dropped`/`running` 这类**屈折形态**——词表存原形，文本是变形，找不到近邻。补 `stems()`（复数 `-s`/`-es`/`-ies`、过去式 `-ed`/`-ied` + 双写辅音还原、进行时 `-ing` + 去 e 还原）后误报降到 14，再补词表降到 **0**。这正是 hunspell 用「词缀规则」解决的问题，此处用保守的英语后缀规则近似。
5. **误报防线集中在 `normalize_candidate`**：内部大写（`camelCase`/`ParseError`）、首字母大写（句首/专有名词）、长度 < 3（`a`/`of`/`is`）一律跳过；中文按字节扫描天然不产出候选。
6. **波浪线绘制**：复用编辑区既有的自定义 `layouter`——在它内部拿到本帧真实 `Galley` 后就地用 `painter` 绘制，天然叠在文本之上且坐标已平移。`squiggles_for` 是**纯函数**（字节区间 × 排版 → 片段几何），使跨行拆分、相位错开、越界丢弃都能无头断言。
7. **跨行错词拆段**：本版本 egui 的 `Row` 不暴露字符区间，故按 `galley.rows` 的纵向位置定位起止行，中间行取整行宽度。
8. **应用层三处刻意差异**（相对 `highlight`）：**不报错**（检查失败降级为空集）、**不防抖**（开着就该立刻反映最新输入）、**大文件降级**（超 500KB 直接跳过）。
9. **缓存**：新增 `SpellCache`（键含版本号），未变的帧不重复付费；`enabled=false` 时**既不查缓存也不调检查器**，让「关开关」零成本。

**验证**

- 基础设施 17 例：召回（`teh`/`recieve`/`seperate`/`definately`/`mesage`）、误报防线（中文 / 驼峰 / 缩写 / 短词 / 首字母大写 / 屈折形态 / 整句英文）、坐标契约（升序不重叠、多字节边界）、纯函数（`check_english` 全判错、`edit_distance` 已知值）、900KB 性能预算。


- 应用层 7 例：开关透传、关闭零成本、缓存命中、**检查失败降级为空集**、词典不可用跳过、大文件降级、未注入 checker 不 panic。
- 表现层 8 例：片段产出、位置随词推进、origin 平移、非法区间不 panic、多字节前缀、跨行拆段、相位错开。
- **实测误报率**：对本仓库 26 份 Markdown 文档（`docs/*.md` + ADR + REQUIREMENTS）逐一跑检查，**误报0 个**；同时 `teh recieve seperate definately mesage` 5/6 命中（`accomodate` 距2，按设计门槛漏判）。
- `cargo fmt --check` 零差异；`clippy -D warnings` 0 告警（修了 2 处 `collapsible_if`）；`check-arch` OK。
- 全量 `cargo test --all-targets -- --skip large_full_text_find_is_bounded`：**1192 passed / 0 failed**。

**实施中的踩坑与修正（记录以免后人重犯）**

- **算法漏洞（测试直接抓出）**：`has_near_neighbor` 最初只实现了「候选删一字符即词表词」，漏掉「少字母」与「替换/易位」两类形态，`mesage` 与 `teh` 都抓不到。补齐三条判定后才正确。中途曾试图用「删字典序最小字符」的统一指纹，**行不通**（丢失信息，`teh`/`the` 指纹不同），已回退。
- **概念澄清**：`teh` → `the` 的**Levenshtein 距离是 2**（Damerau 才是 1），而删除变体索引对易位同样命中。测试断言已按此更正，避免后人误以为 `edit_distance` 实现有 bug。
- **egui API 猜测错误**：`rect_from_range` / `galley.from_utf8` 在 0.33 **不存在**；正确用法是 `CCursor::new(字符索引)` + `pos_from_cursor`，且 `Row` 无 `char_range` 字段。
- **波浪线「一个片段都不产出」**：`pos_from_cursor` 返回**零宽**光标矩形，用它俩拼出的矩形**高度也是 0**，被自己的零面积过滤整条丢掉。必须回到 `galley.rows` 取真实行高。这正是几何纯函数 + 无头测试的价值——否则只是「波浪线不显示」这种肉眼难归因的现象。
- **文件事故**：用 `cat >> file` 时误写成 `cat > file`，**截断了 `tests/application/fakes.rs`**（该文件是 `tests/common/fakes.rs` 经 `#[path]` 引入前的旧位置，并未被实际使用）。已按全量使用点重建 `tests/common/fakes.rs`，123 → 130 例全绿。**教训：对已有文件追加内容必须用 `>>`，且先确认该路径是否真被引用。**

**已知问题**

- **只支持英文**：中文按字切分无空格，不属拼写范畴，直接跳过（已在测试中固化该行为）。
- **词表偏小（3779 词）**：生僻词与专有名词若**恰好**有近邻词表词，仍可能被误判；反之距离 ≥ 2 的错词会漏判（`accomodate` 实测漏判）。这是「低误报优先」路线的固有取舍。
- **词表需人工扩充**：遇到新的误报词需手工加进 `words.txt`。该文件是纯文本、空白分隔，替换即可，无需改代码。
- **首字母大写词一律放过**：故 `Teh`（句首错词）不报；换来的是人名 / 地名零误报。
- **无拼写建议（did you mean）**：当前只画波浪线，未提供右键替换建议。`Misspelling` 刻意不携带 `String` 建议以保持 `Copy + Eq`（渲染层每帧要比对标记集）。
- 波浪线观感需 Stage 09 真机目测（振幅 1.5px / 波长 4px 为经验值）；headless 无法验证渲染效果。

---

## ✅ 第 5 项 · 长图 PNG 导出（P2，零依赖，推荐方案）

> 2026-10-09 实施。此前交付的视口截图子集保持不变；本项补齐「整篇长图」。

**修改文件清单**

- `src/app/frame.rs`：`PendingCapture` 新增 `long: Option<LongImageState>`；新增 `LongImageState`（offsets / next / parts / viewport_h / armed）、`begin_long_capture`、`compute_long_image_skip`、`MAX_LONG_IMAGE_SEGMENTS = 256`；`deliver_capture` / `pump_capture` 支持长图多段分支。
- `src/infrastructure/export/viewport_capture.rs`：新增 `MAX_LONG_IMAGE_HEIGHT = 16_384`、`stitch(parts, skip_top)`（纵向拼接 + 校验）。
- `src/presentation/state/export_ui_state.rs`：新增 `long_image` 开关（`set_format` 切走 Image 时夹紧）。
- `src/presentation/views/export_view.rs`：Image 格式新增「整篇长图」勾选；`run_export` 按开关分派 `begin_viewport_capture` / `begin_long_capture`。

**关键设计**

1. **复用截图回路而非离屏重建**：egui 没有离屏重建布局的公开路径。长图 = 把预览区滚到每个分段位置、各截一帧、基础设施层纵向拼接。段偏移为首段 0、其余每段推进一个视口高、末段滚到底（`content_h - viewport_h`）；仅末段与上一段重叠，拼接时按 `skip_top = (上一段底边 − 本段顶边) × ppp` 丢弃重叠行。
2. **两拍状态机（本项最关键的时序）**：`ViewportCommand::Screenshot` 捕获的是**发命令那一帧**的画面。故每段两拍：预备拍（设 `preview.scroll_y`，`armed=true`）→ 发令拍（`armed && requested=false` 时发命令）。**段交付后必须把 `armed` 清零**——否则步骤 2 会在同一帧立即重发命令，捕获到的仍是上一段画面，长图出现重复内容。headless e2e 注入 `Event::Screenshot` 驱动真实帧循环验证了该时序。
3. **提前拦截**：登记时刻就按 `content_h × ppp` 估算成品高度（上限 16 384px）、按段数（上限 256 屏）拒绝超限请求——不等整个滚动流程跑完才报错。内容不足一屏时直接委托视口截图。
4. **`stitch` 的字节/像素坑**：`RgbaImage::as_raw()` 是**字节**缓冲，切片偏移与行数必须乘 4（RGBA8）。初版按像素数切片，`from_raw` 长度校验拒绝（报「拼接缓冲尺寸不符」）——单测五例全过却漏了它，是 e2e 用例抓出来的；教训是**单测覆盖参数校验分支，不覆盖真实尺寸的组合数学**。

**验证**

- 前置条件 5 例（`export_view_test.rs`）：无预览矩形拒绝、可滚动内容受理、不足一屏委托视口、段数超限拒绝、高度超限拒绝。
- `stitch` 单测 5 例（`viewport_capture.rs` 内）：无重叠拼接、重叠行丢弃、单段恒等、非法参数、超限 TooLarge。
- headless e2e 1 例（`long_capture_e2e_stitches_segments_into_one_png`）：注入 `Event::Screenshot` 驱动真实帧循环，几何设定（预览区 100×300、内容高 800 → 偏移 [0,300,500]、末段重叠 100 行）下断言成品 100×800——若末段重叠行未丢弃会是 900，此断言钉住重叠丢弃逻辑。
- 全量验证链：`cargo fmt --check` 零差异；`clippy --workspace --all-targets -- -D warnings` 0 告警；`check-arch` OK；`cargo test --all-targets -- --skip large_full_text_find_is_bounded` **1263 passed / 0 failed**（14 个测试二进制）。

**已知边界**

- 长图在**可见视口宽度**内截取：超出预览区宽度的内容（若预览自身出现横向滚动）不在范围内——这是视口截图回路的固有能力边界。
- 滚动捕获期间预览区持续滚动（每段约 2–3 帧），用户可见；256 段上限内的耗时约几秒到十几秒。
- 像素保真度仍属 Stage 09 真机目测范畴（headless 注入的是假像素）。
