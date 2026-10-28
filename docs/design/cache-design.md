# RustMark 缓存技术文档与优化方案

> 适用范围：RustMark（基于 `egui`/`eframe` 的桌面 Markdown 编辑器）进程内缓存子系统。
> 本文档基于 `src/` 当前实现逐文件核对，所列文件、常量、行号均对应实际代码。
> 配套性能基线见 `docs/spec/metrics.md`（§3.2.x 各段）。

---

## 1. 概述

RustMark 的"缓存"不是一个独立中间件，而是一组**进程内、分层、按文档版本号（`revision`）语义失效**的内存缓存，外加一层**防抖（debounce）节拍器**。它要解决的不是"跨进程共享数据"，而是"在每一渲染帧（60fps）内，把若干 O(全文) 的昂贵计算（语法高亮、Markdown 渲染、拼写检查、字数统计、逐行排版）降到可忽略或仅付一次"。

核心设计信条：

- **内容即版本号**：文档每次编辑 `revision` 自增，缓存键一律带 `revision`。内容没变 → 命中；内容变了 → 语义失效。缓存与"重算"结果**完全一致**，不存在"颜色滞后 / 数字滞后"。
- **防抖优于缓存首算**：缓存只能省"未变的帧"，省不掉"变化后的第一次重算"。而第一次重算本身就很贵（高亮 10KB≈56.5ms、统计 1MB≈9.5ms），所以在其上再叠加 debounce，把连续打字合并成"停手后算一次"。
- **零拷贝共享**：不可变快照（高亮片段表、整篇 `Galley`）用 `Arc` 跨帧共享，返回路径不克隆实体。
- **分层就近**：应用层缓存"计算结果"，表现层缓存"排版产物"，帧层缓存"单文档瞬时视图"，进程级 `static` 缓存"加载一次即不变的资源"。

---

## 2. 缓存选型依据

| 维度 | 选择 | 理由（来自代码注释与实测） |
|---|---|---|
| 存储位置 | 进程内内存，无外部缓存（Redis/磁盘） | 单用户桌面程序，数据量小、要求子帧延迟；外部缓存的 IPC 延迟与运维复杂度得不偿失。 |
| 依赖选型 | 自研 + `ahash::HashMap`，未引入 `lru`/`moka` 等成熟库 | 需要**按 `revision` 语义失效**而非通用 TTL，需要 `Arc` 零拷贝共享，需要精细控制淘汰顺序；自研可把"键语义"与"命中路径零分配"钉死（`highlight_cache.rs`、`spell_cache.rs` 注释）。代价是部分实现出现了淘汰顺序不一致（见 §7 优化项 G1）。 |
| 失效策略 | 版本号精确失效，而非 TTL | 文档是版本化真相源，`revision` 是确定性信号；TTL 会引入"陈旧色/陈旧数"。 |
| 与计算的配合 | 缓存层之上再叠防抖 | 缓存救不了"变化后的首算"，首算本身 O(全文) 且昂贵，防抖把连续编辑合并成单次结算。 |
| 并发模型 | 跨线程缓存 `Arc<Mutex<>>`；UI 线程专有缓存 `thread_local!{RefCell<>>}` | 应用层服务被多线程共享（`Arc<RwLock<DocumentRegistry>>` 等）；表现层只活在 `eframe::App` 主线程，无并发，用 `RefCell` 免去锁开销。 |
| 粒度 | 整文档 Galley 缓存 + 逐行增量布局；单槽统计缓存 + 真 LRU 高亮缓存 | 访问模式决定：行布局是"全顺序扫描全部行"，任何容量 < 行数的 LRU 命中率恒为 0（`highlight_cache.rs` §"为什么不是定容 LRU"），故改用"显式保留上一版全部行"。 |

---

## 3. 整体架构

### 3.1 分层与线程模型

```
┌──────────────────────────────────────────────────────────────┐
│  进程级 static（加载一次，跨文档/跨会话共享）                    │
│   · syntect SYNTAXES / CACHE  (infrastructure/highlighting/…)  │
│   · spell dictionary         (infrastructure/spell/dictionary)│
└──────────────────────────────────────────────────────────────┘
            │ 被下列缓存调用（资源已预热）
┌──────────────────────────────────────────────────────────────┐
│  应用层（MarkdownAppService / DocumentAppService）— Arc<Mutex> │
│   · HighlightCache   真LRU cap=32  键(id,rev,lang,range)→Arc<Vec<SpanView>> │
│   · SpellCache       近似LRU cap=32 键(id,rev)→Vec<(usize,usize)>          │
│   · RenderCache      近似LRU cap=32  键(id,rev)→Vec<RenderedBlock>          │
│   · HighlightPace    debounce 节拍器 cap=32                                │
│   · DebouncePolicy   渲染防抖 150ms（纯函数）                               │
│   · stats_cache     单槽 Mutex<Option<(id,rev,StatsView)>>                  │
└──────────────────────────────────────────────────────────────┘
            │ 计算结果（SpanView / RenderedBlock / StatsView）
┌──────────────────────────────────────────────────────────────┐
│  表现层（presentation，UI 线程）— thread_local! RefCell        │
│   · WHOLE_CACHE    整文档 Galley 单槽（主题签名+文本等 7 元组键）│
│   · LINE_LAYOUT    逐行 Galley 增量缓存（保留上一版全部行）      │
│   · IMAGE_CACHE    图片引用→纹理 HashMap（无上限）              │
└──────────────────────────────────────────────────────────────┘
            │ 排版产物 / 纹理
┌──────────────────────────────────────────────────────────────┐
│  帧层（app/frame，UI 线程）— RefCell 单槽                      │
│   · LineIndexCache  单槽 (key, Rc<LineIndex>) 键=(doc,rev,len) │
│   · StatsPace       状态栏统计防抖 150ms                       │
└──────────────────────────────────────────────────────────────┘
```

### 3.2 缓存组件清单

| 缓存 | 文件 | 类型 | 容量 | 键 | 值 | 失效 |
|---|---|---|---|---|---|---|
| HighlightCache | `application/markdown/service/highlight_cache.rs` | 真 LRU（`order: Vec`） | 32 | `(DocumentId, rev, lang, range)` | `Arc<Vec<SpanView>>` | `rev/lang/range` 任一不同；`invalidate` / `invalidate_all` |
| SpellCache | `application/markdown/service/spell_cache.rs` | **真 LRU**（`order: Vec` 确定性驱逐，G1 已落地） | 32 | `(DocumentId, rev)` | `Vec<(usize,usize)>` 错词区间 | `rev` 不同；`invalidate` / `invalidate_all` |
| RenderCache | `application/markdown/service/render_cache.rs` | **真 LRU**（G1 已落地） | 32 | `(DocumentId, rev)` | `Vec<RenderedBlock>` | `rev` 不同；主题切换 `invalidate_all`；关闭文档 `invalidate`（G2） |
| HighlightPace | `highlight_cache.rs` | debounce 节拍表 | 32 | `DocumentId` | `(last_rev, seen_at)` | 容量满任意踢 |
| stats_cache | `application/document/service.rs` | 单槽 | 1 | `(DocumentId, rev)` | `StatsView` | `rev` 不同（覆盖写） |
| LineIndexCache | `app/frame.rs` | 单槽 `RefCell` | 1 | `(Option<Doc>, rev, text_len)` | `Rc<LineIndex>` | 键变化覆盖 |
| WHOLE_CACHE | `presentation/theme/mapper/highlight_cache.rs` | 整文档 Galley 单槽（thread_local） | 1 | `(主题签名, 换行宽, Galley, 查找签名, 着色态, 度量签名, 图集令牌)` | `Arc<Galley>` | 键任一不同（含 `galley.text()==text` 文本比对） |
| LINE_LAYOUT | 同上 | 逐行 Galley 增量（thread_local） | 1 文档全部行 | 同上 + 上一版行内容 | `Vec<Arc<Galley>>` + 签名 | 主题/度量/图集令牌变；文本变→前后缀 diff 定位变更行 |
| IMAGE_CACHE | `presentation/views/preview_view.rs` | 图片纹理 **LRU**（thread_local，`ImageCache`） | **64**（G3 已落地） | 图片引用字符串 | `ImageEntry{texture,size,error}` | 超容淘汰最旧者，纹理随 `ImageEntry` 析构自动释放（G3） |

---

## 4. 读写流程

### 4.1 语法高亮（`MarkdownAppService::highlight`，`highlight.rs`）

```
每帧（编辑区请求全文 span）
  │
  ├─ provider.revision_and_len(doc)        ← O(1)，仅取版本号+字节长度（问题046）
  ├─ len > TRUNCATE_THRESHOLD_BYTES(500KiB)? → 返回 empty_spans()（大文件降级，FR-06）
  ├─ HighlightPace.settled(doc,rev,now,150ms)
  │     ├─ 首次见该文档 → true（立刻上色，避免闪烁）
  │     ├─ rev 变了     → 记下时刻，返回 false（本帧不上色，打字期≈0 成本）
  │     └─ rev 未变     → 距上次变化 ≥150ms ? true : false
  ├─ false → 返回 empty_spans()            ← 防抖期跳过，不查缓存也不重算
  ├─ HighlightCache.get(id,rev,lang,range)  ← 真 LRU 命中检查
  │     └─ 命中 → 返回共享 Arc（不克隆实体）
  └─ 未命中 → fetch 全文 → syntect 解析(O全文) → insert(Arc) → 返回
```

关键：**防抖期与命中路径只消耗 O(1) 元数据**；只有真正要走解析器时才 `fetch` 全文（`highlight.rs` 注释"帧路径上只碰 O(1) 元数据"）。

### 4.2 字数统计（`DocumentAppService` + `StatsPace`）

```
状态栏每帧调用 get_stats（同一文档查两次：写作目标进度 + 字数项）
  │
  ├─ stats_cache 单槽：(id,rev) 命中? → 返回（零成本）
  ├─ StatsPace：doc 改变/首算 → 直接 compute_stats（避免打开时先显 0 再跳真值）
  └─ 否则：距上次 rev 变化 ≥ STATS_DEBOUNCE_MS(150)?
         ├─ 是 → compute_stats（1MB≈9.5ms）回填 stats_cache + 更新 Pace
         └─ 否 → 返回上一次静止时的结果（打字期完全不重算）
```

浮层 `stats_view` 走 `EditorApp::current_stats`：点击时才算、即时，不经防抖（见 `frame.rs` `StatsPace` 注释）。

### 4.3 编辑区逐行排版（表现层 `highlight_layout` + `highlight_cache.rs`）

```
TextEdit 的 layouter 每帧要整篇 Galley
  │
  ├─ WHOLE_CACHE：键含 galley.text()==text 比对 → 文本/主题/换行/度量/图集未变 → 直接返回（空闲帧零开销）
  └─ 否则 LINE_LAYOUT：用上一版"逐行 Galley"与新文本做前后缀 diff
         ├─ 仅变更行重新排版（reshaped_lines）
         └─ 其余行直接复用 Arc<Galley>（命中 cached_lines）
    以 mem::take 整条移出旧表、用完写回（避免 58k 行击键克隆，问题052）
```

图集令牌（`atlas_token`）进键：egui 重建 `Fonts`/`FontAtlas` 后旧 `Arc<Galley>` 的字形 `uv_rect` 失效，令牌变了即整篇重排取新坐标（V11-17 修复乱码）。

### 4.4 图片纹理（`preview_view.rs`）

```
预览遇到图片引用 url
  ├─ IMAGE_CACHE 命中 → 返回 ImageEntry（已解码纹理）
  └─ 未命中 → AssetLoader 取字节 → decode → ctx.load_texture → 插入 IMAGE_CACHE
```
尺寸参数 `=30x30`/`=600x200` 不进缓存键（每帧重算成本可忽略）。

---

## 5. 关键配置参数

| 参数 | 值 | 位置 | 含义 / 调节影响 |
|---|---|---|---|
| `HIGHLIGHT_CACHE_CAPACITY` | `32` | `application/markdown/service.rs` | 高亮/拼写/高亮节拍器共用容量。多文档（上限 30）下留有余量。 |
| `RENDER_CACHE_CAPACITY` | `32` | `application/markdown/service.rs`（**唯一事实来源**，装配处 `app/bootstrap.rs` 向下 `use`） | 渲染缓存实际容量。`DEFAULT_RENDER_CACHE_CAPACITY=16` 的冲突常量已在 G4 删除，全仓只此一处（G4 已落地）。 |
| `IMAGE_CACHE_CAPACITY` | `64` | `presentation/views/preview_view.rs` | 图片纹理 LRU 容量上限（G3 已落地）；超出淘汰最旧者，纹理随析构释放。 |
| `TRUNCATE_THRESHOLD_BYTES` | `500*1024` | `application/markdown/service.rs:54` | 大文件降级阈值：超此尺寸源不解析高亮/拼写/导航（FR-06）。 |
| `LARGE_FILE_BYTES` | `500*1024` | `presentation/theme/mapper/highlight_cache.rs:66` | 表现层同样阈值：超此尺寸停止 span 着色，仅纯文本排版。 |
| `STATS_DEBOUNCE_MS` | `150` | `app/frame.rs:144` | 状态栏统计防抖窗口。 |
| `policy.interval_ms()` | `150` | `MarkdownAppService.policy`（DebouncePolicy） | 高亮防抖窗口（FR-01）。 |
| `MAX_OPEN_DOCUMENTS` | 默认 `12`，范围 `1..=30` | `domain/preferences/value_objects/editor.rs:139,145` | 可同时打开文档数上限，间接约束所有按文档键的缓存规模。 |

> 注：防抖间隔、大文件阈值目前在代码里为编译期常量，未暴露到设置页（§7 G4 建议收口）。

---

## 6. 适用场景

| 缓存 | 适用场景 | 不适用 / 注意 |
|---|---|---|
| HighlightCache | 编辑区每帧请求全文 span、文档未变的滚动/空闲/切面板帧 | 连续打字期被防抖跳过（短暂无色）；>500KiB 文档降级为空。 |
| SpellCache | 编辑区每帧可能请求错词区间的 900KB 级文档 | 真 LRU（G1 已落地），多文档切标签不再误踢；关闭文档统一失效（G2）。 |
| RenderCache | 预览区 Markdown 渲染，文档未变时免重解析 | 主题切换整体失效（`invalidate_all`）；关闭文档统一失效（G2，G1 已落地真 LRU）。 |
| stats_cache | 状态栏每帧两次统计查询 | 仅单槽，切文档必失效一次（已是最优，因同帧查同一活动文档）。 |
| LineIndexCache | 一帧内状态栏/编辑区多次索取行号、行数 | 仅缓存当前活动文档，切文档重建（本就只该付一次）。 |
| WHOLE_CACHE / LINE_LAYOUT | 编辑区排版，绝大多数帧文本/主题未变或仅局部变 | 仅保单文档上一版，多文档切换整篇重排（G5 探索）。 |
| IMAGE_CACHE | 预览同一图片在多文档/多块复用 | 容量 64 + LRU（G3 已落地），显存/内存可预测；当前帧渲染的图每帧触碰，不会被误踢。 |

---

## 7. 缓存优化方案

### 7.1 优化目标

1. **G1 消除近似 LRU 的任意淘汰缺陷，统一为确定性 LRU**，消除多文档切标签时的重算尖峰。
2. **G2 显式失效已关闭文档的缓存**，控制长跑常驻内存，杜绝"打开/关闭循环"式泄漏。
3. **G3 为 `IMAGE_CACHE` 引入容量上限与 LRU 淘汰**，让 GPU/内存占用可预测。
4. **G4 把缓存相关旋钮收口为可配置参数**，并消除 `RenderCache` 双常量（16 vs 32）不一致。
5. **G5（探索性）跨文档行布局复用**，降低多文档切换的整篇重排成本。
6. **G6 补齐缓存命中率可观测性**，为上述优化提供前后对照基线。

### 7.2 具体措施

#### G1 — 把 SpellCache / RenderCache 改为与 HighlightCache 同构的真 LRU
- **现状**：`HighlightCache` 已用 `order: Vec<DocumentId>` 维护访问序（命中移尾、超容踢头，问题043 已修）；但 `SpellCache::insert`（`spell_cache.rs:58-65`）与 `RenderCache::insert`（`render_cache.rs:69-77`）仍用 `self.entries.keys().next()`——`HashMap` 迭代序任意，可能正好踢掉下一次要命中的文档。
- **措施**：为 `SpellCache`、`RenderCache` 各加 `order: Vec<DocumentId>`，复制 `HighlightCache::touch`/`insert` 的访问序维护；命中时把键移尾，超容时从 `order` 头踢最旧者。
- **预期收益**：多文档（上限 30，容量仅 32）下，切一轮标签不再触发"恰好踢中→整篇重算"的尖峰。对应实测基线：拼写 900KB 文档单次 94ms（debug），渲染 Markdown 解析 O(全文)。
- **潜在风险**：需保证 `order` 与 `entries` 键集同步（已有 `touch` 防御范式可复用）；轻微写入开销，可忽略。

#### G2 — 文档关闭时统一失效其所有缓存条目
- **现状**：各缓存仅在 `invalidate(doc_id)` / `invalidate_all`（主题切换/保存）时被清理。代码未见"文档关闭"统一钩子调用 `invalidate`，导致关闭文档后其在高亮/拼写/渲染缓存中的条目滞留。
- **措施**：在文档关闭流程中，对 `HighlightCache`、`SpellCache`、`RenderCache` 统一调用 `invalidate(id)`；渲染缓存在全失效处保持不变。
- **预期收益**：常驻内存随工作集收敛，避免长会话内存单调增长。
- **潜在风险**：必须以"被关闭的 ID"精确调用，绝不波及活动文档（误失效会触发误重算）；线程安全由各缓存 `Arc<Mutex<>>` 保证，无需额外处理。

#### G3 — IMAGE_CACHE 容量上限 + LRU 淘汰 + 纹理释放
- **现状**：`preview_view.rs:132` 的 `IMAGE_CACHE` 注释明确"不设淘汰上限"，理由是"有界于用户内容"。但 `ImageEntry.texture` 是 egui GPU 纹理，多图/大图会持续占显存。
- **措施**：引入容量上限（建议 64 张）与访问序；淘汰时显式释放纹理（`TextureHandle::destroy()` 或 drop `ImageEntry`）。渲染当前帧所引用的图必须在当帧 `touch`（防误踢）。
- **预期收益**：显存/内存占用可预测，长文档多图场景不再无限增长。
- **潜在风险**：淘汰后重访需重新 `decode`（CPU 成本）并可能短暂占位闪烁一次；实现须保证"正在渲染的图不会被踢"。

#### G4 — 缓存旋钮配置化 + 消除双常量
- **现状**：容量/阈值/防抖间隔散落为常量；且 `DEFAULT_RENDER_CACHE_CAPACITY=16`（`service.rs:57`）与 `RENDER_CACHE_CAPACITY=32`（`bootstrap.rs:64`）并存，注释亦称"开满时缓存必然溢出"，属隐患。
- **措施**：将高亮/渲染/拼写容量、`TRUNCATE_THRESHOLD_BYTES`、防抖间隔收口进（高级）设置页，并 `clamp` 到合理范围；删除重复的 `DEFAULT_RENDER_CACHE_CAPACITY`，统一由 `RENDER_CACHE_CAPACITY` 装配。
- **预期收益**：用户可在"流畅 vs 内存"间权衡；消除双常量导致的语义歧义。
- **潜在风险**：用户误设极端值可能导致 OOM 或抖动——须 `clamp` + 设置文案提示；默认保持 32 不变以保证现状行为。

#### G5（探索性）— 跨文档行布局复用
- **现状**：`WHOLE_CACHE` / `LINE_LAYOUT` 仅保单文档上一版。多文档切换回已排过的文档仍整篇重排。
- **措施**：引入受 `MAX_OPEN_DOCUMENTS`（≤30）约束的"小容量文档→行布局"映射（带内存预算上限）。
- **预期收益**：多文档切换编辑区零重排。
- **潜在风险**：内存随文档数线性增长（须预算上限）；实现复杂（整文档键须不含 viewport）。**建议作为后续探索，先做 G1–G4。**

#### G6 — 命中率可观测基线
- **现状**：`HighlightCache`/`SpellCache` 已有 `hits`/`misses`，表现层有 `last_highlight_stats()`、`line_cache_len()`，但 `stats_cache_hits` 仅有命中计数无未命中。
- **措施**：补齐各缓存未命中计数 + 调试面板聚合命中率。
- **预期收益**：G1–G3 前后可量化对照，作为回归基线。
- **潜在风险**：低。

### 7.3 预期收益汇总（基于现有实测基线）

| 优化项 | 收益的量化依据 | 主要风险 |
|---|---|---|
| G1 真 LRU 统一 | 消除切标签重算尖峰（拼写 900KB 单次 94ms；渲染 O(全文)） | 低（同步 order 与 entries） |
| G2 关闭失效 | 长会话内存收敛，消除泄漏 | 低（精确 ID，不波及活动文档） |
| G3 图片 LRU | 显存/内存可预测 | 中（重访解码成本 + 短暂占位） |
| G4 旋钮配置 | 用户可权衡；消除双常量 | 低–中（需 clamp/文案） |
| G5 跨文档布局 | 多文档切换零重排 | 高（内存线性增长、实现复杂）— 探索项 |
| G6 可观测 | 提供前后对照基线 | 低 |

### 7.4 实施建议顺序

1. **G4（消除双常量 + 收口配置）** → 低风险、先止血隐患，顺带为其余项提供调节入口。
2. **G1（真 LRU 统一）** → 复制已验证范式，收益确定、风险低。
3. **G2（关闭失效）** → 定位关闭钩子，收益确定。
4. **G6（可观测）** → 在 G1/G2 前后量化收益。
5. **G3（图片 LRU）** → 评估显存基线后实施，注意纹理释放与防误踢。
6. **G5（探索）** → 独立技术预研，不在本期主线。

### 7.5 实施状态（代码已落地）

以下优化项已在本仓库实现并通过 `cargo test` / `cargo clippy -D warnings` / `check-arch` 门禁：

| 优化项 | 落地内容 | 关键改动 |
|---|---|---|
| **G1 真 LRU 统一** | `SpellCache`、`RenderCache` 各加 `order: Vec<DocumentId>`，`get` 命中 `touch`、超容从序头确定性驱逐；新增单元测试钉死「驱逐最久未用而非任意键」 | `spell_cache.rs`、`render_cache.rs` |
| **G2 关闭失效** | `MarkdownAppService::clear_cache` 在单文档失效分支补 `spell_cache.invalidate`；`event_subscriptions` 的 `DocumentClosed` 处理器调用 `clear_cache(Some(id))` | `service.rs`、`event_subscriptions.rs` |
| **G3 图片 LRU** | `IMAGE_CACHE` 由裸 `HashMap` 改为 `ImageCache`（容量 64 + 访问序），超容淘汰最旧者，纹理随 `ImageEntry` 析构自动释放 | `preview_view.rs` |
| **G4 双常量消除** | 删除 `DEFAULT_RENDER_CACHE_CAPACITY`，`RENDER_CACHE_CAPACITY=32` 收为 `application` 层唯一常量，装配处向下 `use`；3 个测试 + `bootstrap_test` 同步更新 | `service.rs`、`bootstrap.rs`、测试文件 |
| **G6 可观测** | 新增 `MarkdownAppService::cache_stats()`（汇总高亮/拼写/渲染命中与未命中 + `hit_rate()`）；`RenderCache` 补 `hits`/`misses`；`DocumentAppService` 补 `stats_cache_misses` | `service.rs`、`render_cache.rs`、`document/service.rs`、`queries.rs` |

**G5（跨文档行布局复用）** 仍按 §7.4 第 6 条为独立技术预研，本期未实现（内存随文档数线性增长、整文档键不含 viewport，风险高）。

> 实测收益对照基线见 `docs/spec/metrics.md`；G6 的 `cache_stats()` 可在「优化前/后」各采一次快照做前后对照。

#### 复审记录（2026-10-17，通用 `cache-audit` skill 复查）

对源码做了一次**新鲜侦察**（非凭记忆），逐项核对现状：

- 四类缓存（`HighlightCache`/`SpellCache`/`RenderCache`/`IMAGE_CACHE`）均已确认为**真 LRU + 访问序 + 守卫测试**，G1/G2/G3/G4/G6 落地状态与上表一致，无回退。
- **修正两处文档漂移**：`render_cache.rs:1` 与 `highlight_cache.rs:1` 的模块头注释仍写「近似 LRU」，与各自结构体 doc（「真 LRU」）自相矛盾——已改为「真 LRU」。这正是「复审必须重新 grep、不可凭记忆」的价值。
- **`HighlightPace::seen` 的任意淘汰（`keys().next()`）判定为刻意设计、非缺陷**：该表只存「版本 + 时刻」这类廉价 pacing 元数据，误淘汰代价至多是该文档下次编辑被当成「首次见到」而多上色一次（保守降级，非重算尖峰、非陈旧显示）。已在 `remember` 处补 doc 说明，防止未来被过度「修」成 LRU。
- 门禁（复审后）：`cargo fmt --check` 干净、`cargo clippy --all-targets --all-features -- -D warnings` 通过（仅沙箱 `os error 5` 锁文件告警）、`cargo test` lib 369 / application 156 / integration 79 全绿。

- 缓存实现：`src/application/markdown/service/{highlight_cache,render_cache,spell_cache,highlight}.rs`、`src/application/document/service.rs`、`src/app/frame.rs`、`src/presentation/theme/mapper/highlight_cache.rs`、`src/presentation/views/preview_view.rs`
- 装配与参数：`src/app/bootstrap.rs`、`src/application/markdown/service.rs`
- 进程级静态资源：`src/infrastructure/highlighting/syntect_highlighter.rs`、`src/infrastructure/spell/dictionary.rs`
- 性能基线：`docs/spec/metrics.md` §3.2.1（高亮 56.5ms）、§3.2.4–§3.2.7（统计/排版/1MB 击键帧）、§3.2.5（统计 9.5ms）
