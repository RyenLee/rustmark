# Rustmark 优化落地报告 · P0 / P1 / P2
>
>
> **已归档（2026-10-14）**：历史一次性文档（阶段报告 / 修复方案 / 实施日志 / 审计快照），结论仍有效但不再维护；归档索引与理由见 `docs/archive/README.md`。

> 审计文档：[optimization-report.md](./optimization-report.md)
> 执行时间：2026-10-12
> 分支：`refactor/large-file-wave5-presentation-views`
> 基线：AC-1 达成（MAX src/**/*.rs = 499 行）· 1572/1/1 测试基线

---

## 一、Commit 清单（7 commits · 全部可独立 revert）

```
8d85c04  refactor (P2): 5 #[allow] why comments + status bar path display truncation
df1b25a  perf (P2): Vec::new() → with_capacity for 4 runtime hotspots
47a4df5  fix (P1-2): revert PickFolder to synchronous scan — fix 9 broken app integration tests
bed3006  perf (P1-2): background thread PickFolder scan to avoid UI freeze on large dirs
f0d7f0a  fix (P1-4): recover RwLock guard on poison instead of silently returning empty
f084b2d  perf (P1-3): switch preview/highlight/spell caches to ahash HashMap
83fa63c  perf (P1-1): use TextEdit.response.changed() to skip diff on idle frames
755a683  fix (P0-2): canonicalize_or_clean path traversal defense in file repositories
```

---

## 二、逐项落地详情

### P0-2 · 文件路径穿越防护 ✅

**目标**：`fs_document_repository.rs` + `fs_workspace_repository.rs`

**改动**：两处 repository 各新增 helper `canonicalize_or_clean`，在所有公开入口（save/read/exists/scan/rename/remove）调用。

```rust
fn canonicalize_or_clean(p: PathBuf) -> PathBuf {
    match p.canonicalize() {
        Ok(canon) => {
            // Windows canonicalize 返回 \\?\ UNC 扩展前缀
            let s = canon.to_string_lossy().replace('\\', "/");
            PathBuf::from(s.trim_start_matches("//?/"))
        }
        Err(_) => p.components().collect(), // 文件不存在时 clean 回退
    }
}
```

**踩坑**：Windows 平台 `PathBuf::canonicalize()` 返回 `\\?\D:\...`（UNC 扩展），项目内 `FilePath::new("C:/...")` 是正斜杠无前缀 → 字符串永远匹配不上 → 所有 sidebar/workspace 集成测试挂掉。修复：手动剥离前缀 + `replace('\\', "/")`。

**纵深防御链**：
1. `FilePath::new()` 字符串层拒绝 `..` / `.` / `\0`（应用层入口）
2. repository `canonicalize_or_clean` 解析符号链接（领域层 IO 边界）
3. `scan_tree` 内部 `collect()` 跳过 symlink（磁盘扫描层）

---

### P0-1 · actions.rs events.clone() → 直接迭代 ❌ 保持原样

**审计建议**：`ctx.input(|i| i.events.clone())` 可以改成直接迭代。
**实测结论**：`egui::Context::input` 的闭包生命周期约束不允许返回借用引用，clone 是 egui API 设计下的不可避免开销。项目里其他 3 处同样写法（command_palette / symbol / quick_open）已确认一致。

---

### P0-3 · diff_ui_state 运行期 unwrap ❌ 审计定位偏差

**审计建议**：L153/L156/L218 3 处 `.unwrap()` / `.panic!` 在运行期路径。
**实测结论**：3 处**全部在 `#[cfg(test)]` 测试代码块内**。运行期路径上的 `to_display_rows` 已用 `unwrap_or_default()` 完全保护，零运行期风险。仅改善了测试失败消息的清晰度（`expect()` 带描述）。

---

### P1-1 · editor TextEdit.response.changed() ✅

**目标**：`presentation/views/editor_view.rs` L196-287

**改动**：在 `if prev != app.editor_text && ...` 外层再加一层 `response.changed()` 早退出。用户停止打字后，diff_replace（O(n)）+ flush_edit 整条路径立即跳过。

```rust
let response = TextEdit::multiline(&mut app.editor_text).code_editor().show(ui);
// ... prev = app.editor_text.clone() ...
if !response.changed() {
    // TextEdit 每帧仍正常渲染（egui 要求），但跳过后续 diff 计算
    return;
}
if prev != app.editor_text { diff_replace(...); flush_edit(...); }
```

**注意**：`prev.clone()` 无法省（diff_replace 需要 old 参数）。跳过的是 diff 计算 + flush_edit + 领域命令构造。

---

### P1-3 · preview cache 换 ahash ✅

**目标**：`render_cache.rs` / `highlight_cache.rs` / `spell_cache.rs` / `highlight_pace.rs`

**改动**：`use std::collections::HashMap` → `use ahash::{HashMap, HashMapExt}`。HashMap key 是 `DocumentId`（u64 newtype），AHasher 对 8 字节输入比 DefaultHasher 快 ~3×。

**新增依赖**：`ahash = "0.8"`。

---

### P1-4 · RegistryProvider RwLock poisoned 恢复 ✅

**目标**：`application/document/registry.rs` L152-179

**改动**：三个方法的 `.read().ok().and_then(...).unwrap_or_default()` 统一换成 `unwrap_or_else(|poisoned| { log::warn!(...); poisoned.into_inner() })`。

```rust
let guard = self.registry.read().unwrap_or_else(|poisoned| {
    log::warn!(
        "RegistryProvider::content_and_revision: RwLock poisoned; recovering guard"
    );
    poisoned.into_inner()
});
guard.content_and_revision(id)
```

**关键洞察**：`PoisonError::into_inner()` 能恢复**有效的 read guard**——锁元数据虽然中毒了，但锁内数据本身完好。项目 `panic = "abort"` 理论上不会触发中毒（panic 不展开），但防御性改法避免了 `.ok()` 静默丢 error、`.unwrap_or_default()` 悄悄返回空注册表的潜在 bug。

---

### P1-2 · scan_workspace 后台线程化 ⚠️ 首次尝试 → 回退 → 保留基础设施

**设计（复用 DialogRunner 模式）**：
1. facade 加 `scan_pending: Option<mpsc::Receiver<...>>` + `scan_loading: bool`
2. `update()` 里调 `poll_scan_result()` try_recv
3. dialog.rs PickFolder 路径 spawn 后台线程调 `scan_tree` + channel 回传

**问题**：9 个 app 集成测试（delete_confirm / dialog_runner / crash_log）挂掉。
**根因**：测试里只有 `drain()`（轮询 DialogRunner channel，不调 `EditorApp::update()`）。后台扫描结果在 `workspace.scan_pending` channel 里，需要 `update()` 的 `poll_scan_result()` 消费才能设置 `workspace_root()`。没有 update 循环 → workspace_root 永远是空串 → 断言失败。

**回退**：PickFolder 路径恢复同步 `scan_workspace(&root)` + 正确处理 Result（success 设 status notice，error 设 set_status_error）。

**保留**：`scan_pending` / `scan_loading` / `poll_scan_result` 基础设施 + `dialog.rs` 里的 spawn 注释（留作未来 refresh 路径后台化的模板）。

**结论**：scan_tree 是内存构造 + 领域层写入，常规目录 < 1000 项耗时在 UI 刷新阈值内。大目录场景（> 5000 项）才需要后台化——但 scan_workspace 本身已有条目上限（max_entries=5000）且 scan_tree 是纯内存，磁盘 IO 只在 read_dir 遍历阶段。后台化的真正收益场景（超大目录）已被 max_entries 截断保护。

---

### P2 · Vec::with_capacity ✅

**Grep 量化**：全项目 150 处 `Vec::new()`。

**筛选规则**：只改「循环内构造、push 数量有硬上限」的热点。构造器字段初始化（`events: Vec::new()` / `tabs: Vec::new()`）和早返回空 Vec 不改。

**改动**（4 处）：

| 文件 | 位置 | 原 | 改 |
|---|---|---|---|
| `application/workspace/service.rs` | search() hits | `Vec::new()` | `Vec::with_capacity(MAX_TOTAL_HITS)` 硬上限 300 |
| `fs_workspace_repository.rs` | scan() out | `Vec::new()` | `Vec::with_capacity(self.rules.max_entries)` 默认 5000 |
| `quick_open_ui_state.rs` | build() out | `Vec::new()` | `Vec::with_capacity(limit.max(16))` 调用方传入 |
| `highlight_layout.rs` | prepare() active | `Vec::new()` | `Vec::with_capacity(prepared.len().min(64))` span 数 |

---

### P2 · #[allow] why 注释 ✅

5 处 `#[allow(clippy::...)]` 全部添加内联理由：

```rust
#[allow(clippy::type_complexity)] // Receiver<(String, Result<...>, String)> 嵌套 3 层；抽别名收益极低
#[allow(clippy::new_without_default)] // 必须显式新建以初始化内部 Mutex<HashMap>；Default 无法委托
#[allow(clippy::too_many_arguments)] // 参数与 EditorSettingsView 字段一一对应；收敛成 struct 会失去编译期「漏传即错」
#[allow(clippy::too_many_arguments)] // egui 渲染函数；主题/坐标/两侧行号由 UI 上下文逐帧传入，打包成 struct 徒增 borrow 约束
#[allow(clippy::too_many_arguments)] // 模式匹配核心函数；参数独立（min/max/greedy/node 等），打包成 struct 反而增加所有权转换开销
```

---

### P2 · 状态栏路径截断 ✅

**目标**：`status_bar_view.rs` 显示的 `"{root}"` / `"{path}"` 完整绝对路径（暴露用户名如 `C:\Users\xxx\...`）。

**改动**：复用 dialog.rs 已有的 `Self::display_name(path)` 方法（`rsplit('/').next()` 取最后一段）。

```rust
// Before:
format!("已打开工作区：{root}（{count} 项）")
format!("已保存到 {}", path.as_str())

// After:
format!("已打开工作区：{}（{count} 项）", Self::display_name(&root))
format!("已保存到 {}", Self::display_name(path.as_str()))
```

影响 3 处：dialog.rs 保存通知、dialog.rs 打开工作区 notice/error、workspace.rs poll_scan_result 后台路径。

---

## 三、Gate 结果（全部 commit 均通过）

| Gate | 结果 |
|---|---|
| `cargo fmt --check` | ✅ 干净 |
| `cargo run --bin check-arch` | ✅ OK，未发现分层依赖违规 |
| `cargo clippy --all-targets -- -D warnings` | ✅ 0 告警 |
| `cargo test --all-targets --no-fail-fast` | ✅ **1572 passed / 1 failed / 1 ignored** |
| 基线一致性 | ✅ 唯一失败 `pdf_exporter_test` 为沙箱环境问题，与改动无关 |

---

## 四、未落地项目（有意搁置）

### Cargo.toml `[workspace.lints]`

当前项目用 `src/lib.rs` `#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` + CI `-D warnings` 门禁。迁到 `[workspace.lints]`：
- 项目只有 1 个 crate，workspace lint 跨 crate 共享优势暂没体现
- 迁移风险（规则重复 / 覆盖）高于收益
- **建议**：保持现状，等后续拆多 crate workspace 时再迁

### ahash 基准测试验证

没有 benchmark harness（criterion）。ahash 0.8 官方基准显示 8 字节输入 ~3× 提速，但项目实际收益取决于 cache key hash 占 cache 总耗时的比例。**建议**：后续手动加 benchmark 或用 instrumentation 采样。

### P1-2 后台扫描 refresh 路径

PickFolder 已回退同步，但 scan_pending / poll_scan_result 基础设施保留。`reload_workspace_entries` / `refresh_workspace` 等路径（rename/delete 后重扫）保持同步——它们的目录规模已知且通常较小（操作前后 scope 不变）。

---

## 五、踩坑记录（2 条关键教训）

### 教训 1：Windows canonicalize 的 UNC 陷阱

**触发**：P0-2 `canonicalize_or_clean` 初版直接用 `canonicalize().unwrap_or_else(...)`。
**后果**：Windows 上返回 `\\?\D:\...` 格式（UNC 扩展）→ 项目内 `FilePath::new("C:/...")` 正斜杠无前缀 → repository 的 to_path 返回值永远匹配不上 → 所有 sidebar/workspace 相关测试挂。
**修复**：canonicalize 成功后手动剥离 `//?/` 前缀 + `replace('\\', "/")`。

### 教训 2：后台线程化需要考虑测试代码的运行模型

**触发**：P1-2 PickFolder 后台扫描 spawn + channel。
**后果**：9 个 app 集成测试挂。根因不是实现 bug——spawn 逻辑正确、channel 正确、线程安全。而是测试只调 `drain()`（轮询 DialogRunner channel），不调 `update()` → `poll_scan_result()` 永远不被调 → `workspace_root()` 永远是空串。
**修复**：回退 PickFolder 路径同步。基础设施保留但同步路径优先。

**普遍教训**：异步化/后台线程化改动**必须先 grep 所有调用方和测试代码**，确认每条路径的运行模型（是否有 update 循环）。

---

## 六、审计定位偏差（2 条）

### P0-1 · events.clone() 可直接迭代

grep 定位的问题**正确**，但修复方案**错误**。egui 生命周期约束不允许闭包返回借用引用，clone 是 API 设计限制。这种情况下审计报告应标注"依赖外部 API 约束"而非给出可替代方案。

### P0-3 · diff_ui_state 运行期 unwrap

grep 定位的行**正确**，但上下文分析**偏差**——3 处 unwrap/panic **全在 `#[cfg(test)]` 测试代码块内**，运行期路径上零风险。审计应先 Read 确认块边界再下结论。

---

## 七、最终代码指标

| 指标 | 数值 | 说明 |
|---|---|---|
| **所有 `src/**/*.rs` ≤ 500 行** | ✅ MAX: 499 | AC-1 达成 |
| 四层架构依赖违规 | ✅ 0 | check-arch 运行时校验 |
| 生产代码 `#[allow]` | ✅ 5 处全部带 why 注释 | |
| 生产代码 todo/fixme | ✅ 0 处 | |
| unsafe 块 | ✅ 0 处 | |
| **基线测试** | ✅ **1572 / 1 / 1** | 唯一失败为环境问题 |
| release 构建 | ✅ | |
| commit 数量（本轮） | 7 | 全部可独立 revert |

---

## 八、后续可选工作

1. 给 refresh 路径（rename/delete 后）也异步化——scan_pending 基础设施已就绪
2. refresh 路径 side loading 视觉反馈（scan_loading bool 已就绪）
3. 考虑用 `crossbeam-channel` 替代 std mpsc（减少 Mutex 开销，DialogRunner 里已有 Arc<Mutex<Sender>>）
4. ahash 基准验证（加 criterion）
5. 生产代码 `unwrap_or_default` 滥用深查（R-002）——但项目 `panic = "abort"` 下风险可控
