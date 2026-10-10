# 阶段 07 · 端到端用例集（S7-01）

> 对应任务：`.agent/tasks/07-集成联调与原型对齐.md` → S7-01
> 上游输入：阶段 06 完整应用、`REQUIREMENTS.md`（P0 / FR 条目）
> 下游输出：本文档（用例目录）+ `tests/e2e/**`（可自动化部分）

---

## 1. 范围与判定方式约定

本文档按 `REQUIREMENTS.md` 的 **13 条 P0 功能（FUNC-P0-01…13）** 与 **6 条关键交互（FR-01…06）** 逐条派生端到端用例，共 **32 条**。每条用例标注「判定方式」：

- **[自动]**：可脱离真实窗口、在 `cargo test --test e2e` 中跑通（驱动 `EditorApp` 逻辑或解析/高亮/导出纯逻辑），且断言确定。已落入 `tests/e2e/main.rs`。
- **[手工]**：必须看屏幕或涉及 IME / 像素 / 跨平台，进 `docs/manual-qa-*.md`（S7-05），本阶段只记录步骤与预期。

> 已自动化且**被既有单元/集成测试覆盖**的项，统一标注「[自动·既有]」，不再在 `tests/e2e` 重复实现，避免双份真相（见 S6-07 教训）。

执行环境：每条自动化用例都用 `Services::assemble_with_dialogs(ConfigPaths::in_dir(tempdir), FakeDialog)` 起一个**全新配置目录**，满足「干净环境从零执行」验收。

---

## 2. 覆盖矩阵（P0 / FR → 用例 → 判定）

| 需求 | 能力 | 用例 | 判定 |
|---|---|---|---|
| `FUNC-P0-01` | 语法高亮 | E2E-01、E2E-02 | [自动·既有]（S6-04 mapper）、[自动]（内容经高亮路径不损坏） |
| `FUNC-P0-02` | 撤销 / 重做 | E2E-03、E2E-04 | [自动] |
| `FUNC-P0-03` | 查找 / 替换（含正则） | E2E-05、E2E-06、E2E-07 | [自动] |
| `FUNC-P0-04` | 分屏预览 + 双向滚动同步 | E2E-08、E2E-09 | [自动·既有]（S6-06 纯函数）+ [手工] |
| `FUNC-P0-05` | 预览渲染（标题/列表/引用/段落） | E2E-10、E2E-11、E2E-12 | [自动·既有]（S5-02 解析）+ [手工] |
| `FUNC-P0-06` | 代码块高亮 | E2E-13 | [手工] |
| `FUNC-P0-07` | 表格（GFM） | E2E-14 | [自动·既有]（S5-02）+ [手工] |
| `FUNC-P0-08` | 任务列表（点击回写） | E2E-15、E2E-16 | [手工] + [自动·既有]（S5-02 口径） |
| `FUNC-P0-09` | 图片（本地/远程/data URI） | E2E-17 | [手工] |
| `FUNC-P0-10` | 链接 | E2E-18 | [手工] |
| `FUNC-P0-11` | 新建 / 打开 / 保存 | E2E-19、E2E-20、E2E-21、E2E-22 | [自动] |
| `FUNC-P0-12` | 导出 HTML | E2E-23、E2E-24 | [自动] |
| `FUNC-P0-13` | 快捷键系统（可自定义） | E2E-25、E2E-26 | [自动] |
| `FR-01` | 实时预览防抖 150ms | E2E-27 | [手工] |
| `FR-02` | 双向滚动同步误差 ≤1 块 | E2E-09 | [自动·既有] + [手工] |
| `FR-03` | 增量高亮仅重排变更行 | E2E-02 | [自动·既有] |
| `FR-04` | 原子保存 | E2E-28 | [自动] |
| `FR-05` | IME 中文输入（组合期不产撤销快照） | E2E-29 | [手工] |
| `FR-06` | 大文件降级（>500KB 仅高亮可视区） | E2E-30 | [自动] |

**P0 覆盖：13/13；FR 覆盖：6/6。**

---

## 3. 用例明细

### E2E-01 语法高亮不损坏内容（FUNC-P0-01）
- 前置：新建文档。
- 操作：插入 `# 标题\n\`\`\`rust\nlet x=1;\n\`\`\`\n**粗体**`。
- 预期：`current_content()` 与插入文本逐字节一致（高亮是排版层，不得改动业务文本）。
- 判定：[自动]

### E2E-02 增量高亮仅重排变更行（FUNC-P0-01 / FR-03）
- 前置：既有 `tests/presentation/highlight_layout_test.rs` 已钉死「单次编辑重建行数 == 1」「散布修改只重排变更行」。
- 判定：[自动·既有]

### E2E-03 撤销 / 重做往返（FUNC-P0-02）
- 前置：新建文档。
- 操作：插入 `hello` → 插入 ` world` → `undo_edit()` → `redo_edit()`。
- 预期：插入两次后为 `hello world`；撤销后回到 `hello`；重做后回到 `hello world`。
- 判定：[自动]

### E2E-04 连续输入撤销一致（FUNC-P0-02）
- 前置：新建文档。
- 操作：连续 3 次 `insert_at_cursor` 不同片段；连续 `undo_edit` 3 次；再 `redo_edit` 3 次。
- 预期：逐次撤销内容单调缩短至空；逐次重做单调增长到原点；每次结果确定。
- 判定：[自动]

### E2E-05 字面量替换全部（FUNC-P0-03）
- 前置：新建文档，插入 `foo bar foo foo`。
- 操作：`find_text="foo"`、`replace_text="baz"`、`options`（字面量、区分大小写），`sync_find()` 后 `find_replace_all()`。
- 预期：`current_content()` == `baz bar baz baz`，`match_count` 原值 3。
- 判定：[自动]

### E2E-06 正则替换（FUNC-P0-03）
- 前置：新建文档，插入 `abc adc aec`。
- 操作：`options.regex=true`，`find_text="a.c"`、`replace_text="X"`，`find_replace_all()`。
- 预期：内容变为 `X X X`。
- 判定：[自动]

### E2E-07 全字匹配（FUNC-P0-03）
- 前置：新建文档，插入 `this is a test is`。
- 操作：`options.whole_word=true`，`find_text="is"`，`sync_find()`。
- 预期：`match_count == 1`（仅独立的 `is`，不计 `this` 中的 `is`）。
- 判定：[自动]

### E2E-08 预览随编辑刷新（FUNC-P0-04）
- 判定：[自动·既有]（解析层在 S5-02、预览在 S6-05 已覆盖）；视觉刷新节奏见 E2E-27。

### E2E-09 双向滚动同步误差 ≤1 块（FUNC-P0-04 / FR-02）
- 判定：[自动·既有]（`tests/presentation/scroll_sync_test.rs` 纯函数 + 真实帧用例）+ [手工]（S7-05 真机观感）。

### E2E-10/11/12 预览渲染标题/列表/引用（FUNC-P0-05）
- 判定：[自动·既有]（S5-02 块解析单测）+ [手工]（S7-05 像素对照原型 `PreviewPane.vue`）。

### E2E-13 代码块语法高亮（FUNC-P0-06）
- 判定：[手工]（S7-05 对照 `highlight.js` 配色）。

### E2E-14 表格（GFM）渲染（FUNC-P0-07）
- 判定：[自动·既有]（S5-02 表格结构）+ [手工]（S7-05 网格对照）。

### E2E-15 任务列表点击回写（FUNC-P0-08）
- 判定：[手工]（S7-05 复选框点击后源码 `- [ ]` ↔ `- [x]` 回写）。

### E2E-16 任务完成度口径（FUNC-P0-08）
- 判定：[自动·既有]（`compute_stats` 任务计数在 S6-11e 已测）。

### E2E-17 图片三来源（FUNC-P0-09）
- 判定：[手工]（S7-05 本地/远程/data URI 三种加载）。

### E2E-18 链接渲染（FUNC-P0-10）
- 判定：[手工]（S7-05；当前链接不可点击，按原型约定渲染为纯文本）。

### E2E-19 新建并编辑（FUNC-P0-11）
- 前置：全新配置目录。
- 操作：`new_document()` → `insert_at_cursor("# 标题\n正文")`。
- 预期：`open_document_list()` 长度 1，`current_content()` 等于插入文本。
- 判定：[自动]

### E2E-20 保存后重开内容一致（FUNC-P0-11）
- 前置：新建并编辑（E2E-19）。
- 操作：`save_document_to(tmp/note.md)` → `open_path(tmp/note.md)`。
- 预期：重开后 `current_content()` 与保存前逐字节一致；`current_document_dirty()` 为 false。
- 判定：[自动]

### E2E-21 打开已有文件（FUNC-P0-11）
- 前置：磁盘上有一个 `.md` 文件。
- 操作：`open_path(file)`。
- 预期：内容与磁盘一致；`current_document_path()` 等于该路径。
- 判定：[自动]

### E2E-22 未命名文档保存提示另存为（FUNC-P0-11）
- 前置：新建文档，编辑后未保存。
- 操作：`save_document(save_as=false)`。
- 预期：返回「未命名文档需先指定保存路径」类错误（不静默落盘到不可见位置）；`suggested_save_name()` 返回 `未命名.md`。
- 判定：[自动]

### E2E-23 导出 HTML 产出文件（FUNC-P0-12）
- 前置：新建并编辑含标题与代码块。
- 操作：`export_current_document(ExportRequestView{format="html",...}, tmp/out.html)`。
- 预期：返回 `Ok`，`output_path` 文件存在且非空；内容含插入的标题文本与 `<html`/`<pre` 之类标签。
- 判定：[自动]

### E2E-24 导出 Markdown 源码（FUNC-P0-12）
- 操作：同上，`format="markdown"`。
- 预期：产出文件内容等于（或等价于）源 Markdown 文本。
- 判定：[自动]

### E2E-25 改键后三处同步（FUNC-P0-13）
- 前置：全新配置目录。
- 操作：重绑某动作快捷键 → 读取菜单显示 / 命令面板 / 快捷键一览三处。
- 预期：三处显示一致；冲突动作在另绑时给出冲突提示。
- 判定：[自动]（S6-09 已有 5 条核心用例；本用例补「端到端跨三入口」串联）

### E2E-26 快捷键冲突提示（FUNC-P0-13）
- 判定：[自动·既有]（S6-09 `conflict` 用例）+ 本用例补端到端触发路径。

### E2E-27 实时预览防抖 150ms（FR-01）
- 判定：[手工]（S7-05 用计时观察输入停止后 150ms 内重解析）。

### E2E-28 原子保存（FR-04）
- 前置：新建并编辑。
- 操作：正常 `save_document_to(path)`（先写临时文件再 `rename`）。
- 预期：写入成功后原路径文件字节完整；模拟「写入中途」路径由底层保证（临时文件 + 原子 rename），原文件不出现半截内容。
- 判定：[自动]（断言正常保存后内容完整；原子性由 `FsDocumentRepository` 实现 + S5-04 单测钉死）

### E2E-29 IME 中文输入（FR-05）
- 判定：[手工]（S7-05 中文/日文输入法组合期不产生撤销快照，候选框跟随）。

### E2E-30 大文件降级（FR-06）
- 前置：全新配置目录。
- 操作：插入 >500KB 的单段文本；读取内容、打开大纲、保存后重开。
- 预期：不崩溃；`current_content()` 长度与输入一致；>500KB 时高亮走降级路径（可视区高亮）。
- 判定：[自动]（`tests/e2e` 冒烟；降级分支由 S6-04 单测覆盖）

### E2E-31 多标签切换与欢迎页（FUNC-P0-11 / 欢迎页）
- 前置：全新配置目录。
- 操作：`new_document()` 两次（插入不同内容）→ `open_document_list()` → `close_all_documents()`。
- 预期：列表长度 2；关闭全部后 `should_show_welcome()` 为 true。
- 判定：[自动]

### E2E-32 脏文档关闭提示（FUNC-P0-11 / FR 可靠性）
- 前置：新建并编辑（产生脏标记）。
- 操作：检查 `current_document_dirty()`；`save_document_to` 后再次检查。
- 预期：编辑后为 true；保存后为 false（关闭脏文档时据此弹「是否保存」）。
- 判定：[自动]

---

## 4. 自动化执行说明

```bash
# 仅跑端到端自动化用例
cargo test --test e2e

# 全量（含干净环境验证）
cargo test
```

`tests/e2e/main.rs` 复用 `tests/support/` 的 `FakeDialog`，每条用例以 `tempfile::TempDir` 起独立配置目录，保证「全新配置目录从零执行」。
