# UI State 字段审计报告（V11-59）

- 日期：2026-10-24
- 依据：`docs/core-refactor-tasklist-2026-10-24.md` V11-59（组 C 前置）
- 范围：`crates/rustmark-core/src/presentation/state/` 全部 24 个文件
- 覆盖基准：**39 个结构体 / 201 个字段 / 0 未解析**（机械清单，见 §7 复现命令）

---

## 1. 结论摘要

对全部 201 个字段逐一提四问（① 跨帧保持？② 可被单测断言？③ 经 domain/preferences 持久化？④ 被 application 消费？），结论分布：

| 结论 | 字段数 | 说明 |
|---|---|---|
| **K 留 core state** | **201** | 全部字段维持现状 |
| V 挪 view-local（egui memory / 局部变量） | 0 | 见 §4.2「为什么零 V」 |
| A 挪 application | 0 | 结构性不可能，见 §3.2 |

**三条结构性事实**（均有实测取证，§3）：

1. state 结构体**零 serde**——没有任何字段经 state 自身序列化落盘；持久化一律走「领域 `EditorSettings` 等 ↔ 启动播种 / 实时写领域」的旁路。
2. application 层对 `presentation::state` **零引用**（grep 实证 + check-arch 单向依赖守卫）——UI 状态不泄漏进应用层，也不该泄漏。
3. `tests/field_visibility_guard_test.rs` 白名单 **168 条 = 全部 `pub` 字段**（0 缺失 / 0 多余）；未入名单的 33 条恰为 15 个 `pub(crate)` + 18 个 private——守卫的辖域精确等于「跨 crate 可见字段」。

**一个真实缺陷**（§6 F-1）：`EditorUiState::word_wrap` / `spell_check` 双路写都不完整——设置页只写领域（当前会话不生效、重启不回读），状态栏点击只翻会话状态（不落盘、重启丢失），且两条路互相矛盾。对照 `line_numbers` 的完整范式（播种 + 即时生效 + 落盘 + 取消回滚四处齐全）即可定位修法。

三个观察项（§6 O-1~O-3）：会话级偏好不落盘是设计已声明的（记录备查）；`only_changes` 等无回读路径；`export_ui.theme` 经验证无 staleness（每次打开浮层重新播种，**不是**缺陷）。

---

## 2. 审计方法与判据

### 2.1 四问定义

| 问 | 判据 |
|---|---|
| ① 跨帧保持？ | 该字段的值是否需要活过单帧（ egui 即时模式下「不存就没有下一帧」） |
| ② 可被单测断言？ | 存在内联测试或集成测试直接/经行为断言该字段（而非仅字段存在） |
| ③ 经 domain/preferences 持久化？ | 该字段的值是否经领域设置落盘并在启动时回读（**不是** state 自身 serde） |
| ④ 被 application 消费？ | application 层（`crates/rustmark-core/src/application/`）是否引用该字段，或其值是否作为 application 查询/命令的入参 |

### 2.2 三选一判定规则

- **K（留 core state）**：四问中 ① 为是，且所在结构体是表现层自治的视图状态——默认结论。
- **V（挪 view-local）**：① 为否（无需跨帧）**且**不存在「必须由应用持有的结构性理由」（被两个视图共享、需要帧间边沿判定、会被 egui 全局量污染等）。
- **A（挪 application）**：④ 为是**且**该字段承载的是业务语义而非视图意图。本审计中 A 恒不成立（§3.2）。

### 2.3 纪律遵循

- 「预检结论必须实测复核」：本审计两次实测推翻先验——(a) 白名单条数与 `pub` 字段数的关系是逐字段机器比对得出的，不是估计；(b) F-1 是追 `word_wrap` 写入方时发现的，任务清单预检未涵盖。
- 覆盖面用机械清单钉死：201 字段逐行打标，§4 表格行数合计 = 201。

---

## 3. 结构性事实

### 3.1 零 serde：持久化全部走旁路

`grep -E "serde|Serialize|Deserialize" state/` 为空。39 个结构体的 derive 只有 `Clone/Copy/Debug/Default/PartialEq/Eq` 家族。持久化的真实形态是**领域旁路**：

```
领域 EditorSettings（serde，落盘 config）
   │  EditorApp::new 播种（root，frame.rs:509-531）
   ▼
UI state 镜像字段（split_ratio / sidebar_width / sidebar_visible /
                  line_numbers —— 仅此 4 个 + theme + shortcuts）
   │  用户改动（设置页 / 拖拽 / 状态栏）
   ▼
persist_* / apply_editor_field 写回领域（实时落盘）
```

即：**state 字段本身永不序列化**，「③ 持久化」问的其实是「这个字段在不在上述旁路的名单里」。答案是：仅 §4 中标「经领域」的 **6** 个字段在名单里（`split_ratio` / `sidebar_width` / `sidebar_visible` / `line_numbers` / `theme` / `dispatcher`）；另有 12 个 `Snapshot` 字段是「取消回滚镜像」（打开时从领域快照、取消时回滚，自身不 serde）；其余 **183** 个均为会话级。

### 3.2 application 层零引用：A 结论结构性为空

`grep -rn "presentation::state" crates/rustmark-core/src/application/` 为**空**（依赖方向由 check-arch 机械守卫）。state 值进入 application 的唯一形态是**组合根（root `EditorApp`）把值作为查询/命令入参传进去**，本审计仅见两处：

- `app.editor.spell_check` → `SpellCheckQuery { enabled }`（`views/editor_view.rs:145`、`views/editor_view/spell.rs:54`）
- 写作目标：`StatsView::words` → `preferences.goal_progress(words)` → `GoalProgressView` **流入** `status_bar.goal`（方向是 application → state，与上一条相反）

没有字段承载业务语义，故 **A = 0** 不是疏漏而是架构正确的证据。

### 3.3 白名单 = pub 全集

机器比对（§7 命令 3）：201 字段中 `pub` 168 / `pub(crate)` 15 / private 18。`field_visibility_guard_test.rs` 的 168 条与 168 个 `pub` 字段**逐一相等**。33 个非 pub 字段（crate 内可见或私有）不在守卫辖域，与守卫设计意图（防跨 crate 直写）一致。**本次审计无字段落入「guard 该管没管」的缺口。**

---

## 4. 逐字段四问打标（201/201）

标记：① 跨帧 ② 单测断言 ③ 持久化 ④ application 消费。「经领域」= 经 §3.1 旁路持久化。

### 4.1 `app_ui_state.rs` — AppUiState（16）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `sidebar_visible` | ✓ | ✓ | 经领域（`EditorSettings::sidebar_visible`，`frame.rs:527` 播种 + `persist_sidebar_visibility` 每帧检测落盘） | ✗ | K。未决定过（`None`）按默认收起——「会话级」与「可持久化」在此字段上并存，靠领域 `Option` 三分（真/假/未决定） |
| `sidebar_tab` | ✓ | ✓ | ✗ | ✗ | K。会话级 |
| `preview_visible` | ✓ | ✓ | ✗ | ✗ | K。会话级 |
| `status_bar_visible` | ✓ | ✓ | ✗ | ✗ | K。会话级（S6-02 三开关独立） |
| `split_ratio` | ✓ | ✓ | 经领域（`frame.rs:509` 播种 + `persist_split_ratio`） | ✗ | K。拖过即落盘 |
| `split_dragging` | ✓ | ✓ | ✗（瞬态） | ✗ | K。单帧边沿检测：`true→false` 跳变时落盘一次 |
| `sidebar_width` | ✓ | ✓ | 经领域（`frame.rs:514` 播种 + `persist_sidebar_width`） | ✗ | K。存「用户意图」，渲染时二次动态夹紧 |
| `sidebar_dragging` | ✓ | ✓ | ✗（瞬态） | ✗ | K。与 `split_dragging` 对称但目的相反：拖拽中**不**落盘 |
| `focus_mode` | ✓ | ✓ | ✗ | ✗ | K。退出只走 `CloseOverlay` 动作（B-03） |
| `active_tab` | ✓ | ✓ | ✗ | ✗ | K。会话级 |
| `tab_count` | ✓ | ✓ | ✗ | ✗ | K。会话级 |
| `tabstrip_scroll_x` | ✓ | ✓ | ✗ | ✗ | K。**不能挪 V**：egui `scroll_delta` 是全局量，任何视图 `scroll_to_rect` 都会写进下一个 begin 的 ScrollArea——问题 042 根因，必须应用持有并每帧纠正 |
| `tabstrip_last_active` | ✓ | ✓（`ui_state_writes_test`） | ✗ | ✗ | K。**不能挪 V**：放 egui temp memory 会每帧判成「变化」，reveal 逐帧撤销用户滚动 |
| `theme` | ✓ | ✓ | 经领域（`ThemeSettings`，启动 `resolve` 播种） | ✗ | K。切主题同步预览缓存签名 |
| `dispatcher` | ✓ | ✓（`shortcut_registry_test`） | 经领域（`from_settings` + 改键落盘） | ✗ | K。唯一动作表 |
| `sync` | ✓ | ✓（`scroll_sync_test` 41 例） | ✗ | ✗ | K。两视图共享的状态机，谁持有都不合适 |

### 4.2 `command_palette_ui_state.rs` — CommandRow（2）+ CommandPaletteUiState（2）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `CommandRow::entry` | ✗（每次开面板由动作表重建） | ✓ | ✗ | ✗ | K。直接包 `ActionEntry` 不复制字段——防漂移 |
| `CommandRow::hits` | ✗（同上） | ✓ | ✗ | ✗ | K。命中字符坐标，关键词命中时为空 |
| `query` | ✓ | ✓ | ✗ | ✗ | K。跨帧；`open()` 清空（语境每次不同） |
| `selected` | ✓ | ✓ | ✗ | ✗ | K。环绕语义 + `clamp` 防越界选中 |

### 4.3 `dialog_ui_state.rs` — DialogUiState（3，private）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `open` | ✓ | ✓ | ✗ | ✗ | K。栈而非单值：对话框可叠加 |
| `pending_close` | ✓ | ✓ | ✗ | ✗ | K。`Esc` 的跨帧仲裁标记（关闭浮层 vs 取消输入） |
| `focus_granted` | ✓ | ✓ | ✗ | ✗ | K。一次性闩锁：边沿触发防「每帧抢焦点」（导出文件名框打不进字的回归防线） |

### 4.4 `diff_ui_state.rs` + `model.rs` — DiffUiState（8）/ DiffRow（4）/ DiffStat（2）/ CharSeg（2）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `right_path` | ✓ | ✓ | ✗ | ✗ | K。保留选择让用户知道是哪个文件的问题 |
| `left_title` | ✓ | ✓ | ✗ | ✗ | K。仅显示用 |
| `right_title` | ✓ | ✓ | ✗ | ✗ | K。仅显示用 |
| `rows` | ✓（打开期间缓存） | ✓ | ✗ | ✗ | K。**刻意不缓存文件内容**（内容会被编辑区改掉，缓存必然过期且无提示）——缓存的是计算结果不是业务数据 |
| `stat` | ✓ | ✓ | ✗ | ✗ | K。行级统计（不受渲染合并影响） |
| `error` | ✓ | ✓ | ✗ | ✗ | K。读取失败原因 |
| `degraded` | ✓ | ✓ | ✗ | ✗ | K。规模超限必须明说 |
| `only_changes` | ✓ | ✓ | ✗（会话级，`open` 刻意不重置） | ✗ | K。用户查看偏好，非本次打开的一部分。未落盘是已声明设计（观察项 O-2） |
| `DiffRow::op` | ✗（`recompute` 重建） | ✓ | ✗ | ✗ | K。纯计算结果 |
| `DiffRow::text` | ✗（`recompute` 重建） | ✓ | ✗ | ✗ | K。纯计算结果 |
| `DiffRow::old_no` | ✗（`recompute` 重建） | ✓ | ✗ | ✗ | K。纯计算结果 |
| `DiffRow::new_no` | ✗（`recompute` 重建） | ✓ | ✗ | ✗ | K。纯计算结果 |
| `DiffStat::added` | ✓（随 `recompute` 更新后驻留） | ✓ | ✗ | ✗ | K。纯计算结果 |
| `DiffStat::removed` | ✓（随 `recompute` 更新后驻留） | ✓ | ✗ | ✗ | K。纯计算结果 |
| `CharSeg::kind` | ✗（`to_display_rows` 重建） | ✓ | ✗ | ✗ | K。字符级差异段 |
| `CharSeg::text` | ✗（`to_display_rows` 重建） | ✓ | ✗ | ✗ | K。字符级差异段 |

### 4.5 `editor_ui_state.rs` — EditorUiState（9）+ SpellSuggestMenu（6）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `cursor` | ✓ | ✓ | ✗ | ✗ | K。**范式注释**：撤销不该把光标弹回三行之前（B-07）——视图状态不放领域 |
| `anchor` | ✓ | ✓ | ✗ | ✗ | K。无选区为 `None` |
| `scroll_y` | ✓ | ✓ | ✗ | ✗ | K。会话级 |
| `scroll_x` | ✓ | ✓ | ✗ | ✗ | K。会话级 |
| `composing` | ✓ | ✓（`enter_ime_test`） | ✗ | ✗ | K。IME 组字预编辑文本跨帧；组字期间抑制撤销快照（FR-05 最高风险项） |
| `word_wrap` | ✓ | ✓ | **写而不读**（F-1） | ✗ | K + **F-1**。设置页写领域但启动不回读；状态栏点击翻会话但不落盘 |
| `line_numbers` | ✓ | ✓ | 经领域（**完整范式**：播种 `frame.rs:531` + 即时 `set_line_numbers` + 落盘 + 取消回滚 `settings.rs:207`） | ✗ | K。**四路齐全的对照基准**——F-1 的修法模板 |
| `spell_check` | ✓ | ✓ | **双路都不完整**（F-1） | **✓**（`SpellCheckQuery::enabled`） | K + **F-1**。唯一 ④ 为是的字段之一 |
| `spell_menu` | ✓ | ✓（`spell_menu_test`） | ✗ | ✗ | K。**不能挪 V**：菜单要跨帧存在，egui `Area` 是即时模式，不存状态就没有「下一帧还开着」 |
| `SpellSuggestMenu::byte_start` | ✓（随菜单跨帧） | ✓ | ✗ | ✗ | K。错词区间起点 |
| `SpellSuggestMenu::byte_end` | ✓（随菜单跨帧） | ✓ | ✗ | ✗ | K。错词区间终点 |
| `SpellSuggestMenu::word` | ✓（随菜单跨帧） | ✓ | ✗ | ✗ | K。被点错的词（候选替换的对象） |
| `SpellSuggestMenu::suggestions` | ✓（随菜单跨帧） | ✓ | ✗ | ✗ | K。候选列表（已截断到上限） |
| `SpellSuggestMenu::screen_x` | ✓（随菜单跨帧） | ✓ | ✗ | ✗ | K。屏幕坐标，来自指针位置 |
| `SpellSuggestMenu::screen_y` | ✓（随菜单跨帧） | ✓ | ✗ | ✗ | K。屏幕坐标，来自指针位置 |

### 4.6 `export_ui_state.rs` — ExportUiState（14）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `format` | ✓ | ✓ | ✗（会话级；硬边界禁缓存「上次导出选项」） | ✗ | K。打开时保留（连续导出多格式不必重选） |
| `include_toc` | ✓ | ✓ | ✗ | ✗ | K。切格式时按能力夹紧（防「选了 Markdown 还勾着目录」被领域拒） |
| `embed_images` | ✓ | ✓ | ✗ | ✗ | K。切格式时按能力夹紧 |
| `page_size` | ✓ | ✓ | ✗ | ✗ | K。切格式时按能力夹紧 |
| `paginate` | ✓ | ✓ | ✗ | ✗ | K。切格式时按能力夹紧 |
| `output_dir` | ✓ | ✓ | ✗ | ✗ | K。空目录时 `can_submit` 拦下（防文件落进看不见的 CWD） |
| `file_stem` | ✓ | ✓ | ✗ | ✗ | K。空目录时 `can_submit` 拦下 |
| `theme` | ✓ | ✓ | 构造时从 `ThemeKind::as_str()` 播种；每次打开浮层重新播种（`export.rs:38`）——**无 staleness，不是缺陷** | ✗ | K。走真源不硬编码 `"dark"` |
| `resolved_path` | ✓（派生缓存） | ✓ | ✗ | ✗ | K。避免每帧重复拼串 |
| `running` | ✓ | ✓ | ✗ | ✗ | K。跨帧（关对话框会复位，故与下一项分离） |
| `awaiting_capture` | ✓ | ✓ | ✗ | ✗ | K。**必须活到 GPU 回传兑现**：少了它，关掉再开对话框能再点一次导出，同一 `pending_capture` 被静默覆盖 |
| `long_image` | ✓ | ✓ | ✗ | ✗ | K。Image 专属，切走即清零 |
| `error` | ✓ | ✓ | ✗ | ✗ | K。与 `notice` 互斥（`begin/succeed/fail` 保证） |
| `notice` | ✓ | ✓ | ✗ | ✗ | K。与 `error` 互斥（`begin/succeed/fail` 保证） |

### 4.7 `find_replace_ui_state.rs` + `engine.rs` — FindOptions（3）/ FindReplaceUiState（12）/ Match（2）/ Finder（3，private）/ ReplaceSummary（3）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `FindOptions::case_sensitive` | ✓ | ✓ | ✗ | ✗ | K。三开关正交；变更清 `current`（旧下标指向旧模式第 N 项） |
| `FindOptions::regex` | ✓ | ✓ | ✗ | ✗ | K。三开关正交；变更清 `current` |
| `FindOptions::whole_word` | ✓ | ✓ | ✗ | ✗ | K。三开关正交；变更清 `current` |
| `find_text` | ✓ | ✓ | ✗ | ✗ | K。跨文档切换按 `keep_query_across_documents` 决定去留 |
| `replace_text` | ✓ | ✓ | ✗ | ✗ | K。跨文档切换按 `keep_query_across_documents` 决定去留 |
| `options` | ✓ | ✓ | ✗ | ✗ | K。同上 |
| `replace_open` | ✓ | ✓ | ✗ | ✗ | K。`Ctrl+F` / `Ctrl+H` 共用入口 |
| `current` | ✓ | ✓ | ✗ | ✗ | K。当前命中下标；跨文档必清（偏移属于旧文档） |
| `finder` | ✓ | ✓ | ✗ | ✗ | K。**派生缓存**：编译后模式 + 命中列表拆开存只会多一份可能不同步的副本；`rescan` 是唯一写入口 |
| `error` | ✓ | ✓ | ✗ | ✗ | K。正则无效时**不**保留上次 finder（否则「下一个」跳到与输入无关的位置） |
| `find_focused` | ✓ | ✓ | ✗ | ✗ | K。打开即聚焦的边沿标记（查找期间编辑区仍可编辑，A-28） |
| `last_doc_id` | ✓ | ✓ | ✗ | ✗ | K。跨文档切换判据 |
| `keep_query_across_documents` | ✓ | ✓ | ✗（定义已写进模块注释：保留查询与选项、重置命中选中） | ✗ | K。验收要求「明确定义并写进文档」已满足 |
| `last_replace_count` | ✓ | ✓ | ✗ | ✗ | K。`aa→a` 无限增生必须让用户看见 |
| `last_replace_grows` | ✓ | ✓ | ✗ | ✗ | K。同上（替换结果是增是减） |
| `Match::start` | ✗（随 `finder` 重建） | ✓ | ✗ | ✗ | K。字节区间起点 |
| `Match::end` | ✗（随 `finder` 重建） | ✓ | ✗ | ✗ | K。字节区间终点 |
| `Finder::pattern` | ✓（随 `finder` 缓存跨帧） | ✓（经 `scan`/`step` 行为） | ✗ | ✗ | K。私有，仅 crate 内可见 |
| `Finder::options` | ✓（随 `finder` 缓存跨帧） | ✓（经 `scan`/`step` 行为） | ✗ | ✗ | K。私有，仅 crate 内可见 |
| `Finder::matches` | ✓（随 `finder` 缓存跨帧） | ✓（经 `scan`/`step` 行为） | ✗ | ✗ | K。私有，仅 crate 内可见 |
| `ReplaceSummary::result` | ✗（一次性预览产物，调用即弃） | ✓ | ✗ | ✗ | K。替换后的文本 |
| `ReplaceSummary::count` | ✗（一次性预览产物，调用即弃） | ✓ | ✗ | ✗ | K。替换处数 |
| `ReplaceSummary::will_match_again` | ✗（一次性预览产物，调用即弃） | ✓ | ✗ | ✗ | K。用**原模式**扫新文本判定 |

### 4.8 `preview_ui_state.rs` — PreviewCacheKey（4）+ PreviewUiState（8）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `PreviewCacheKey::doc_id` | ✓ | ✓ | ✗（由领域 `revision` 派生，键本身不落盘） | ✗ | K。**必须进键且优先于版本号**：版本号是每份文档各自计数器，两份未编辑文档天然相同 |
| `PreviewCacheKey::content_version` | ✓ | ✓ | ✗ | ✗ | K。同上 |
| `PreviewCacheKey::palette` | ✓ | ✓ | ✗ | ✗ | K。改配色不重建 → 字号/主题设置看起来无效 |
| `PreviewCacheKey::font_size_q` | ✓ | ✓ | ✗ | ✗ | K。整数化避免浮点抖动误判 |
| `scroll_y` | ✓ | ✓ | ✗ | ✗ | K。会话级 |
| `last_key` | ✓ | ✓（`preview_debounce_test`） | ✗ | ✗ | K。上次完成渲染的键 |
| `last_doc` | ✓ | ✓ | ✗ | ✗ | K。**不复用 `last_key.doc_id`**：`invalidate` 会清空 `last_key`，那一刻靠它判换文档会漏判 |
| `last_edit_at_ms` | ✓ | ✓ | ✗ | ✗ | K。防抖计时起点（编辑时打戳，不是渲染时——否则退化成节流） |
| `blocks` | ✓（缓存到键失效，S6-05） | ✓ | ✗ | ✗（application **产出** DTO 流入 state，反向无） | K。存 DTO 视图而非 `Galley`（立即模式每帧重发绘制指令） |
| `rects` | ✗（**每帧重新采集**，注释明言不跨帧复用） | ✓ | ✗ | ✗ | K。大图片解码完成那帧用旧矩形会跳错块 |
| `render_count` | ✓ | ✓（`preview_debounce_test` 实测防抖生效） | ✗ | ✗ | K。调试观察量 |
| `degraded` | ✓ | ✓ | ✗ | ✗ | K。>500KB 只渲染可视区（FR-06） |

### 4.9 `quick_open_ui_state.rs` — QuickOpenItem（3）+ QuickOpenUiState（2）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `QuickOpenItem::path` | ✗（每次打开由工作区条目 + 最近文件投影重建） | ✓ | ✗ | ✗ | K。**不重新扫盘**——与文件树同源防漂移 |
| `QuickOpenItem::name` | ✗（每次打开由工作区条目 + 最近文件投影重建） | ✓ | ✗ | ✗ | K。**不重新扫盘**——与文件树同源防漂移 |
| `QuickOpenItem::group` | ✗（每次打开由工作区条目 + 最近文件投影重建） | ✓ | ✗ | ✗ | K。**不重新扫盘**——与文件树同源防漂移 |
| `query` | ✓ | ✓ | ✗ | ✗ | K。`open()` 清空 |
| `selected` | ✓ | ✓ | ✗ | ✗ | K。环绕 + `clamp` |

### 4.10 `scroll_sync_state.rs` — PaneGeometry（2）/ BlockRect（3）/ ScrollSync（9）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `PaneGeometry::content_h` | ✓（每帧写入、跨帧比较） | ✓ | ✗ | ✗ | K。滚动数学的输入 |
| `PaneGeometry::viewport_h` | ✓（每帧写入、跨帧比较） | ✓ | ✗ | ✗ | K。滚动数学的输入 |
| `BlockRect::id` | ✗（每帧采集） | ✓ | ✗ | ✗ | K。同步换算依据 |
| `BlockRect::top` | ✗（每帧采集） | ✓ | ✗ | ✗ | K。同步换算依据 |
| `BlockRect::bottom` | ✗（每帧采集） | ✓ | ✗ | ✗ | K。同步换算依据 |
| `ScrollSync::owner` | ✓ | ✓（41 例） | ✗ | ✗ | K。单一主动方破反馈环 |
| `owner_until_ms` | ✓ | ✓ | ✗ | ✗ | K。时间窗口对夹紧免疫（比对写入值会误判回声） |
| `last` | ✓ | ✓ | ✗ | ✗ | K。上帧两侧偏移；`reset` 必清（否则开新文件预览自己滚一下） |
| `geom` | ✓ | ✓ | ✗ | ✗ | K。两侧几何量 |
| `editor_text_top` | ✓ | ✓ | ✗ | ✗ | K。Galley→内容坐标系加数；`reset` **刻意不清**（换文档不改 frame 内边距） |
| `enabled` | ✓ | ✓ | ✗ | ✗ | K。同步总开关 |
| `pending_click` | ✓ | ✓ | ✗ | ✗ | K.离散事件不走抑制窗口（点一下就该跳） |
| `drive_count` | ✓ | 间接（守卫登记；无行为断言） | ✗ | ✗ | K。调试面板观察量 |
| `last_driven` | ✓ | 间接（守卫登记；无行为断言） | ✗ | ✗ | K。调试面板观察量 |

### 4.11 `settings_ui_state.rs` — Snapshot（12）/ PendingConflict（4）/ SettingsUiState（9）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `Snapshot::theme_kind` | ✓（打开期间） | ✓（`snapshot_covers_every_field` 应用层测试钉全覆盖） | **镜像**（对应领域 `ThemeSettings`；Snapshot 自身不 serde，落盘走领域） | ✗ | K。**快照可以放这里而 DTO 不行**：快照是「用户按取消要回到哪」的视图状态，DTO 是「业务现在是什么」的第二份真相 |
| `Snapshot::font_family` | ✓（打开期间） | ✓（同上） | **镜像**（对应领域 `EditorSettings::font_family`；落盘走领域） | ✗ | K。同上 |
| `Snapshot::font_size` | ✓（打开期间） | ✓（同上） | **镜像**（对应领域字段；落盘走领域） | ✗ | K。同上 |
| `Snapshot::line_height` | ✓（打开期间） | ✓（同上） | **镜像**（对应领域字段；落盘走领域） | ✗ | K。同上 |
| `Snapshot::tab_size` | ✓（打开期间） | ✓（同上） | **镜像**（对应领域字段；落盘走领域） | ✗ | K。同上 |
| `Snapshot::word_wrap` | ✓（打开期间） | ✓（同上） | **镜像**（对应领域字段；落盘走领域） | ✗ | K。同上。注意此字段的镜像与 §4.5 `word_wrap` 会话字段是两份值——正是 F-1 的症状面 |
| `Snapshot::auto_save_interval` | ✓（打开期间） | ✓（同上） | **镜像**（对应领域字段；落盘走领域） | ✗ | K。同上 |
| `Snapshot::spell_check` | ✓（打开期间） | ✓（同上） | **镜像**（对应领域字段；落盘走领域） | ✗ | K。同上。与 §4.5 `spell_check` 会话字段的双份值同属 F-1 症状面 |
| `Snapshot::show_line_numbers` | ✓（打开期间） | ✓（同上） | **镜像**（对应领域字段；落盘走领域） | ✗ | K。对应领域字段的会话镜像 `line_numbers` 是四路齐全的范式（§4.5） |
| `Snapshot::lint` | ✓（打开期间） | ✓（同上） | **镜像**（对应领域字段；落盘走领域） | ✗ | K。同上 |
| `Snapshot::max_open_documents` | ✓（打开期间） | ✓（同上） | **镜像**（对应领域字段；落盘走领域） | ✗ | K。同上 |
| `Snapshot::writing_goal` | ✓（打开期间） | ✓（同上） | **镜像**（对应领域字段；落盘走领域） | ✗ | K。同上。其编辑缓冲是 `writing_goal_text`（见下） |
| `PendingConflict::combo` | ✓（确认前） | ✓ | ✗ | ✗ | K。冲突的按键组合本身 |
| `PendingConflict::with_label` | ✓（确认前） | ✓ | ✗ | ✗ | K。被抢占动作的显示名（提示文案用） |
| `PendingConflict::with_key` | ✓（确认前） | ✓ | ✗ | ✗ | K。被抢占动作的键（回滚依据之一） |
| `PendingConflict::with_combo` | ✓（确认前） | ✓ | ✗ | ✗ | K。`with_combo` 是解除原绑定的恢复依据 |
| `section` | ✓ | ✓ | ✗ | ✗ | K。`SettingsTab` 与领域 `SettingsSection` 分两个枚举（「关于」无设置项，同枚举会让「恢复默认」按钮存在但无效） |
| `snapshot` | ✓ | ✓ | ✗ | ✗ | K。`None` = 从未打开过 |
| `recording` | ✓ | ✓ | ✗ | ✗ | K。下一个组合键写给谁 |
| `pending_conflict` | ✓ | ✓ | ✗ | ✗ | K。未确认不写盘 |
| `error` | ✓ | ✓ | ✗ | ✗ | K。领域拒绝时的可见反馈（禁静默）；与 `notice` 互斥 |
| `notice` | ✓ | ✓ | ✗ | ✗ | K。领域拒绝时的可见反馈（禁静默）；与 `error` 互斥 |
| `shortcut_rollback` | ✓ | ✓ | ✗（回滚清单是会话态；改键落盘走领域） | ✗ | K。只记**第一次**原值（连改两次时第二次记的是中间值） |
| `dirty` | ✓ | ✓ | ✗ | ✗ | K。「取消」是否有事可做 |
| `writing_goal_text` | ✓ | ✓ | ✗（编辑缓冲；落盘值由领域独占） | ✗ | K。**不能挪 V**：`TextEdit` 需要稳定 `String` 持源串，每帧重建会让刚敲的字符被覆盖 |

### 4.12 `shortcut_overview_ui_state.rs` — ShortcutRow（3）/ ShortcutSection（2）/ ShortcutOverviewUiState（1）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `ShortcutRow::action` | ✗（每次打开由注册表投影重建） | ✓ | ✗（`combo` 读**活跃**注册表，刻意不做快照——B-08） | ✗ | K。改键后三处显示同步的真源 |
| `ShortcutRow::label` | ✗（每次打开由注册表投影重建） | ✓ | ✗（同上） | ✗ | K。同上 |
| `ShortcutRow::combo` | ✗（每次打开由注册表投影重建） | ✓ | ✗（读**活跃**注册表，刻意不做快照——B-08） | ✗ | K。改键后三处显示同步的真源 |
| `ShortcutSection::group` | ✗（每次打开由注册表投影重建） | ✓ | ✗ | ✗ | K。空结果分组**保留**（删掉会让面板高度忽变） |
| `ShortcutSection::rows` | ✗（每次打开由注册表投影重建） | ✓ | ✗ | ✗ | K。同上 |
| `query` | ✓ | ✓ | ✗ | ✗ | K。`open()` 清空 |

### 4.13 `sidebar_ui_state.rs` + `search.rs` + `tree.rs` — SidebarUiState（15）/ SearchOptions（3）/ SearchGroup（3）/ SearchHitRow（3）/ TreeRow（6）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `expanded_dirs` | ✓ | ✓ | ✗（会话级） | ✗ | K。展开态不落盘是已声明设计（观察项 O-2） |
| `selected_path` | ✓ | ✓ | ✗ | ✗ | K。会话级 |
| `search_query` | ✓ | ✓ | ✗ | ✗ | K。空则不发起搜索（避免无谓全盘扫描） |
| `search_options` | ✓ | ✓ | ✗ | ✗ | K。会话级 |
| `search_scope` | ✓ | ✓ | ✗（注释明言：范围是这一轮查找的临时决定，刻意不落偏好） | ✗ | K。默认「当前文档」被测试钉死 |
| `outline_max_level` | ✓ | ✓ | ✗ | ✗ | K。夹紧 1–6 |
| `outline_selected` | ✓ | ✓ | ✗ | ✗ | K.高亮定位 |
| `context_menu_path` | ✓ | ✓ | ✗ | ✗ | K.`None` = 菜单关闭 |
| `context_menu_pos` | ✓ | ✓ | ✗ | ✗ | K.`Area` 悬浮层每帧要显式位置 |
| `renaming_path` | ✓（编辑跨帧） | ✓ | ✗ | ✗ | K.与 `creating` 共用同一块视觉空间，互斥 |
| `rename_draft` | ✓（编辑跨帧） | ✓ | ✗ | ✗ | K.编辑缓冲：重命名中的名字 |
| `creating` | ✓ | ✓ | ✗ | ✗ | K.`(父目录, 名字, 是否目录)`；默认名取 `default_extension()` 真源 |
| `error` | ✓ | ✓ | ✗ | ✗ | K.与 `notice` 互斥（`fail`/`succeed` 保证） |
| `notice` | ✓ | ✓ | ✗ | ✗ | K.与 `error` 互斥（`fail`/`succeed` 保证） |
| `drawn_tab` | ✓（帧末回写、跨帧比较） | ✓ | ✗ | ✗ | K.**不能挪 V**：标签值可能在帧绘制前就被改掉，拿帧开头读的值当「上一个」永远检测不到切换 |
| `SearchOptions::case_sensitive` | ✓ | ✓ | ✗ | ✗ | K.`reset()` 整块回 `Default`（防新增开关漏清零） |
| `SearchOptions::regex` | ✓ | ✓ | ✗ | ✗ | K.`reset()` 整块回 `Default` |
| `SearchOptions::whole_word` | ✓ | ✓ | ✗ | ✗ | K.`reset()` 整块回 `Default` |
| `SearchGroup::path` | ✗（每次搜索投影重建） | ✓ | ✗ | ✗ | K.按文件分组（平铺会让同文件名重复十几次） |
| `SearchGroup::file_name` | ✗（每次搜索投影重建） | ✓ | ✗ | ✗ | K.同上 |
| `SearchGroup::hits` | ✗（每次搜索投影重建） | ✓ | ✗ | ✗ | K.同上 |
| `SearchHitRow::line` | ✗（每次搜索投影重建） | ✓ | ✗ | ✗ | K.`highlight` 是**字符**区间（字节下标在中文里切错位置） |
| `SearchHitRow::snippet` | ✗（每次搜索投影重建） | ✓ | ✗ | ✗ | K.上下文摘录 |
| `SearchHitRow::highlight` | ✗（每次搜索投影重建） | ✓ | ✗ | ✗ | K.**字符**区间（字节下标在中文里切错位置） |
| `TreeRow::path` | ✗（`tree_rows` 纯函数每次重建） | ✓ | ✗ | ✗ | K.投影非缓存：条目一变结果立刻变 |
| `TreeRow::name` | ✗（`tree_rows` 纯函数每次重建） | ✓ | ✗ | ✗ | K.同上 |
| `TreeRow::depth` | ✗（`tree_rows` 纯函数每次重建） | ✓ | ✗ | ✗ | K.同上 |
| `TreeRow::is_directory` | ✗（`tree_rows` 纯函数每次重建） | ✓ | ✗ | ✗ | K.同上 |
| `TreeRow::is_last` | ✗（`tree_rows` 纯函数每次重建） | ✓ | ✗ | ✗ | K.同上（树形缩进线绘制） |
| `TreeRow::has_next_sibling` | ✗（`tree_rows` 纯函数每次重建） | ✓ | ✗ | ✗ | K.同上（树形缩进线绘制） |

### 4.14 `status_bar_ui_state.rs` — StatusMessage（3）/ StatusBarUiState（4）/ GoalWatch（4）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `StatusMessage::text` | ✓（TTL 跨帧） | ✓ | ✗ | ✗ | K.按时间到期而非等覆盖（连续两次操作第一次会被吞） |
| `StatusMessage::is_error` | ✓（TTL 跨帧） | ✓ | ✗ | ✗ | K.错误与提示分色 |
| `StatusMessage::expires_at` | ✓（TTL 跨帧） | ✓ | ✗ | ✗ | K.到期时刻 |
| `message` | ✓ | ✓ | ✗ | ✗ | K.全局唯一反馈出口（各视图各存一份会互相覆盖） |
| `encoding` | ✓ | ✓ | ✗（只读不写回：`encoding_of` 查询后写入，无落盘路径——观察项 O-3） | ✓（application 查询结果**流入**） | K.未落盘文档显示 `—` 不谎称 UTF-8 |
| `goal` | ✓ | ✓ | ✗（`goal_progress` 查询结果流入） | ✓（`words` 入参来自 `StatsView`） | K.未设目标整项隐藏 |
| `goal_watch` | ✓ | ✓ | ✗ | ✗ | K.去重游标：`(文档, 目标)` 身份 + 先见过未达成才播报 |
| `GoalWatch::doc` | ✓ | ✓（经 `note_goal` 间接断言：到期时刻不变 = 没再弹） | ✗ | ✗ | K.私有 |
| `GoalWatch::goal` | ✓ | ✓（经 `note_goal` 间接断言） | ✗ | ✗ | K.私有 |
| `GoalWatch::seen_below` | ✓ | ✓（经 `note_goal` 间接断言） | ✗ | ✗ | K.私有；先见过未达成才播报 |
| `GoalWatch::announced` | ✓ | ✓（经 `note_goal` 间接断言） | ✗ | ✗ | K.私有；已播报过则不再重复 |

### 4.15 `symbol_ui_state.rs` + `catalog.rs` — SymbolUiState（4）/ SymbolItem（3）

| 字段 | ① | ② | ③ | ④ | 结论与判据 |
|---|---|---|---|---|---|
| `group` | ✓ | ✓ | ✗ | ✗ | K.切换分组夹回 0（两分组长度差 10 倍） |
| `query` | ✓ | ✓ | ✗ | ✗ | K.子序列匹配（符号中文名 3–4 字，子串要求记词序） |
| `selected` | ✓ | ✓ | ✗ | ✗ | K.环绕 + `clamp` |
| `opened_query` | ✓（打开时快照、跨帧比较） | ✓ | ✗ | ✗ | K.「搜索是否改过」的判据 |
| `SymbolItem::text` | ✗（`'static` 静态表） | ✓ | ✗ | ✗ | K.照搬原型但剔除 Hugo 短代码（插一个本应用永不消费的东西比不提供更糟） |
| `SymbolItem::glyph` | ✗（`'static` 静态表） | ✓ | ✗ | ✗ | K.同上 |
| `SymbolItem::label` | ✗（`'static` 静态表） | ✓ | ✗ | ✗ | K.同上 |

### 4.16 为什么零 V（无字段建议挪 view-local）

V 的候选是「① 为否」的 **37** 个字段（各 `rows`/`hits`/`Item`/`Row`/`Match`/`ReplaceSummary` 投影与瞬态类，机械计数见 §7 命令 6）。逐一查看后全部不挪，理由分三类：

1. **所在结构体本身就是 state 模块的公开 API**：`TreeRow` / `SearchHitRow` / `QuickOpenItem` / `CommandRow` / `ShortcutRow` / `DiffRow` 是「DTO → 显示行」的纯函数产物，调用方（视图）本就每次重新取。把它们降级成视图局部变量 = 把纯函数挪进视图文件，反而让 `tree_rows` 这类已测透的投影失去单测入口。
2. **缓存类字段（`finder` / `blocks` / `rows` / `last_key`）① 实为是**：它们跨帧驻留正是为了省重复计算，注释已论证拆开/不存的代价。
3. **真正瞬态的（`rects` / `ReplaceSummary`）① 为否，但已是「取出即用、调用即弃」的形态**——再降级只是把 `Vec` 从结构体字段变成函数返回值，零收益且破坏现有单测入口。

**结论：V = 0 是审计结果，不是漏判。** 每个候选都在上表给出了不挪的具体判据。

---

## 5. 与 field_visibility_guard 的交叉核对结论

- 白名单 168 条 ↔ 201 字段机器比对：**168 个 `pub` 字段全部在名单内，名单无一条对不上 inventory**。
- 33 个非 pub 字段（15 `pub(crate)` + 18 private）不在名单，符合守卫「只管跨 crate 可见面」的设计。
- 无「字段改了可见性但白名单没同步」的漂移（若有，比对会出现在 extra/missing 列表）。

---

## 6. 发现与建议

### F-1（缺陷，建议修）：`word_wrap` / `spell_check` 双路写都不完整

**现象**：两个字段各有两条写入口，每条都只写一半——

| 入口 | 写了什么 | 没写什么 | 用户可见后果 |
|---|---|---|---|
| 设置页复选框（`editor_prefs.rs:31` / `:103`） | 领域 `EditorSettings`（落盘） | `app.editor.*` 会话状态 | 改了当前会话**不生效**（编辑区读的是会话字段）；注释甚至自认「那要重启或状态栏切换才对编辑区生效」 |
| 状态栏点击（`status_bar_view.rs:361-362` → `toggle_*`） | `app.editor.*` 会话状态 | 领域（不落盘） | 即时生效但**重启丢失**；且与设置页显示值（读领域）互相矛盾 |
| 启动 `EditorApp::new` | — | 两个字段都**不回读** | 设置页存的值重启后照样丢 |

**对照范式** `line_numbers`（同一文件、同一模式，四路齐全）：启动播种 `frame.rs:531`；设置页 `set_line_numbers(on)` + `apply_editor_field(...)` 双写 `editor_prefs.rs:40-41`；取消回滚 `settings.rs:207`。

**建议修法**（按 `line_numbers` 范式补齐，二选一）：

- **方案 A（推荐）**：设置页改双写（`app.toggle`/`set_*` + `apply_editor_field`）+ `EditorApp::new` 补两行播种。改动 3 个文件、约 6 行；让「设置页即权威」。
- 方案 B：删去设置页入口，只留状态栏点击并令其双写。入口少一个，但设置页少一个开关与「编辑器」分区定位不符。

验收：设「自动换行 = 关」→ 重启 → 编辑区仍不折行；设置页复选框与状态栏显示一致；取消回滚恢复。需补集成用例（判据走 `EditorApp` 入口，不自己给字段赋值再断言——本项目判据自指的纪律）。

### O-1（观察，不阻塞）：会话级偏好的「不落盘」是声明过的设计

`sidebar_tab` / `preview_visible` / `focus_mode` / `expanded_dirs` / `only_changes` / `search_scope` / `outline_max_level` 等均不落盘，注释逐一声明了理由（会话语境、默认值更可预期、避免第二份真相）。与 §3.1 的 8 个持久化字段之间的分界线清晰，**无需改动**，记录备查。

### O-2（观察）：`only_changes` 有「打开不重置」语义但无任何持久化

`diff_ui_state.rs:76-83`：它是用户查看偏好、不随 `open` 重置——但既不落盘也不在 `reset_for_new_document` 之列。当前形态自洽（纯会话），仅记录：若将来要做「记住只看差异」，落点应是领域偏好而非此处直接 serde。

### O-3（观察）：`status_bar.encoding` 只读不写回

`EditorApp` 在打开/切换文档时向 `DocumentAppService::encoding_of` 查询后写入（V11-03 / DEF-05），保存时编码写回由领域负责。state 字段是查询结果的镜像，无独立写入口，**无双写风险**。仅记录：若编码检测策略变化，该镜像的刷新点是文档切换而非每帧。

---

## 7. 复现命令

```bash
# 1. 机械清单（覆盖基准：24 文件 / 39 结构体 / 201 字段 / 0 未解析）
#    一次性清单脚本 tmp_state_inventory.py 的输出已全文固化在 §4，提交时删除；
#    复核可用等价的正则提取（结构体定位 + 字段行匹配）：
python - <<'PY'
import re, pathlib
root = pathlib.Path('crates/rustmark-core/src/presentation/state')
pat = re.compile(r'^(?:pub(?:\(crate\))?\s+)?struct\s+(\w+)\s*\{(.*?)^\}', re.S|re.M)
files = structs = fields = 0
for f in sorted(root.rglob('*.rs')):
    files += 1
    for m in pat.finditer(f.read_text(encoding='utf-8')):
        structs += 1
        fields += len(re.findall(r'^\s*(?:pub(?:\(crate\))?\s+)?([a-z_]\w*)\s*:', m.group(2), re.M))
print(f'files={files} structs={structs} fields={fields}')   # 期望 24 / 39 / 201
PY

# 2. 零 serde 实证
grep -rEn "serde|Serialize|Deserialize" crates/rustmark-core/src/presentation/state/   # 空

# 3. application 层零引用实证
grep -rn "presentation::state" crates/rustmark-core/src/application/                    # 空

# 4. 白名单交叉比对（期望：168 = 全部 pub；missing 33 全部非 pub；extra 0）
grep -o '"presentation/state/[a-z_/]*\.rs::[A-Za-z]*::[a-z_]*"' \
  tests/field_visibility_guard_test.rs | sed 's/"//g' | sort   # 168 行
# 与清单按 (文件, 结构体, 字段) 三元组求差集

# 5. F-1 取证
grep -n "line_numbers\|word_wrap\|spell_check" src/app/frame.rs \
  src/app/frame/settings.rs src/presentation/views/settings_view/editor_prefs.rs \
  src/app/frame/editor.rs src/presentation/views/status_bar_view.rs

# 6. 本报告自校验：§4 表格行数 = 201；「① 为否」行 = 37
grep -c '^| `' docs/state-field-audit.md                                   # 201（表头行以 | 字段 | 开头，不匹配）
grep '^| `' docs/state-field-audit.md | grep -c '| ✗（.*） | ✓ | ✗ | ✗ |'   # 37（① 为否且 ④ 为否）
```

第一条 grep 只数 §4 各表的字段行（表头 `| 字段 | ① | …` 与分隔行不以反引号开头，天然排除）；第二条同时要求「① 为否、② 为是、③ 为否、④ 为否」，即 §4.16 枚举的瞬态/重建类全集。两个数字与 §4 小结标题中的逐结构体字段数（16/4/3/16/15/14/23/12/5/14/25/6/30/11/7，合计 201）互相印证；字段名多重集亦可与命令 1 的源码提取结果求差集（本报告定稿时双向差集为空）。

一次性清单脚本 `tools/tmp_state_inventory.py` 的输出已全文固化在 §4，随本报告提交一并删除（命令 1 给出等价复核路径）。
