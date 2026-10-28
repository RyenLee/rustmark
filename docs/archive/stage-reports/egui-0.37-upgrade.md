# egui 0.33.3 → 0.37.0 依赖升级报告

> 写作日期：2026-10-16 ｜ 动因：验证新版 `egui` 能否改善 / 解决 `TextEdit` 在**输入、长文本渲染、高频重绘**时的卡顿
> 状态：**升级已完成并交付**（四道门禁全绿，见 §6）。本文随后进入归档区，其「当前 / 待办」表述只在写作日期成立。
> 数字来源：本文 §3 的实测数字已登记进 [`docs/spec/metrics.md`](../../spec/metrics.md) §3.2（唯一数字来源）；版本事实已登记进 [`docs/guides/dependencies.md`](../../guides/dependencies.md) §1。

---

## 0. 结论摘要

| 问题 | 结论 |
|---|---|
| 升级是否完成 | ✅ egui / eframe / epaint / egui-wgpu / egui-winit `0.33.3 → 0.37.0`，wgpu `27.0.1 → 30.0.1`（跨 4 个次版本） |
| 是否改善 TextEdit 卡顿 | **部分改善**：大文档文本选择、GalleyCache 驱逐、重绘爆发、TextEdit 布局偏移等**卡顿直接成因在 0.34~0.37 被逐个修掉**（§3.2）；但**超长单行排版回退约 5×**（§3.3），属文本栈换代（`ab_glyph` → `harfrust`+`skrifa`+`vello_cpu`）的代价 |
| 升级是否「纯换版本」 | ❌ 不是。暴露并修复 **1 处真实渲染缺陷**、修复 **1 处布局缺陷（问题 044）**、适配 **2 处 egui 行为变更**（§5） |
| 遗留 | 尚无「本应用侧」的卡顿根因被本次升级解决——那部分不在 egui 侧（§3.4） |

---

## 1. 各依赖升级前后的版本与兼容性注意事项

### 1.1 版本对照

| crate | 升级前 | 升级后 | 说明 |
|---|---|---|---|
| `egui` | 0.33.3 | **0.37.0** | UI 主库；API 破坏面最大 |
| `eframe` | 0.33.3 | **0.37.0** | 窗口 / 渲染框架；`App` trait 重组 |
| `epaint` | 0.33.3 | **0.37.0** | 被动升级；**文本栈整换代** |
| `egui-wgpu` | 0.33.3 | **0.37.0** | 被动升级 |
| `egui-winit` | 0.33.3 | **0.37.0** | 被动升级 |
| `wgpu` | 27.0.1 | **30.0.1** | 三平台（dx12 / metal / vulkan）声明同步提升 |
| `winit` | 0.30.13 | 0.30.13 | **未变** |
| （新链入）`harfrust` / `skrifa` / `vello_cpu` | — | 0.12.0 / 0.44.0 / 0.2.0 | 取代 `ab_glyph`：整形（harfrust）+ 字形解析（skrifa）+ CPU 光栅化（vello_cpu） |

### 1.2 兼容性注意事项（4 条，均已在 `Cargo.toml` 注释中固化）

1. **eframe 的 `wgpu` feature 语义变了**：0.33 用 `"wgpu"`；0.37 里它**连带**打开 `egui-wgpu/default` → `wgpu/default`（各平台后端全开，体积回到 DEF-03 之前的水平）。必须改用 **`wgpu_no_default_features`**，各平台后端仍由下方 `wgpu` 直接声明按平台各开一个。
2. **egui 必须新增 `monochrome_emoji_fonts`**：0.37 把 emoji 字体从 `default_fonts` 拆了出去。只开 `default_fonts` 会缺 ✅❌⚠💡🔗☑ 等码位，本项目符号面板 / 工具栏图标直接变豆腐块（实测 6 个用例失败）。
3. **`monochrome_emoji_fonts` 是 egui-only 的 feature**：照抄到 `eframe` 会直接 `failed to select a version for eframe`（eframe 无此 feature）。因 feature 可加性，只加在 egui 上即可。
4. **文本栈换代有观感与性能后果**：0.34 换 `ab_glyph → skrifa + vello_cpu`（并首次支持字体 hinting），0.35 再引入 `harfrust` 做字距 / 连字。字形整形**更准但更贵**，且**排版结果会与 0.33 有像素级差异**——这是 §3.3 超长单行回退的直接来源，也是任何「字符宽度 / 折行位置」断言的潜在影响源。

---

## 2. 受影响的 API 变更清单及对应代码位置

### 2.1 框架与上下文

| # | 0.33 用法 | 0.37 用法 | 代码位置 |
|---|---|---|---|
| 1 | `impl eframe::App { fn update(&mut self, ctx, frame) }` | `fn ui(&mut self, ui, frame)` + 可选 `fn logic(ctx, frame)`（**`update` 已移除**） | `src/app/frame.rs`（唯一实现点）+ 所有测试驱动点 |
| 2 | `ctx.run(input, \|ctx\| {})` | `ctx.run_ui(input, impl FnMut(&mut Ui)) -> FullOutput` | 全部无头驱动 |
| 3 | `ctx.run_logic(...)`（会初始化字体） | `run_logic` **不再初始化字体**（`fonts_mut` 会 panic：「No fonts available until first call to Context::run()」） | `src/infrastructure/fonts.rs`（4 处）、`src/presentation/theme/mapper/tests.rs`（3 处）——改用 `run_ui` |
| 4 | `ctx.style()` / `ctx.set_style(s)` | `ctx.style_of(theme)` / `ctx.set_style_of(theme, s)` + `ctx.set_theme(theme)` | `tests/presentation/theme_mapper_test.rs`（9 处）、`main_view` 主题装配 |
| 5 | `Id::new(x)` / `ui.id()` | `Id::unique(x)` / `ui.scope_id()` | `src/presentation/views/toolbar_view.rs`（4 处）+ 测试 14 处 |

### 2.2 面板与滚动

| # | 0.33 用法 | 0.37 用法 | 代码位置 |
|---|---|---|---|
| 6 | `SidePanel::left(id)` / `TopBottomPanel::top(id)`；`CentralPanel::default().show(ctx, ..)` | `egui::Panel::left/right/top/bottom(id_salt)`，统一 `Panel::show(ui, ..)`；`CentralPanel::show(ui, ..)`（**收 `ui` 而非 `ctx`**） | `src/presentation/views/{toolbar_view, sidebar_view, main_view, status_bar_view}.rs`、`tests/infrastructure/viewport_capture_test.rs` 等 |
| 7 | `exact_width(f)` / `exact_height(f)` | `exact_size(f)` | 同上（顶栏 / 侧栏 / 状态栏） |
| 8 | `ScrollArea::State.offset`（可读写字段） | **私有**：读 `clamped_offset()` / `unclamped_offset()`，写 `.scroll_offset(Vec2)` | `src/presentation/views/toolbar_view.rs`（标签条 offset 的唯一写者） |

### 2.3 文本与光标类型

| # | 0.33 用法 | 0.37 用法 | 代码位置 |
|---|---|---|---|
| 9 | `CCursor.index: usize` | `CCursor.index: CharIndex(pub usize)`（**语义仍是字符下标**，与 `chars()` 配对） | `src/app/frame/editor/ime.rs`、`toolbar_view` 无关处 |
| 10 | `LayoutSection.byte_range: Range<usize>` | `Range<ByteIndex>` | 高亮 LayoutJob 构造 |
| 11 | `LayoutJob` 允许「预填 `text` + `append` 段落」 | 新增 `debug_sanity_check()`（`Fonts::layout_job` 每次调用）：sections 必须从 byte 0 起、无空隙覆盖全文 | `src/presentation/theme/mapper/highlight_cache.rs`（见 §5.1） |
| 12 | `FullOutput` 无约束 | `FullOutput.textures_delta` 的 `Drop` 会 `debug_assert!(is_empty())`——无头测试必须清空 | 统一收口到 `tests/support/mod.rs::run_ui` |

### 2.4 事件与输入

| # | 0.33 用法 | 0.37 用法 | 代码位置 |
|---|---|---|---|
| 13 | `Event::Ime(ImeEvent::Preedit(String))` | `Preedit { text, active_range_chars: Option<Range<usize>> }`（**struct 变体**） | `src/app/frame/editor/ime.rs`、`tests/presentation/enter_ime_test.rs` |
| 14 | `ImeEvent::Enabled` / `Disabled` | **已废弃、egui 不再消费**（IME 开关由 egui 自管）；新增 `DeleteSurrounding { before_chars, after_chars }` | 同上（保留补发 `Disabled` 的意图 + `#[allow(deprecated)]`） |
| 15 | `DroppedFile` 结构体（`path` 字段） | **trait**（`fn path(&self)`、`fn bytes(&self)`）；`RawInput.dropped_files: Vec<Arc<dyn DroppedFile + Send + Sync>>` | `tests/integration/drag_open_test.rs`（新增 `TestDroppedFile`） |
| 16 | `Event::Screenshot { .. }` / `egui::UserData` | 均**移除**：`ctx.request_screenshot(cb)` → 命令以 `ViewportCommand::Screenshot(ScreenshotCallback)` 出现在 `FullOutput.viewport_output[ROOT].commands`，`cb.complete(Arc<ColorImage>)` 兑现 | `tests/presentation/export_view_test.rs`、`src/app/frame/capture.rs` |
| 17 | `Event::MouseWheel { delta, .. }` | 新增必填 `phase: TouchPhase` + `source: MouseWheelSource`（**两者都没实现 `Default`**，须具名变体） | 滚动类用例 |
| 18 | `epaint::ClippedShape { clip_rect, shape }` | 新增第三字段 `transform_after_tessellation` → 用 `ClippedShape::new(clip_rect, shape)` | `tests/presentation/sidebar_boundary_test.rs` 等 |

### 2.5 两处**行为**变更（不是 API 变更，但会静默出 bug）

| # | 0.37 的行为差异 | 本项目应对 | 代码位置 |
|---|---|---|---|
| 19 | `ImeEvent::Commit("")` 不再等价于「折叠选区」：0.33 的空串分支只折叠光标、**不碰文本**；0.37 改为先 `delete_selected(cursor_range)` 再插入 `commit_text`——组合期 `cursor_range` 正是预编辑区间，空串于是把预编辑串**真删掉**（实测 `"helloni"` + 回车 → `"hello\n"`） | 回提交**与预编辑串同一段文本**（delete 与 insert 相抵 = 折叠选区而文本不变）；串的内容与位置从 TextEdit 状态读回（`composition_text()`） | `src/app/frame/editor/ime.rs` |
| 20 | 0.33 在 `ime_enabled` 时会把 IME 事件**稳定排序到最前**（`events.sort_by_key(\|e\| !matches!(e, Event::Ime(_)))`），旧代码注释据此写下「IME 事件必然先于按键处理」；0.37 **删掉了这个排序**，改为按列表顺序就地处理 | 补发的 `Commit` 显式 `events.insert(0, ..)`，否则回车会先执行、照样吃掉预编辑串 | 同上 |

> 另附一条 0.37 的新概念：`Memory::owns_ime_events(id)`（实现为 `has_focus(id)`）。只有聚焦的 `TextEdit` 才会消费 `Event::Ime`；未聚焦时 `cursor_purpose` 被重置为 `Selection`、选区折叠。

---

## 3. TextEdit 卡顿的可能成因，以及目标版本是否包含针对性改进

### 3.1 先分清「谁的锅」

`TextEdit` 的一次击键要经过四段，只有第 ③ ④ 段在 egui 侧：

| 段 | 内容 | 归属 |
|---|---|---|
| ① | 领域编辑命令 + 全文 diff（`editor_view` 的 diff 路径） | **本应用** |
| ② | 语法高亮 span 生成（syntect，全量或防抖） | **本应用 / 依赖 syntect** |
| ③ | 逐行 `LayoutJob` → `Galley`（排版、折行、字形整形与光栅化） | **egui / epaint** |
| ④ | 每帧 UI 逻辑 + 绘制 + 重绘调度 | **egui** |

### 3.2 0.34~0.37 中**针对卡顿**的改进（逐条对照 changelog）

| 卡顿成因 | 对应改动 | 版本 |
|---|---|---|
| 大文档**文本选择**慢 | *Optimize text selection performance for large documents* #7917 | 0.34.2 |
| **词边界扫描 O(n²)**（双击选词 / Ctrl+←→ 的词跳转） | *Add regression test for O(n²) word boundary scan* #8077（先修后加回归测试） | 0.34.2 |
| 字形缓存 / 光栅化 | *Replace `ab_glyph` with Skrifa + vello_cpu；enable font hinting* #7694 | 0.34.0 |
| 字距与连字质量（间接影响整形耗时） | *Use `harfrust` for better kerning and ligatures* #8031 | 0.35.0 |
| 字体变体下的字形缓存失效 | *Fix glyph caching on font variations* #8189 | 0.35.0 |
| **每帧 `Response` 体积** | *Shrink the byte-size of `Response` slightly* #8011 | 0.34.0 |
| **`GalleyCache` 跨 viewport 驱逐**（多窗口 / 多 viewport 反复重排） | *Avoid cross-viewport GalleyCache eviction* #8528 | 0.37.0 |
| **`request_repaint_after` 截止期前的额外重绘爆发**（高频重绘） | *Fix burst of extra repaints before `request_repaint_after` deadlines* #8595 | 0.37.0 |
| **TextEdit 布局偏移 / 未裁剪 TextEdit 撑高** | *Fix `TextEdit` layout shift with custom margin; unclipped `TextEdit` grows to fit text* #8628 | 0.37.0 |
| 字体设置延迟生效导致的额外重排 | *Apply `set_fonts`/`add_font` right away when no font is loaded yet…* #8602 / #8609 | 0.37.0 |
| **CJK 行首避头尾标点**（本项目是 CJK 主场景） | *Don't start a CJK row with full-width closing punctuation* #8658 | 0.37.0 |
| 编辑类交互补强 | `TextEdit::event_filter` #8530（0.36.2）、双击拖拽 / 三击拖拽选择 #8370（0.37.0） | — |

### 3.3 本次实测（release，2026-10-16）

命令：`cargo test --release --test presentation -- --nocapture measure_`（3/3 通过）。括号内为 `docs/spec/metrics.md` §3.2 的历史值。

| 场景 | 本次实测 | 对比历史 | 判定 |
|---|---|---|---|
| 1MB 降级 · 冷首帧 | 19.23 ms | (47.9 ms) | ✅ 明显改善 |
| 1MB 降级 · 稳态滚动帧 | p50 0.27 / p95 **0.73** ms | (p50 0.26 / p95 1.19 ms) | ✅ 持平偏优，远优于 16 ms 目标 |
| 10KB 全文高亮 · 排版（增量，改 1 行） | 冷 2.07 ms ｜ 增量 p50 **0.07** / p95 0.11 ms | (冷 0.75 / 增量 p50 0.16 / p95 0.51 ms) | ✅ 增量帧更好（重建行数 == 1） |
| 10KB 全文高亮 · **span 生成**（syntect） | p50 **58.72** / p95 86.97 ms | (p50 56.5 / p95 59.1 ms) | ⚠️ 持平（这是 syntect 侧，与 egui 无关；历史即已超 NFR-PERF-05 五倍） |
| **超长单行**（27 万字节 / 无换行）· 冷排版 | **69.06 ms** | (13.5 ms) | ⚠️ **回退 ~5×** |
| 超长单行 · 改一字符（整行重排） | p50 **66.65** / p95 **77.51** ms | (p50 13.5 / p95 14.5 ms) | ⚠️ **回退 ~5×**，但距 500 ms「卡死」线仍有 6.4× 余量（历史 34×） |

**对 §3.3 末两项的归因**：该行无换行、近 9 万 CJK 字符，成本全落在 egui/epaint 的**折行 + 字形整形**上；0.34 换代到 `skrifa`+`vello_cpu`、0.35 引入 `harfrust` 之后整形更精细，代价即体现在这类极端输入上（正常文档无此规模的单行）。**结论：不需要行级截断高亮**（08 文档列出的备选方案仍未触发红线），但余量已从 34× 缩到 6.4×，若未来出现更大单行需重估。

### 3.4 升级**没有**解决的部分（仍属本应用侧）

以下成因不在 egui 侧，换版本不会改善，需按项目既有规则排查（`docs/guides/debug-guide.md`、`docs/spec/metrics.md` §3.2 归因表、以及本仓库 043 号每帧热路径规则）：

1. **每帧取打开文档列表走完整视图**（`list_open` 的全文快照 + 行数扫描）——必须走 `open_document_list()` 轻量摘要。
2. **高亮 LRU 驱逐策略**——误用任意迭代序驱逐会让来回切标签触发 56 ms 级 syntect 重算尖峰。
3. **syntect 全量高亮**——已由 OPT-011 ① 防抖压到「每 150 ms 至多一次」，打字期 ≈ 0.00 ms；进一步只能走 ② 增量高亮（需扩领域接口）。
4. **状态栏每帧查询**——`get_stats` 必须按 `(id, revision)` 缓存，漏 revision 会「数字滞后」。
5. **`highlight_layout` 的缓存命中判据含全文 `cached_text == text` 比较**（O(n) memcmp，1MB 约几十 µs 量级）——可接受，但若将来文档规模再上一档，可改为 (revision, len, 指纹) 组合键。

---

## 4. 若升级后卡顿仍未解决：后续可尝试的排查与优化方向

按「先测再改」排序，任一条都先取数字再动手：

1. **先分清段**：在 `highlight_layout_with_find` 内分别计时「span 生成 / `build_line_job` / `ctx.fonts_mut(layout_job)`」三段，得到 ③ 段真实占比——本次数据显示 ③ 段在超长单行上占了绝对多数（69 ms 里大头是排版）。
2. **若瓶颈在 epaint 排版（③）**：
   - 超长单行改走 `layout_no_wrap`（跳过折行计算）对照实验；本项目编辑区的折行开关已存在。
   - 评估**行级截断高亮**（08 文档备选方案）：对超阈值单行只高亮可视片段，代价是横向滚动时的高亮不完整。
   - 检查自定义 `layouter` 是否每帧都被调用（本项目整文档缓存命中时为 O(1)，但命中判据本身是 O(n) 全文比较）。
3. **若瓶颈在 syntect（②）**：先沿用 OPT-011 ① 防抖并调窗口；仍不够则执行 OPT-011 ②「增量高亮」（扩领域接口，改动最大）。
4. **若瓶颈在应用侧（①）**：按 043 号规则逐项体检（每帧路径上是否存在 O(全文) 的 DTO 构造），并用 `stats_cache_hits` 之类的计数器确认缓存真的命中。
5. **若是高频重绘**：0.37 修掉了「截止期前的额外重绘爆发」（#8595），但前提是应用侧不滥用 `request_repaint()`；用 `ctx.request_repaint_after` 替代无条件 `request_repaint`，并检查动画类逻辑（0.35 起动画时长从 0.1 s 放慢到 0.2 s，会影响滚动动画的观感）。
6. **若是 IME 输入路径**：组合期本项目每帧都要扫一遍输入事件、并读一次 TextEdit 状态（`composition_char_range`）；若日后测得开销可观，可改为「只在有 IME 事件或守卫生效时才读状态」。
7. **若怀疑字形整形**：对超长单行做 `harfrust` 开关对照（0.35 起为默认），确认回退是否来自整形而不是折行。
8. **回归守卫**：本次已把「不卡死」阈值按编译档位分开（debug 1500 ms / release 500 ms，理由见 §6.2）。若将来引入行级截断，须同步更新 `docs/spec/metrics.md` §3.2 与 `tests/presentation/highlight_perf_test.rs`。

---

## 5. 升级中暴露并修复的真实缺陷

### 5.1 `build_line_job` 把整行文本写了两遍（**真实渲染缺陷**）

- **现象**：0.37 起大量高亮单测 panic：`First LayoutSection must start at byte 0` / `Last LayoutSection must end at the end of the text`。
- **根因**：`LayoutJob` 初始化时预填了 `text: line.to_string()`，随后又 `job.append(&line[lo..hi], ..)` —— 文本被写了两遍（诊断打印：`line="the cat sat on the mat" len=22 bounds=[0, 22] sections=[(22, 44)]`）。0.33 **只排版 append 出来的 section**，故这个 bug 一直不可见；0.37 新增了 `LayoutJob::debug_sanity_check()`（在 `Fonts::layout_job` 内每次调用），要求 sections 从字节 0 起、无空隙覆盖全文，于是把它暴露出来。
- **修复**：删掉预填的 `text`，空行仍由既有 `if job.sections.is_empty() { job.append(line, ..) }` 兜底。
- **位置**：`src/presentation/theme/mapper/highlight_cache.rs`

### 5.2 标签条垂直基准负偏移（问题 044）

- **现象**：`tab_strip_test` 4 个用例失败——标签文字中心 y=66，而「全部关闭」图标中心 y=64.5。
- **根因**：标签行高 `TAB_ROW_H`(24) 常大于可用条带高 `strip_h`（实测 17~19，被菜单栏预算挤压），此时「在条带内居中」算出的是**负**偏移（`(strip_h - TAB_ROW_H)/2 < 0`）：导航箭头与「全部关闭」整体上移，而标签行由 `ScrollArea` 贴顶排布，两者不再同一中线。
- **修复**：引入统一垂直基准 `row_top = strip_top + max(0, (strip_h - TAB_ROW_H)/2)`，标签行、导航箭头、全部关闭三处共用（条带够高时居中，不够时与标签一样贴顶）。
- **位置**：`src/presentation/views/toolbar_view.rs`

### 5.3 IME：组合期折叠语义与事件排序（§2.5）

- **现象**：4 个组合期用例失败——预编辑串被吞（`"helloni"` + 回车 → `"hello\n"`，`"基线sheng"` → `"基线\n"`）。
- **根因与修复**：见 §2.5 第 19/20 条。
- **位置**：`src/app/frame/editor/ime.rs`（新增 `composition_char_range` / `composition_text`）

---

## 6. 验证与门禁

### 6.1 四道门禁（2026-10-16 实测）

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` | ✅ 通过 |
| `cargo run --bin check-arch` | ✅ `check-arch: OK（未发现分层依赖违规）` |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 通过（EXIT=0） |
| `cargo test --all-targets` | ✅ **1722 passed / 0 failed / 2 ignored** |

各目标明细：lib 346 ｜ app 67 ｜ application 153 ｜ arch_violation 14 ｜ domain 193 ｜ e2e 16 ｜ infrastructure 294（1 ignored）｜ integration 79（1 ignored）｜ presentation 522 ｜ 其余 38。

### 6.2 测试基础设施的两处必要改造

1. **`tests/support/mod.rs::run_ui`**：统一清空 `FullOutput.textures_delta`（0.37 的 `Drop` 会在未 apply 增量时 `debug_assert` 崩），全部 80 处无头驱动收口于此。
2. **`tests/support/mod.rs::settle`**：egui 0.37 的**首帧是测量 pass**（实测菜单栏首帧占 43 pt、之后恒为 44 pt → 标签条 `strip_top` 在 54/56 间漂移 2 pt）。渲染类断言若从不同帧取证，几何会差 1~2 pt，直接导致「三条顶栏同一水平线」「标签同一行」这类 1 pt 容差断言稳定误报。取画面之前先空跑一帧即稳定。
3. 另有 2 处断言口径修正（属**测试自身的脆弱假设**，非产品缺陷）：
   - `tab_strip_test::close_all_stays_visible_when_tabs_overflow` 原钉 `doc0` 必须可见——但活动标签变化时会自动滚入视野，溢出场景下最左的 `doc0` 本就该滚出视口；旧写法能过只是因为被测帧恰好是首帧（reveal 的写回要到下一帧才生效）。改为「至少一个标签与按钮同行可见，且都在按钮左侧」。
   - `top_menu_test::three_pane_top_bars_align` 原用「`文件`」作侧栏锚点——但该文案在侧栏出现**两次**（035 起的活动栏导航项 + 内容面板作用域标题），`find` 取到的是前者，而活动栏是纵向导航列、不是顶栏。加 `left() > 60` 限定后取到真正的内容面板标题行（实测三条顶栏文字中心均为 y=119）。
   - `highlight_perf_test` 的「不卡死」阈值改为**按编译档位**给：本文件顶部即规定数字必须 release 下取，而日常门禁 `cargo test --all-targets` 跑的是 debug（同一段排版 debug/release 差约 8.4×，实测 p95 650.5 ms vs 77.5 ms）。release 500 ms、debug 1500 ms；**达标结论一律以 release 数字为准**。

### 6.3 冒烟验证

`cargo build --bin rustmark` 后用独立配置目录实启（`APPDATA` 指向临时目录，避免污染用户配置）并跑满 15 s：

```
[startup-timing] 冷启动里程碑：装配完成=20.3ms → 窗口就绪=854.0ms → 应用构造完成=854.8ms → 首帧提交=949.4ms ｜ 合计（到 首帧提交）=949.4ms
```

进程在 15 s 内无 panic、无提前退出（由 `timeout` 强杀，退出码 137），**窗口成功建立并提交首帧**。debug 档冷启动 949 ms 中，854 ms 是 wgpu 适配器 / 设备初始化（一次性成本，release 下单次实测约 209.7 ms 量级见 metrics §3.2）。
