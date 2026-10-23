# 侧边栏「搜索」标签卡顿排查报告

> 结论先行：卡顿不是渲染慢，也不是「组件重复挂载」——是**默认的「当前文档」范围在应用层把范围过滤放在了读盘之后**，每次搜索都把整个工作区逐文件读一遍并解码；而视图**每帧还要跑两次**这样的搜索。千文件工作区下每帧 ≈ 163 ms（release 实测等价成本），必然卡死。
>
> 复现命令：`cargo test --release --test presentation -- search_perf --nocapture`

---

## 1. 触发条件与影响范围

| 项 | 结论 |
|---|---|
| 触发条件 | 侧栏在**搜索**标签（可见且未折叠）**且**输入框非空（`SidebarUiState::should_search()`）。此状态下**每一帧**都会重算搜索 |
| 加剧条件 | ① 工作区文件数（线性，最致命）② 活动文档体积（线性）③ 打开的文档数量 |
| 用户可见症状 | 切到搜索标签后整窗冻结；在输入框里打字逐字卡顿；鼠标划过侧栏/滚动结果也卡（每次重绘都重跑搜索） |
| 不受影响 | 查询为空时 `search` 提前返回，无开销；搜索标签不可见时视图根本不调用 |
| 与「大文件」的关系 | 1MB 文档整帧 release ≈ 11.5 ms（旧实现），**不构成「严重」卡顿**——「严重」只出现在文件多的工作区，说明卡顿主导项是读盘而不是文档大小 |

## 2. 全链路调用路径（逐帧）

```text
main_view::show
└─ sidebar_view::show（仅侧栏可见时）
   └─ 标签分派 → sidebar_view/search.rs:search_tab        ← 只有选中「搜索」才挂载，无重复挂载
      ├─ TextEdit::singleline                              ← 每帧重建，changed() 才回写 query
      ├─ scope_row                                         ← 范围切换（CurrentDocument 默认）
      ├─ toggle_row
      │  └─ count_hits → EditorApp::search_workspace  ①    ← 旧实现：第 1 次全量搜索
      ├─ （结果列表）EditorApp::search_workspace      ②    ← 旧实现：第 2 次全量搜索
      └─ group_search_hits → group_hits → 逐行 Row
```

`search_workspace` 之下：

```text
EditorApp::search_workspace                       src/app/frame/outline.rs:113
└─ WorkspaceAppService::search                    src/application/workspace/service.rs:282
   ├─ gather_candidates                            service.rs:369
   │  ├─ DocumentContentProvider::open_documents   → 每份打开文档做一次 rope→String 全文快照
   │  └─ 磁盘分支：遍历工作区条目
   │     └─ FsWorkspaceRepository::read_content    fs_workspace_repository.rs:274
   │        └─ fs::read + decode_public             ← 逐文件读盘 + 编码探测（本次卡顿主因）
   ├─ search_text                                  service.rs:484   ← O(字符数 × 模式长)，旧实现每字符分配
   └─ 命中回填 line / col / snippet
```

旧实现的关键缺陷：`gather_candidates(!only_open_documents)` 对「当前文档」范围也传 `include_disk = true`，**先把整个工作区读进内存**，再用 `candidates.retain(|id| *id == active)` 丢掉除目标外的一切。函数文档当时写的却是「过滤放在候选收集之后、读盘之前」——**注释与实现相反**，这正是缺陷长期未被发现的原因。

## 3. 六项排查逐个结论

| # | 排查项 | 结论 | 依据 |
|---|---|---|---|
| 1 | 全量重渲染 / 重复组件挂载 | 无重复挂载（标签分派只走一个分支）。但**同一帧内重复执行了两次全量搜索**（计数 + 列表） | `sidebar_view/search.rs:136` 旧 `count_hits` + `search_tab` 各调一次 |
| 2 | 结果列表批量创建节点 / 无虚拟列表 | 非瓶颈。egui 即时模式每帧重建全部行，但实测渲染增量 ≈ 0.0–0.2 ms（50 条命中，release）；上限 300 条受 `MAX_TOTAL_HITS` 兜底 | 帧时间 6.02 ms ≈ 单次搜索 6.24 ms |
| 3 | 未防抖的高频事件监听 | 搜索框只有 `TextEdit::changed()`（非每字符订阅），**但没有「输入未变就不重算」的短路**：任何重绘（悬停、滚动、光标闪烁）都会重跑搜索 | `search_tab` 每帧无条件调用 |
| 4 | O(n²) 以上遍历 / 重复计算 | 三处：① 逐字符比较时**每次比较分配两个 `Vec<char>`**（1MB 文档数十万次堆分配）② **每条命中重扫全文**求行号（`text[..start_off]` 数换行 + `rfind`，50 条命中 = 50 遍全文）③ `group_hits` 的 `find` 是 O(命中 × 文件)，但命中上限 300，实测可忽略 | `service.rs:467`、`service.rs:484`、`sidebar_ui_state/search.rs:190` |
| 5 | 内存泄漏 / 未清理定时器、订阅 | 无。搜索路径不注册定时器（全仓统一走 `FrameScheduler::arm/flush`），不订阅事件，结果持有 ≤300 条命中，无跨帧增长 | — |
| 6 | 重复接口请求 / 未取消 | 本应用无网络请求；等价物是**磁盘读**。旧实现每帧重复读全工作区两次，且读到的内容立即被丢弃（不可取消，因为是同步阻塞） | 见 §2 |

## 4. 根因（按代价排序）与实测证据

| 序号 | 根因 | 位置 | 修复前（release） | 修复后（release） |
|---|---|---|---|---|
| R1 | 「当前文档」范围也逐文件读盘 | `service.rs:282/369` | 1001 文件工作区：**81–146 ms/次**（文件缓存冷热决定）；视图 ×2 → **≈160–290 ms/帧** | **0.00 ms/次**，切换首帧 **0.69 ms** |
| R2 | 一帧两次全量搜索 | `search.rs:136/17` | 1MB 文档：一帧两次 **11.5–11.7 ms** | 一帧一次 **6.0–6.4 ms**（p95 6.5） |
| R3 | 逐字符分配 + 命中行号重复全文扫描 | `service.rs:467/484` | 1MB 文档单次 12.1 ms（debug） | 11.1 ms（debug）；ASCII 快路径 + 增量行号 |

R1 是「严重卡顿」的唯一来源，量级随工作区文件数线性增长——这就是「换台机器/换个目录就卡」的原因。
R2、R3 在单文档场景把每帧成本再放大一倍与一截。

## 5. 修复内容

| 文件 | 改动 |
|---|---|
| `src/application/workspace/service.rs` | `search`：`include_disk = only_document.is_none() && !only_open_documents`，范围过滤与读盘由**同一个**判断决定，不再「读完再丢」；`gather_candidates` 增加 `only_document` 形参，该范围下按 id 单取全文（不再为其余打开文档做全文快照） |
| `src/application/workspace/service.rs` | `search_text`：新增 `char_eq_ignore_case`（ASCII 走 `eq_ignore_ascii_case`，其余 `to_lowercase().eq()` 迭代器比较，**零分配**）；行号/行首改为**跨命中增量**统计（总 O(n)，替代 O(命中数 × n)） |
| `src/presentation/views/sidebar_view/search.rs` | `toggle_row` 返回本帧唯一一次搜索结果，计数与列表**同源复用**；删除 `count_hits` |
| `src/app/frame/outline.rs`、`src/presentation/views/sidebar_view.rs` | 更正文档：原注释描述的「读完再丢」行为与实现不符，已按新语义重写（含「范围决定读不读盘」的判据） |
| `tests/presentation/search_perf_test.rs` | 新增性能门禁（见 §6） |

## 6. 验证

### 6.1 测量方法

Windows 下 `perf`/WPA 采样需要管理员与 ETW 环境，且本缺陷是**同步阻塞的确定性开销**，火焰图只会反复显示同一个栈。改用可复现的帧内分段计时（等价于 scope 采样，且能长期进 CI 作门禁）：

```text
cargo test --release --test presentation -- search_perf --nocapture
```

三段口径：**单次 `search_workspace`** / **侧栏整帧**（含渲染）/ **从「文件」标签切到「搜索」的首帧**。另以「旧实现等价成本」两组对照量化收益：一帧两次搜索、逐文件 `read_file` 全部工作区文件。

### 6.2 结果（release，Windows x86_64）

| 场景 | 旧实现等价 | 修复后 |
|---|---|---|
| 1001 文件工作区·「当前文档」单次搜索 | **81–146 ms**（冷热缓存决定） | **0.00 ms** |
| 1001 文件工作区·切到搜索标签首帧 | ≈160–290 ms（2 × 单次） | **0.69 ms** |
| 1MB 文档·50 条命中·一帧 | 11.5–11.7 ms（两次搜索） | **6.0–6.4 ms**（p95 6.5） |
| 1MB 文档·切到搜索标签首帧 | ≈11.5 ms | **6.2 ms** |
| 侧栏「文件」标签整帧（对照） | — | 0.03 ms |

验收口径「切到搜索后交互响应 < 100 ms」：**1MB 文档 6.2 ms（16× 余量）**；**千文件工作区 0.69 ms（145× 余量）**。debug 口径同向更慢（1MB 整帧 27.1 → 11.9 ms），按 `metrics.md` §3.2.10 的约定只在 release 做硬断言，debug 仅打印数字。

### 6.3 门禁与回归

- `tests/presentation/search_perf_test.rs`：
  - 「当前文档」单次 < 10 ms（旧实现 81 ms 起，任何重新读盘都会顶破；两档均断言）；
  - 切到搜索标签首帧 < 100 ms（两个场景；release 断言，debug 仅打印）。
- `cargo test`：2107 个用例通过，0 失败（含侧栏 55 条、应用层 163 条）。
- `cargo fmt --check`、`cargo clippy --all-targets`：干净。

## 7. 未处理的项与理由

| 项 | 结论 |
|---|---|
| 搜索结果的虚拟列表 | 不做。实测 50 条命中的渲染增量为 0.0–0.2 ms，300 条上限下仍远低于一帧预算；引入 `show_viewport` 会打散现有分组渲染，收益不抵复杂度 |
| 结果缓存（按 revision 失效） | 不做。仓库既定纪律是「搜索结果不在 UI 状态里缓存」（否则易出现「文件改了、结果还是旧的」）。修完 R1–R3 后单帧成本已进预算，缓存只会新增一类失效缺陷 |
| `group_hits` 的 O(命中 × 文件) | 保留。命中总数硬上限 300，最坏 300 × 300 次字符串比较，实测不体现在帧时间里 |
| 搜索的执行「取消」 | 无需。同步本地搜索没有可取消的异步句柄；旧实现的「重复请求」问题已由 R1/R2 消除 |
| 1MB 文档仍 6 ms/次 | 属 `NFR-PERF-04`「1MB 档尽力而为」范围；继续压需要把 `Vec<(usize, char)>`（1MB 中文 ≈ 5.6 MB）改成流式滑窗，收益约 2–3 ms，性价比不足 |
