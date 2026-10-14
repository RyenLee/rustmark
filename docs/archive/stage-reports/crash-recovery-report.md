# S7-06 异常退出与草稿恢复 · 验证报告
>
>
> **已归档（2026-10-14）**：历史一次性文档（阶段报告 / 修复方案 / 实施日志 / 审计快照），结论仍有效但不再维护；归档索引与理由见 `docs/archive/README.md`。

> 阶段 07 · 集成联调与原型对齐 · 任务 S7-06
> 自动化：`tests/integration/crash_recovery_test.rs`

---

## 1. 结论摘要

| 验收点 | 状态 | 说明 |
|---|---|---|
| 原子保存中断原文件完好（FR-04 控制流） | ✅ 已验证 | 目标文件只在最后一次原子 `rename` 处被替换；崩溃残留 `.rustmark-tmp-` 不污染目标 |
| 强杀后损失 ≤1 个自动保存间隔（决策逻辑） | ✅ 逻辑层已验证 | `AutoSaveTracker::due()` 正确按间隔标记待保存文档 |
| 草稿恢复数据层 | ✅ 已验证 | `save_draft`/`load_draft`/`clear_draft` 写读清语义正确 |
| **异常退出后草稿恢复（端到端）** | ✅ **已修复（DEF-02）** | 运行时接线补齐（事件订阅 + `update` 每帧 `run_autosave` + 启动扫描），见 §5 |

---

## 2. 已验证的底层保证

### 2.1 原子保存控制流（FR-04）

实现：`src/infrastructure/persistence/fs_document_repository.rs` 的 `atomic_write`——
同目录临时文件 → `flush` + `sync_all` → `rename` 覆盖。任一步失败都不触碰目标文件。

集成测试（`crash_recovery_test.rs`）以**最贴近真实崩溃**的方式构造场景：

| 用例 | 场景 | 断言 |
|---|---|---|
| `atomic_save_survives_leftover_temp_from_crash` | 进程在「写完 tmp、未 rename」时崩溃，磁盘留下 `.rustmark-tmp-` 垃圾；用户再次正常保存 | 崩溃残留**从未污染**目标（始终是上次完整的 A）；新保存内容 B 恰好覆盖；无残留 tmp |
| `atomic_save_failure_leaves_no_temp_residue` | 保存路径的父组件被普通文件占据，`rename` 不可达 | 保存失败；**不泄漏**临时文件；不产生半截目标 |

> 这两例与 `tests/infrastructure/fs_document_repository_test.rs`（单测）互补：单测验「正常 round-trip / 目标为目录时失败」等，集成测把焦点放在「**写入中途崩溃**」这一 S7-06 特定场景。

### 2.2 自动保存计时逻辑

实现：`src/application/event_subscriptions.rs` 的 `AutoSaveTracker`（`due()` 采用「取出即清除」语义）。

集成测试验证「损失 ≤1 个自动保存间隔」的**决策正确性**：

- 距最后编辑不足一个间隔 → 不标记；
- 恰好到达间隔边界 → 标记待保存；
- 取出后清除 → 不重复保存；
- 多文档、不同步的编辑时间 → 只标记真正空闲达间隔的文档。

### 2.3 草稿仓储机制

`save_draft`/`load_draft`/`clear_draft` 的写→读→清→再读为空、清除幂等，验证恢复功能的**数据层**存在且正确。

---

## 3. DEF-02 修复：自动保存 / 草稿恢复运行时接线（已实施）

> 原「阻塞项」已于 S7-06 收尾阶段修复，以下记录修复内容与验证方式。

### 根因

S7-06 的端到端验收——「强杀后损失 ≤1 个自动保存间隔；异常退出后草稿恢复」——
此前**无法在运行中的 `EditorApp` 上成立**。经全仓检索确认：

1. `AutoSaveTracker`（计时器）在 `event_subscriptions.rs` 中定义，但 `EventSubscribers`
   **从未在装配阶段注册**到事件总线 → `TextModified` 从未驱动计时器记录。
2. `FsDocumentRepository::save_draft` / `load_draft` / `clear_draft`
   （`fs_document_repository.rs`，注释写明「异常退出后可恢复，需求 §4.3」）**从未被任何运行时代码调用**。
3. 启动流程（`app.rs` / `frame.rs` 的 `update`）中**没有任何**「扫描 `.rustmark-draft/` 并提示/恢复」的逻辑。

即：砖（计时器、草稿仓储、原子保存）齐备，但**没人把它们砌成墙**。

### 修复内容

- **事件接线**（`src/app/bootstrap.rs`）：装配阶段构造 `EventSubscribers` 并 `register` 到总线，
  `TextModified` → 计时器记录、`DocumentSaved`/`DocumentClosed` → 移除计时、
  `DocumentSaved` → 刷新最近文件、`PreferencesChanged` → 失效渲染缓存。
  顺带让「最近文件 / 预览缓存失效」也真正生效（此前同样因未注册而失效）。
- **草稿落盘原语上移**（`src/domain/document/repository.rs`）：把 `save_draft`/`load_draft`/
  `clear_draft`/`list_drafts` 提为 `DocumentRepository` trait 方法（带 no-op 默认实现，
  使测试替身无需各自实现），`FsDocumentRepository` 提供真实文件系统实现。
- **应用层编排**（`src/application/document/service.rs`）：新增 `save_draft`/`load_draft`/
  `clear_draft`/`pending_recoveries`（扫描目录并读回每份草稿）/`new_document_with_content`
  （把草稿内容新建为未命名文档，用于恢复）。
- **运行时节拍**（`src/app/frame.rs`）：`EditorApp::update` 每帧调用 `run_autosave`——
  读 `auto_save_interval`，`auto_save_tracker.due(now, 间隔)` 得到到期文档后写草稿；
  保存成功 / 关闭文档 / 关闭全部时 `clear_draft` 清掉对应草稿，避免误报。
- **启动恢复**（`src/app/frame.rs`）：`EditorApp::new` 扫描 `pending_recoveries`，
  非空时每帧弹出一个居中提示，提供「恢复全部」（按内容新建未命名文档）与「丢弃全部」。

> 设计取舍：自动保存采用「写草稿到 `.rustmark-draft/`」而非直接覆盖原文件——
> 既满足 NFR-REL-02（周期持久化未保存内容），又直接构成 NFR-REL-03（崩溃后可恢复）的数据源。
> 已命名文件被编辑但未保存时，重启后草稿以「未命名文档」形式恢复（文档 ID 为会话内临时值，
> 无法跨会话映射到原路径），恢复后用户可重新保存——这是首版实现的已知限制。

### 验证

- `tests/integration/crash_recovery_test.rs`：
  - `autosave_writes_draft_and_is_recoverable_after_wiring`（经生产级 `Services` 装配验证
    `TextModified` → 计时器记录 → 到期 → 写草稿 → 启动扫描发现）；
  - `restart_scan_finds_leftover_drafts`（同一配置目录「两次启动」模拟崩溃重启，验证草稿跨进程留存）；
  - 原有 5 个用例（原子保存控制流 / 计时逻辑 / 草稿数据层）保持有效。
- `e2e_crash_recovery_restores_unsaved_draft`：**已取消 `#[ignore]`**，headless 驱动真实
  `EditorApp::update` 帧循环（eframe `Frame::_new_kittest()` + `ctx.run(1280×800)`，
  无需窗口与 GPU），串起「新建 → 输入 → 间隔到期自动保存落草稿 → 丢弃应用（模拟强杀）
  → 重启扫描到 1 份草稿 → 再驱动数帧提示不丢 → 恢复全部 → 内容一致且草稿清除」。
- `recovery_prompt_discard_clears_leftover_drafts`：覆盖「丢弃全部」分支（草稿被删除、
  不新建文档）。

> 「恢复全部 / 丢弃全部」的动作原先内联在 `render_recovery_prompt` 的按钮回调里，
> 已抽出为 `EditorApp::recover_all_drafts` / `discard_all_drafts` 两个公开方法，
> 使按钮点击与集成测试走**同一条**代码路径——否则测试覆盖的是副本，按钮本身反而是未测代码。
> 另加 `pending_recoveries()` 只读 getter 供状态断言。

### 影响面（修复后）

- `NFR-REL-02`（自动保存，可配置间隔）：**已生效**，默认 30s。
- `NFR-REL-03`（断电 / 异常退出时草稿恢复）：**已生效**，启动提示恢复。

---

## 4. 遗留

- ~~端到端恢复验收的「真实 `update` 循环每帧驱动 `run_autosave` 且弹出恢复提示」全链路，
  待 headless 渲染 `EditorApp` 后由 `e2e_crash_recovery_restores_unsaved_draft` 承接（取消 `ignore`）。~~
  **已完成（2026-10-08）**：headless 帧循环驱动方案落地，该用例已并入门禁。
- 已恢复文档变为「未命名文档」（`DocumentId` 为会话内临时值，无法跨会话映射回原路径），
  需用户重新保存——首版实现的已知限制，见 §3。
- 三平台手工回归中 `MQ-*-27 / MQ-M-26 / MQ-L-25` 用于确认 DEF-02 修复后回归。
- 真机强杀 / 断电属于手工项，自动化只能覆盖到「底层保证 + 逻辑层 + 数据层 + 装配接线」。
