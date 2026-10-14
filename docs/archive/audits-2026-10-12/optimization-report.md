# Rustmark 全项目优化建议 · 2026-10-12
>
>
> **已归档（2026-10-14）**：历史一次性文档（阶段报告 / 修复方案 / 实施日志 / 审计快照），结论仍有效但不再维护；归档索引与理由见 `docs/archive/README.md`。

> 基于 Rust-Full-Stack-Feature-Audit&Fix skill 框架，对重构完成后的 rustmark 项目做 **5 维度静态扫描**。
> 扫描方法：check-arch 规则校验 / clippy lint / 全量 grep 量化 / 关键路径精读。
> 项目基线：AC-1 达成（MAX src/**/*.rs = 499 行）、1572/1/1 测试基线、四层架构分层。

---

## 一、错误处理（最优先级）

### R-001 · 生产代码中仍有 `unwrap()` / `expect()` / `panic!`

**项目级规约**（src/lib.rs L18, src/domain.rs L9）明确禁止生产代码出现 `unwrap/expect/panic!`，但 grep 显示以下分布：

| 类别 | 数量 | 说明 |
|---|---|---|
| 生产代码 **非启动期** | **5 处** | 需要立即替换 |
| 生产代码 **启动期/装配** | 8 处 | 可接受但应标注 |
| 测试代码 | 120+ 处 | 豁免（clippy allow） |
| `unsafe` 块 | 0 处 | ✅ 干净 |

### 🔴 R-001a · `infrastructure/fonts.rs`（启动期但 5 处 expect）

**位置**：`src/infrastructure/fonts.rs` L165, L178, L186, L202, L248

```rust
// L165
let (path, bytes) = found.expect("上面已断言存在");
// L178 / L186 / L202
.expect("默认应有比例族")
.expect("追加后仍应有比例族")
.expect("该族应存在")
// L248
let source = setup.cjk_source.clone().expect("上面已判非空");
```

**风险**：字体初始化在启动路径上，panic 直接导致进程退出（`panic = "abort"`），用户看到 "程序闪退"。但这些 expect 的前提条件是代码上方已 `assert!` 过——逻辑上不可能 None，只是 Rust 类型系统看不见。

**建议**：保留但给 expect 消息加上文件名/行号（当前已有），并在 lib.rs 的 lint 豁免列表中显式列出该文件。或者改用 `unwrap_or_else(|| unreachable!("…"))` 让语义更清晰。

### 🟡 R-001b · `presentation/state/diff_ui_state.rs`（运行期 diff 计算）

**位置**：L153 `rows.iter().find(|r| r.op == DiffOp::Remove).unwrap()` / L156 Add 同理 / L218 `panic!("应为替换行，实际 {other:?}")`

**风险**：diff 计算是用户操作触发的（撤销改动/对比）。如果 diff 算法有 bug 导致 Remove/Add 找不到 → panic。这是**算法 invariant 假设** vs **用户数据路径**的交点，值得改成 Result 传播。

**建议**：把 `DiffUiState::compute_segments` 返回 `Result<..., DiffError>`，让上层 UI 降级为 "diff 计算失败" 提示。

### 🟡 R-001c · `app/frame/actions.rs`（帧循环内 ctx.input 克隆）

**位置**：`src/app/frame/actions.rs` L213

```rust
for event in &ctx.input(|i| i.events.clone()) {
```

这不是 unwrap，但 `.input(|i| i.events.clone())` 每帧会**克隆整个事件向量**。如果事件较多（如拖放时）是不必要的分配。

### R-002 · `unwrap_or_default` 静默降级

项目多处用 `.unwrap_or_default()` 处理错误：
- `src/shared/time.rs` L70-78：时间转换用 `unwrap_or(0)` / `unwrap_or(u64::MAX)` — 正确，属于"饱和值"策略
- `src/application/document/registry.rs` L152-173：锁读取失败用 `ok().and_then(...).unwrap_or_default()` — **有风险**，锁 poisoned 时会返回空注册表，后续文档查找静默失败

**建议**：registry 锁读取应返回 `Result`，让上层决定降级策略；或者至少用 `unwrap_or_else(|e| { log::warn!("registry lock poisoned: {e}"); Default::default() })`

### R-003 · 错误类型一致性

项目混用：
- `domain` 层：自定义 domain error（`ParseError` / `WorkspaceError` / `DocumentError`）
- `application` 层：封装后的 command/query error
- `infrastructure` 层：`std::io::Error` + 自定义
- `shared` 层：`FilePathError` + `TimeError`

**优点**：domain → application 边界有明确的错误类型转换。
**改进空间**：domain error Display 格式不统一（有的含路径、有的只含消息），建议统一 `"{op}: {detail}"` 模式。

---

## 二、架构分层与耦合

### R-004 · `app/` 层同时依赖 infrastructure + application + domain（**设计允许**）

grep `crate::infrastructure::` 在 src/ 下有 28 处引用，全部集中在：
- `src/app/bootstrap.rs`（组合根装配，**设计允许**）
- `src/app/frame/capture.rs`（图片导出直接调 `viewport_capture` 的 `stitch/crop/write_png`，**组合根内的应用侧代码**）
- `src/app/frame.rs` L446（`install_fonts` 直接调 `infrastructure::fonts::font_definitions()`）
- `src/infrastructure/export/routing_exporter.rs`（infrastructure 内部 cross-ref）

**check-arch 规则**对 `app/` 目录做了豁免——组合根允许直接实例化基础设施适配器。这符合六边形架构的装配端模式。**零违规**。

### R-005 · presentation/state 层直接引用 domain 类型（**设计允许**）

`check-arch` 规则允许 presentation 依赖 domain（"领域模型是 UI 的数据契约"）。扫描确认无 presentation → application 的违规引用。**零违规**。

### R-006 · frame 子模块间耦合度

拆成 12 个子模块后，兄弟间引用模式：

```
dialog   → files (open_path), settings (不)
capture  → settings (theme), workspace (不)
editor   → find (不), settings (不), capture (不)
find     → editor (editor_text), frame::settings (不)
files    → editor (不), dialog (不)
outline  → workspace (不), find (不)
recovery → workspace (不), dialog (不)
settings → actions (domain_action), dialog (不)
tabs     → editor (不), workspace (不)
workspace → outline (不), tabs (不)
actions  → settings (不), dialog (不)
```

**结论**：耦合度低。兄弟间直接引用 ≈ 0-1，多数子模块只通过 facade 的 `self.xxx()` 方法间接协作。**解耦成功**。

### R-007 · DialogRunner 跨线程状态

**位置**：`src/app/dialog_runner.rs` L96-177

DialogRunner 在后台 `std::thread` 执行原生对话框（`rfd`），通过 `mpsc::channel` 回传 `DialogResult`，`EditorApp::dialogs` 字段是 `DialogRunner` 持有 tx，`update()` 每帧 try_recv。

**优点**：完全正确的设计——channel 保证线程安全、try_recv 不阻塞 UI 线程。
**改进空间**：tx 用了 `Arc<Mutex<Sender>>` 而不是直接 `Sender`（因为 spawn 需要 'static 捕获 Arc）。可以考虑 `crossbeam-channel` 替代 std mpsc 减少锁开销。

---

## 三、性能热点

### R-008 · 帧循环内高频 `.clone()` 路径（**TOP 优化**）

grep `.clone()` 在 src/ 下有 200+ 处。以下是**每帧执行**的热点：

| 位置 | 对象 | 每帧次数 | 影响 |
|---|---|---|---|
| `editor_view.rs` L196-287 | `editor_text` (String, ~20KB) | 1 | String 分配 + memcpy |
| `frame/actions.rs` L213 | `ctx.input(\|i\| i.events.clone())` | 1 | Vec<Event> 克隆 |
| `frame/capture.rs` L179/266 | `export_ui.theme` | 捕获触发时 1 | Theme 结构体 |
| `frame/find.rs` L55/120 | `self.find.finder` (Vec<String>) | 搜索触发时 | 搜索文本 |
| `find_replace_view.rs` / `welcome_view.rs` / `export_view.rs` | `self.theme()` | 1 | Theme 结构体 |
| `sidebar_ui_state/tree.rs` L107/118 | 树节点 Vec | 刷新时 | Vec<Path> |

**editor_text 每帧克隆**是最大单点开销。eframe 要求 TextEdit value 是 String（不是 &str），所以 clone 是 egui API 强制——但可以尝试：
- 每帧前 `let prev_len = app.editor_text.len();`，若编辑器没变化则跳过 diff_replace 计算
- TextEdit 变化检测：egui 返回的 response 有 `.changed()` 方法

### R-009 · scan_workspace 阻塞 IO

**位置**：`src/app/frame/workspace.rs` L145-169

扫描明确**不在每帧执行**（设计正确）。但单次扫描是同步 `std::fs::read_dir` 递归，可能在大目录（5000+ 文件）时阻塞 UI。

**建议**：spawn 到后台 `std::thread`，UI 显示 loading skeleton，结果通过 channel 回传（DialogRunner 模式可复用）。

### R-010 · preview Markdown 重解析

**位置**：`src/presentation/state/preview_ui_state.rs` L148-182 + `preview_view.rs` L240-255

已有**缓存 + 150ms 防抖**设计（should_render 判定）。缓存键是 `(content_hash, doc_id, theme_version)`。

**结论**：设计良好。唯一改进空间：content_hash 可以用 `ahash` 替代默认 `DefaultHasher` 提速 3×。

### R-011 · 字符串分配热点

```rust
// domain/workspace/aggregate.rs — 20+ 处 path.clone()
// application/workspace/service.rs — 15+ 处 path.clone()
// application/event_subscriptions.rs — 8 处 service.clone() (Arc)
```

**原因**：domain aggregate 里返回错误时要 `path.clone()`（错误结构体需要拥有所有权），这是合理的所有权边界。可以改错误类型持有 `PathBuf` 但用 Cow 包裹——改动面大，收益有限。

### R-012 · egui `events.clone()`（R-008 子项详解）

```rust
// src/app/frame/actions.rs:213
for event in &ctx.input(|i| i.events.clone()) {
```

egui 的 `InputState::events` 是 `Vec<Event>`。`events.clone()` 整 Vec 克隆是不必要的——改成直接引用：

```rust
// cargo clippy 已经会提示 `clippy::map_clone`
// 修复：
for event in ctx.input(|i| i.events.iter()) {
```

预期收益：**每帧少一次 Vec 分配 + 事件结构体 memcpy**（几十到几百字节/帧）。

### R-013 · Vec 预分配缺失

parse/end.rs 内 `Vec::new()` 后可能 push 上百个元素。建议 grep 所有 `Vec::new()` 后 push 10+ 元素的地方，加 `with_capacity`。

---

## 四、安全隐患

### R-014 · 路径穿越（**风险低**）

全项目文件操作路径来源：
- 用户通过原生对话框（rfd）选择 → **rfd 自己 canonicalize** → 安全
- 拖放 → eframe 给的路径 → 直接用但应 canonicalize
- 工作区扫描 → 根目录已知 → 扫描子项时检查 starts_with

**扫描**：`fs_document_repository.rs` / `fs_workspace_repository.rs` 保存文件前**没有** canonicalize 用户路径。拖放路径可能含有 `..`。

**建议**：在 `save_document` / `create_directory` / `remove` 入口加 `path.canonicalize()` 或 `let path = path.components().collect::<PathBuf>()` 过滤 `..`。

### R-015 · HTML 注入 / XSS（**已防护良好**）

Markdown 导出 → HTML → 自定义 `html_sanitize.rs`（禁止 `data:text/html`、过滤危险标签/属性）。

**预览**不走 HTML：pulldown_markdown → `RenderedBlock` 枚举 → egui RichText 渲染。**安全**。

### R-016 · 文件覆盖无警告

**位置**：`fs_document_repository.rs` L201-243（原子写入 `.tmp` → `sync` → `rename`）

保存流程：
1. 写 `.tmp` 临时文件
2. `sync_all()`
3. `rename` 覆盖目标

**设计优点**：原子 rename 保证要么旧文件完整存在，要么新文件完整落盘。临时文件确保写一半断电不会丢内容。
**改进**：应用层 `save_document` 入口（`lifecycle.rs`）在覆盖已有非空文件时**没有**二次确认对话框（"文件已修改，确认覆盖？"）。这是 UX 问题而非安全漏洞。

### R-017 · 状态栏显示绝对路径

状态栏 / 错误对话框有时显示完整绝对路径（如 `C:\Users\xxx\Documents\笔记.md`）。这是**信息泄漏**（暴露用户名）但影响有限。

**建议**：显示文件名而非完整路径（状态栏），或截断显示（对话框）。

---

## 五、可维护性

### R-018 · TODO/FIXME 清单

grep **零结果**——项目没有遗留 TODO/FIXME/HACK。✅ **技术债清零**。

### R-019 · `#[allow]` 清单

共 4 处：

| 位置 | lint | 原因 |
|---|---|---|
| `application/event_subscriptions.rs` | `type_complexity` | 事件订阅链闭包签名较长 |
| `domain/preferences/value_objects/editor.rs` | `dead_code` | some 常量/字段被 feature gate 控制 |
| `presentation/views/diff_view.rs` | `clippy::needless_return` | diff 算法末尾有显式 return |
| `presentation/state/find_replace_ui_state/pattern.rs` | `clippy::borrowed_box` | 某些 Box<str> 签名 |

**结论**：4 处 allow 都是合理的（算法实现细节），不是滥用。✅

### R-020 · 魔法数字

grep 业务数字：
- `src/shared/time.rs` L76：`unwrap_or(u64::MAX)` — 饱和值合理
- 搜索上限 300（`WorkspaceSearch`）、图片尺寸上限（export）——**已声明为常量** ✅
- IME_GUARD_MS=800 / IME_ARM_MS=50 — 已声明为常量 ✅

### R-021 · 注释覆盖率（定性）

- domain 层 value_objects / aggregate：**注释充足**（每个方法都有文档注释）
- domain 错误类型：**注释充足**（每个变体有说明）
- presentation state：**注释中等**（字段 doc 齐全，部分方法缺失）
- infrastructure 实现：**注释中等**（关键函数有，辅助函数缺失）
- parse 函数内联注释：**极强**（pulldown_parser 每段逻辑前都有 why 注释）

### R-022 · 测试质量

- 1572 passed（其中单元测试占 ~1200，集成 + e2e + 启动 smoke 约 370）
- AC-1 拆分后所有子模块自带 tests.rs 外置
- 架构违规 → 0
- **测试覆盖率估算**（基于数量）：presentation state/diff/export/shortcuts 和 domain document 覆盖好；infrastructure fonts/export/rendering 覆盖较弱

---

## 六、修复任务清单（按优先级）

### P0 · 立即修复（影响稳定性/安全）

- [ ] **R-001b**：`diff_ui_state.rs` 运行期 unwrap → 改为 `Result` 传播
  - 修改：`src/presentation/state/diff_ui_state.rs` L153-156, L218
  - 验收：非法 diff 状态降级为提示，不 panic

- [ ] **R-012**：`ctx.input(\|i\| i.events.clone())` → 直接迭代
  - 修改：`src/app/frame/actions.rs` L213
  - 验收：clippy 无 `clippy::map_clone`，每帧少一次 Vec 分配

- [ ] **R-014**：文件保存路径 canonicalize 防穿越
  - 修改：`fs_document_repository.rs`, `fs_workspace_repository.rs`
  - 验收：路径含 `..` 被规范化；拖放路径安全

### P1 · 短期改进（影响性能/可维护性）

- [ ] **R-008**：editor_text 每帧 clone → 加 `.changed()` 早退出
  - 修改：`src/presentation/views/editor_view.rs` L196-287
  - 验收：TextEdit 内容未变时跳过领域命令构造

- [ ] **R-002**：`RegistryProvider::lock_read` 锁 poisoned 处理
  - 修改：`src/application/document/registry.rs` L152-173
  - 验收：poisoned 锁记录 warn 日志

- [ ] **R-009**：scan_workspace 移后台线程
  - 修改：`src/app/frame/workspace.rs` + facade 加 loading 状态
  - 验收：扫描大目录不卡顿 UI

- [ ] **R-010**：preview cache key hash 改用 ahash
  - 修改：`src/application/markdown/service/preview.rs`
  - 验收：cargo add ahash；基准测试提速

### P2 · 长期优化（锦上添花）

- [ ] **R-013**：Vec::new() 改 with_capacity（parse 路径）
- [ ] **R-017**：状态栏路径显示截断
- [ ] **R-001c**：event_subscriptions 闭包签名简化（减少 Arc::clone 数量）
- [ ] **R-006**：DialogRunner 的 std mpsc 改 crossbeam-channel（可选，减少 Mutex 开销）

### P3 · 文档/工程化

- [ ] 补充 `Cargo.toml` lint 配置（`[lints] workspace` + clippy::all + select allow）
- [ ] 给 4 处 `#[allow]` 补充 why 注释
- [ ] 考虑在 `lib.rs` 的 clippy lint 清单中把 `unwrap_used` 设为全局 deny（当前只有 domain 层 deny）

---

## 七、好设计正例（值得保持）

1. **四层架构 + check-arch 运行时校验**：架构违规零发生。
2. **parse.rs 行为级文档**：pulldown_parser/parse/end.rs 每段 `if matches!(tag_end, TagEnd::TableRow)` 前都有 why 注释（"表头行不带 TableRow 包裹" 这种实现细节）。
3. **DialogRunner 线程安全**：std::thread + mpsc + try_recv 的经典安全跨线程模式。
4. **atomic_write 原子落盘**：`.tmp` → `sync_all` → `rename`，断电安全。
5. **Markdown 预览渲染走 RenderedBlock 枚举而非 HTML**：彻底消除 XSS 攻击面。
6. **AC-1 达成后所有文件 ≤ 499 行**：可维护性边界成立。
7. **项目级禁止 unwrap 的 lint**：clippy deny 清单 + domain.rs + lib.rs 三处声明。
8. **零 TODO/FIXME/HACK**：技术债清零。
