# rustmark-core 模块与目录结构评审（2026-10-24）

> 只读评估，未修改任何代码。基线：工作区含未提交的 D-6/D-7 在途改动；`rustmark-core`
> 拆分落盘于 `2f4fd07`（2026-10-23，距本评审仅 1 天）。本文所有数字均为本会话实测，
> 测量口径随文标注。变更频率按「拆分前 `src/` 路径 + 拆分后 `crates/rustmark-core/src/`
> 路径」合并计数（同一批文件迁移前后的历史）。

## 0. 结论摘要

**臃肿判定：成立。但性质是「crate 粒度错配」，不是「层内混乱」。**

1. core 46,375 行 / 238 文件中，**真正的领域脊柱（domain + application + shared）仅
   16,733 行 / 36.1%**；**63.9%（29,642 行）是两类适配器叶层**——表现层非视图部分
   18,818 行（40.6%）+ 基础设施 10,824 行（23.3%）。两类叶层在 core 内部**零消费者**，
   存在意义全部由 root 兑现。
2. **拆分标准自证是「编译所需」而非「领域归属」**：`crates/rustmark-core/Cargo.toml:11-13`
   注释原话「四层 + 表现层非视图子模块**编译所需的全部外部库**下沉到 core」——按编译便利
   切刀，不按内聚切刀。层内放置全部正确，错的是 crate 边界划在了「组合根」而非「领域核心」。
3. **「core 可独立复用」名不副实**：13 个直接外部依赖（egui / syntect / pulldown-cmark /
   image / ropey / rfd / serde / toml / thiserror / ureq / rustls-native-certs / log /
   ahash），`cargo tree -p rustmark-core` 输出 222 行。「可独立编译」成立，「可轻量复用」
   不成立——一个只用 `domain::document` 做 headless 处理的消费者会被整条 UI + 网络栈拖死
   （`publish = false` 暂时掩盖了这一点）。
4. **层内纪律确实好（这不是一个混乱的代码库）**：domain 外部依赖仅 serde（ropey/chrono
   实测零使用，比文档承诺更纯）；禁止方向 use 边全 0；`check-arch` 当日实跑 OK；ropey 被
   正确隔离为 `TextBuffer` 端口；infra 里 5 个 egui 文件全部有文档化理由。
5. 建议路径：**P0 文档与判据 → P1 叶层外移（`rustmark-actions` + `rustmark-icons` 首拆；
   views 留 root 的 `rustmark-ui` 主干）→ P1 state 瘦身审计 → P2 infra 去 egui 化 / find
   engine 上浮评估**。全流程**领域层零改动、root 公开路径不变、不引入新外部依赖**。

## 1. 现状量化画像

### 1.1 分层总览（实测）

| 层 | 文件 | 行数 | 占比 | 内联测试 | 外部依赖（引用文件数） | 12mo 提交数（代表模块） | core 内消费者 | root 引用 |
|---|---|---|---|---|---|---|---|---|
| presentation | 75 | 18,818 | 40.6% | 206 | egui 23 / image 1 / log 2 | state 30 · theme 19 · shortcuts 10 · rich 3 | **0（叶层）** | 161 |
| infrastructure | 51 | 10,824 | 23.3% | 90 | syntect 2 · pulldown_cmark 6 · image 4 · ropey 1 · ureq 1 · rfd 2 · toml 2 · egui 5 · log 7 | persistence 14 · parsing 12 | **0（叶层）** | 23 |
| domain | 46 | 7,180 | 15.5% | 15 | **仅 serde（8 文件）** | preferences 17 · markdown 14 · document 9 | 全部层 | 10 |
| application | 48 | 6,578 | 14.2% | 11 | serde 4 · ahash 1 / log 2 | document 11 · workspace 4 | presentation | 46 |
| shared | 18 | 2,975 | 6.4% | 35 | serde 5 · ahash 1 / log 3 / image 1 | 14 | 全部层 | 48 |
| **合计** | **238** | **46,375** | 100% | 357 | 13 个直接外部依赖 | — | — | 288 |

口径说明：root 引用 = `crate::<层>::` 拼回路径出现次数；root 直连 `rustmark_core::`
仅 2 处（`src/lib.rs` 与 `src/presentation.rs` 两个 re-export 点）；「外部依赖」按
`use <crate>` / `<crate>::` 模式计文件数，不含注释。

### 1.2 脊柱 vs 叶层

```
              ┌──────────────────── root（rustmark，20.8k 行）────────────────────┐
              │ app（组合根 7.0k）   presentation::views（13.8k；70 次提交/12mo）  │
              └──────────────┬───────────────────────────┬───────────────────────┘
                             │ 拼回路径 288 处            │ 组合根装配（port → impl）
                             ▼                           ▼
              ┌─────────────────── rustmark-core（46.4k）───────────────────┐
              │ 叶层  presentation 18.8k（40.6%）   infrastructure 10.8k（23.3%）│
              │        ↑ core 内零消费者              ↑ core 内零消费者          │
              │ ─────────────────────────────────────────────────────────────── │
              │ 脊柱  domain 7.2k ◄ application 6.6k ◄ shared 3.0k（16.7k，36.1%）│
              └──────────────────────────────────────────────────────────────────┘
```

叶层判定标准：在 core 内除自身子模块外零消费者、只被 root 消费或装配。presentation 与
infrastructure 均满足——**「core」这个名字覆盖的 64% 是适配器代码**。

### 1.3 presentation 内部拆解（18,818 行）

| 子模块 | 文件/行数 | 内容 | core 内消费者 | root 消费者 | 12mo 提交 |
|---|---|---|---|---|---|
| state | 24 / 6,682 | 13 个 `*_ui_state`（296–478 行/个）+ `app_ui_state` | shortcuts、theme | views（经桥接） | **30（全仓模块最热）** |
| rich | 25 / 5,918 | **math 公式排版引擎 2,076 + mermaid 图渲染 3,544（占 95%）**+ 其余 298 | **0** | views | 3 |
| theme | 10 / 3,482 | ThemeKind→egui 映射 / builtin / highlight_layout 773 行 | state、widgets | views | 19 |
| shortcuts | 8 / 1,532 | registry（动作词汇表 1,053）+ dispatcher | icons、state | views、app | 10 |
| widgets | 2 / 499 | 4 个 egui 组件（R-17 收口产物） | theme（row.rs） | 10 个 view 文件 | 2 |
| icons.rs | 1 / 589 | 16×16 向量图标规格数据 + Painter 绘制 | **0** | 3 个 view 文件 | ≈2 |

## 2. 臃肿判定：三个结构性问题与五项反向证据

### 2.1 问题一：「core」名不副实——64% 是适配器

`lib.rs:1-5` 自我描述为「rustmark 的核心库……装载四层与表现层的非视图子模块」。按名字，
读者预期 domain + application（±shared）≈ 10k 行；实际拿到 46.4k 行，其中表现层适配器
占 40.6%。认知后果具体可测：新人改 UI 状态要进 `rustmark-core`；grep `egui` 命中
presentation 23 文件 + infrastructure 5 文件，「core 依赖 egui」成为事实。

### 2.2 问题二：拆分标准是编译便利，不是领域归属

`Cargo.toml:11-13` 的注释即证据：切刀由「哪些文件编译时需要哪些库」决定。于是 syntect
（highlighting）、ureq + rustls（远程图片）、rfd（对话框）、egui（fonts /
viewport_capture / rich / widgets / theme / state）全部沉入 core。这不是放错层——每样
东西都在正确的层里——而是 **crate 边界划错了位置**：划在「组合根」而不是「领域核心」。

### 2.3 问题三：「可独立复用」只兑现了一半

- 可独立编译：成立（`cargo build -p rustmark-core` 通过，core 零本地依赖）。
- 可轻量复用：不成立。13 个直接外部依赖、222 行传递依赖树。headless / CLI / 嵌入消费者
  被迫链接 egui + syntect + ureq + rustls + rfd + image。
- 与构建收益相称的诚实：本会话实测冷构建 83s / 190 crate；core 改动增量 4.6s（波及 2/2
  crate）、root 改动 4.7s（波及 1/2）。**crate 边界在此体量下买到的编译隔离约等于零**。
  为 core 辩护的理由是语义与纯度，不是构建速度——把「构建快」当作拆分收益是站不住的。

### 2.4 反向证据：层内纪律确实好（五条）

1. **domain 极纯**：外部依赖仅 serde（8 文件）；`domain.rs:3` 文档允许 ropey/chrono，
   实测两者零使用——实际比承诺更纯。tests/domain 194 条 + 内联 15 条用例可门禁覆盖率。
2. **ropey 正确隔离**：domain 定义 `TextBuffer` 端口（`domain/document/services.rs:3,95,100`），
   实现在 `infrastructure/text/rope_text_buffer.rs`——端口/实现的教科书形态。
3. **方向纪律机械保持**：禁止方向 use 边全 0（自写脚本扫 326 文件）；`check-arch` 当日
   OK；548 字段白名单 + R-11/R-12 后 46 处裸字段写清零。
4. **infra→egui 5 文件是灰色地带而非违规**：fonts 三文件产 `egui::FontDefinitions`
   （`infrastructure/fonts.rs:5-10` 自证「适配成 egui 能消化的形状恰是基础设施本职」）；
   viewport_capture 用 `egui::ViewportCommand::Screenshot`（`viewport_capture.rs:3-8` 有
   放弃自研光栅化的论证）。均为「产出外部库类型」，有文档、有理由。
5. **shared 的非常驻民有论证**：`shared/pattern.rs:1-24` 记录了双层消费者（查找替换 UI +
   workspace 搜索）与手写正则的体积理由（10 MiB 红线）；`shared::theme`（ThemeKind 枚举）
   与 `presentation::theme`（egui 映射）是 R-09 收敛后的分工，不是重复。

## 3. 模块归属判定

### 3.1 判断依据（四维度 + 本项目实测口径）

| 维度 | 口径 | 判定规则 |
|---|---|---|
| 依赖方向 | core 内消费者数 + root 引用数 | 被下层依赖 = 核心；只被 root 消费 = 可外移 |
| 变更频率 | 12mo 提交数（新旧路径合并） | 高频 + 单向消费 = 外移优先（改它不该牵连领域） |
| 复用范围 | 消费者是否跨 crate / 外部复用计划 | 纯资源（数据 / 绘制 / 词汇）= 独立 crate 候选 |
| IO/框架耦合 | 外部 crate 引用文件数 | egui 只应出现在表现适配器；FS/网络/TLS 只应在 infrastructure；domain 必须为零 |

本项目额外实测出两条强信号：

- **零内部消费者（叶层信号）**：presentation、infrastructure 在 core 内都没有消费者。
- **state 零 serde**：`presentation/state` 24 文件无一处 serde——UI 状态不持久化
  （持久化走 `domain/preferences`），「state 必须在 core 才能被持久化」的理由不成立。

### 3.2 真正属于领域核心（保留，零改动）

- **domain 五上下文**（document 2,184 / markdown 1,897 / preferences 1,844 / workspace
  676 / export 519）：纯 serde、零 IO/UI、被所有层依赖。这是 core 的灵魂，任何重构
  不应触碰。
- **application 全部**（6,578）：薄编排 + dto + event_subscriptions（跨上下文反应的正确
  形态，零接线进 service）。
- **shared 内核 15 模块**（error/result/ids/geometry/event/dialog/encoding/export_format/
  fonts/path/system/image_asset/time/theme/text_format）：跨层契约，必须在 core。
- **shared 通用件 3 模块**（pattern 494 / lru / text）：非领域概念但跨层共用且有文档
  论证——保留，设增长监控（触发条件见 §7）。

### 3.3 应拆分为独立 crate（按优先级）

#### ① `rustmark-actions` + `rustmark-icons`（P1 首拆，风险最低）

证据链：

- icons.rs 589 行 / 4 个内联测试：纯 16×16 向量规格数据 + Painter 绘制，零领域逻辑、
  零 IO；**core 内零消费者**，仅 3 个 root 视图文件使用（`toolbar_view.rs`、
  `toolbar_view/controls.rs`、`sidebar_view/files.rs`）；12mo 约 2 次提交（大而稳）。
- UiAction 词汇表实测可无损剥离：`shortcuts/registry/` 全树 1,053 行，其中
  `action.rs`（155）+ `action/impls.rs`（446）+ `action/menu_group.rs`（50）= 651 行
  **零 use 语句或仅 `use super::*`**，`impls.rs` 对 combo/keys/palette/ActionEntry
  引用数为 0（`default_combo` 等返回 `&'static str`）——是完全自足的纯词汇层。
  combo/keys/palette（402 行）留在 core（palette 的 ActionEntry 被 command_palette
  消费，且它们经 registry re-export 即可继续访问 UiAction）。

缠绕点与两个变体（icons 以 `Icon::Action(UiAction)` 为键，`icons.rs:44,180-181`）：

- **变体 A（推荐，若 icons 独立是资源复用计划的第一步）**：先抽 `rustmark-actions`
  （651 行，零外部依赖）；core 的 `shortcuts::registry` 加 `pub use rustmark_actions::*;`，
  **25 个使用文件的 import 路径零改动**；再抽 `rustmark-icons`（依赖 egui + actions）。
  方向全部干净：`actions ← {core, root, icons}`，无环。
- **变体 B（仅当目标是快速验证「叶层可无损外移」）**：icons 直接依赖 core 取 UiAction。
  改动最小且无环，但资源 crate 反向拖 222 行传递依赖树，语义难看——只作练手。

迁移前预检（必做）：确认 `action/impls.rs` 无隐藏的跨模块引用（本次实测为 0）；若预检
发现引用 combo/keys，则将这两个模块一并迁入 actions（它们同属词汇表且零外部依赖）。

#### ② `rustmark-ui`（P1 主干，触发式）

- 证据：presentation 六子模块 18.8k 行全部是 UI 适配器；core 内零消费者；root 引用
  161 处全经桥接（`src/presentation.rs` 6 行）。
- **关键设计（与上轮「方案二」的本质区别）：只搬 6 个非视图子模块，views 留在 root。**
  views（root）与 state（ui crate）仍在不同 crate，**R-11/R-12「views 不得直写 state
  私有字段」的编译保证不降级**。上轮否决「presentation 搬回 root」正是因为那会让 views
  与 state 同 crate、编译保证降级为测试保证；搬进中间 crate 没有这个问题。
- 依赖：`root → {ui, core}`，`ui → core`；无环。ui crate 内保留 `presentation/` 内部
  分层供 check-arch 继续判定。
- 搬移后 core 剩 27.6k 行（shared 3.0 + domain 7.2 + application 6.6 + infrastructure
  10.8）。
- 诚实声明收益：编译隔离收益仍≈0（ui 改动重编 ui + root 两 crate，与现在 core + root
  两 crate 相同）；真实收益是**语义名实相符、core 去 UI 化后可复用、ui 内部后续再拆分
  的空间**。

#### ③ `rustmark-render`（P2，可选）

rich 的 math（2,076 行公式排版）+ mermaid（3,544 行图渲染）是两个自包含渲染引擎，
core 内零消费者、12mo 仅 3 次提交（大而稳）。若 mermaid/math 还要继续加重（见 §8-4），
单独成 crate 利于独立测试与体积观察；否则随 ui crate 一起走，不必单独拆。

#### ④ `rustmark-infra`（P2，触发式）

infrastructure 层归属正确，问题只在 crate 粒度。**前置条件是去 egui 化**：fonts 三文件
产 `egui::FontDefinitions`、viewport_capture 用 egui 截图 API——必须先端口化（shared
定义 `FontSource` 端口只产字节，egui 装配移到 ui 侧；截图请求经组合根端口），否则
infra crate 仍拖 egui，「core 可 headless 复用」依旧不成立。触发条件见 §7。

### 3.4 应上浮到应用服务层（审计候选，非既定错误）

- **`presentation/state/find_replace_ui_state/engine.rs`（351 行）**：模块文档自述
  「查找引擎：编译后扫描、循环定位、全部替换预览」——**匹配扫描算法住在 UI state 层**。
  它构建于 shared::pattern，逻辑上是用例级能力（FindInDocument / ReplaceAll）。当前无
  headless 需求所以无痛；一旦要做 CLI 批量替换必须上浮 application。→ P2：把「扫描 →
  Match 列表」提为 application 用例，UI state 只留选区与循环定位。
- **`presentation/state/symbol_ui_state/catalog.rs`（358 行）**：静态符号表（表情/标点/
  片段），与 icons 同类的资源数据。→ P2：可并入 icons crate 的数据模块（若 icons 已拆）。

### 3.5 应下沉到基础设施层

**无既定错配。** infra→egui 5 文件是「产出外部库类型」且已文档化（§2.4-4）；若走
stage 3 去 egui 化，按 §3.3④ 端口化处理。shared 的 pattern/lru/text 有跨层消费者论证，
留在 shared 是正确的——把它们下沉 infrastructure 反而会让 application 违反方向规则。

### 3.6 灰色地带（已文档化，不建议动）

- `domain/markdown/value_objects/render.rs`（RenderedBlock.spans，119 行）：渲染型值对象
  在 domain，被 domain 自身 + application + infrastructure 三方使用——移动成本 > 收益，
  保持文档化的灰色地带。
- `shared::theme` vs `presentation::theme`：R-09 收敛后的分工（枚举 vs egui 映射），非重复。

## 4. 目标目录结构（三阶段演进）

```
阶段 0（现状）                  阶段 1（P1 首拆后）                阶段 2（触发后）
rustmark                       rustmark                           rustmark（组合根 + views + bin）
├─ app/            7.0k        ├─ app/              7.0k          ├─ app/                    7.0k
├─ presentation/  13.8k        ├─ presentation/    13.8k         ├─ presentation/views      13.8k
│  └─ views/                    │  └─ views/                      ├─ rustmark-actions       0.7k
└─ rustmark-core  46.4k        ├─ rustmark-icons    0.6k ──┐      ├─ rustmark-ui           18.1k
   ├─ shared        3.0k       ├─ rustmark-actions  0.7k ──┤      │  └─ state/theme/rich/shortcuts/widgets
   ├─ domain        7.2k       └─ rustmark-core    45.8k   │      └─ rustmark-core           27.6k
   ├─ application   6.6k         ├─ shared     3.0k       │         ├─ shared / domain / application（脊柱）
   ├─ infrastructure 10.8k       ├─ domain     7.2k       │         └─ infrastructure 10.8k
   └─ presentation  18.8k        ├─ application 6.6k       │            （fonts/viewport_capture 端口化后
      ├─ state   6.7k            ├─ infrastructure 10.8k   │              可再拆 rustmark-infra）
      ├─ rich    5.9k            └─ presentation 17.9k     │
      ├─ theme   3.5k              ├─ state / theme / rich │
      ├─ shortcuts 1.5k            │  / shortcuts / widgets│
      ├─ widgets 0.5k              └─（icons 已拆出）      │
      └─ icons   0.6k                                     │
                                  方向：icons→actions；ui→core；root→{ui,core,icons,actions}
```

阶段 3（真实非 UI 消费者出现后）：core 完成去 egui 化 → 16.7k 纯六边形（shared+domain+
application）；infrastructure → `rustmark-infra`（10.8k，syntect/ureq/rfd/pulldown/image）。

## 5. 渐进重构步骤

通则：每步一个独立 commit；四道门禁（`cargo fmt --check` / `cargo run --bin check-arch` /
`cargo clippy --all-targets --all-features -- -D warnings` / `cargo test --all-targets`，
测试数逐行求和对基线 2131 passed / 0 failed / 4 ignored）；守卫类改动配变异验证；用户
可见功能真帧回归。**domain 7,180 行在全流程零改动——这是「不破坏对外行为」的锚**，
任何步骤 diff 触及 `domain/` 即停。

| # | 步骤 | 范围 | 验证 | 回退 | 量级 |
|---|---|---|---|---|---|
| 0 | 文档先行：`architecture-constraints.md` 加「crate 地图 + 模块归属四问判据」；ADR-013 记录本次决策 | 纯文档 | 五查（目录锚点 ↔ 标题、路径存在性） | 删文件 | 0.5 天 |
| 1 | 抽 `rustmark-actions` | registry 的 action 三文件 651 行；core `shortcuts::registry` 加 `pub use rustmark_actions::*;`；**25 个使用文件零改动** | 四门禁 + shortcuts/command_palette 全用例 | revert 单 commit | 0.5–1 天 |
| 2 | 抽 `rustmark-icons` | icons.rs 589 行 + 4 测试；桥接改 `pub use rustmark_icons as icons;`；root +1 dep | 四门禁 + **工具栏/侧栏图标真帧渲染回归**（icons 有图元数量/外包矩形级断言） | revert | 0.5 天 |
| 3 | state 瘦身审计 | 24 文件逐字段四问（跨帧？可测？持久化？被 application 消费？）；先出清单，纯帧内状态挪 view-local（egui memory / 局部变量） | 每批四门禁 + 状态相关用例 | 分批 revert | 1–2 天 |
| 4 | find engine 上浮评估 | engine.rs 351 行扫描逻辑 → application 用例（若决定做） | 查找替换全用例 + 变异（若动判据） | revert | 1 天 |
| 5 | （触发后）presentation → `rustmark-ui` | 6 子模块 + tests/presentation 中归属部分；check-arch 层表加 ui crate；field guard 测试 crate 对改 (root, ui) | 四门禁 + 全量 2131 用例 + 真帧 | revert | 2–3 天 |
| 6 | （触发后）infra 去 egui 化 → `rustmark-infra` | fonts/viewport_capture 端口化先行，再搬 10.8k | 四门禁 + 字体/导出/截图用例 | revert | 3–5 天 |

## 6. 风险控制要点

1. **对外路径不变**：root 的 `crate::presentation::<子模块>` 公开路径经桥接 re-export
   全部保持（icons：`pub use rustmark_icons as icons;`；ui：`pub use rustmark_ui::{...};`；
   actions：core 内 `pub use rustmark_actions::*;` 中转）。**外部 API 零变化**。
2. **领域层零改动**是行为不变的锚（见 §5 通则）。
3. **体积红线**：release exe 现 9.998 MiB，距 10 MiB 红线仅 512 B。步骤 1/2/5/6 均不
   引入新外部 crate（egui 已在依赖中），但每步仍须过 `scripts/size_gate.sh`。
4. **编译保证不降级**：步骤 1/2/5 都保持「views ∈ root、state ∈ 另一 crate」的跨 crate
   字段保护；唯独**不要把 views 搬进 ui crate**——那才会把 R-11/R-12 的编译保证降级。
5. **测试随迁**：tests/presentation 622 条用例按「被测模块归属」拆分——测 core
   presentation 的随 ui crate 走，测 views 的留 root；保持全量 2131 基线（逐行求和口径）。
6. **不引 feature / optional**：避免 crate 组合爆炸；`publish = false` 保持不变。
7. **守卫同步**：check-arch 的本地依赖边快照（现为 root→core 一条边）随每步更新；
   CI 增加 `cargo tree --workspace --depth 1` 拓扑断言，把「新增本地依赖边」变成显式
   评审事件。

## 7. 守卫与触发条件（量化）

| 动作 | 触发条件（任一满足即启动评估） | 度量方式 |
|---|---|---|
| presentation → ui crate | ① UI 并行开发人数 ≥2；② core 增量编译 >10s（现 4.6s）；③ presentation 年变更 ≥80 提交且「views↔core state 跨 crate 摩擦」在 ≥3 个功能中复现 | git log --stat 月度脚本 |
| infrastructure → infra crate | 出现真实非 UI 消费者（CLI/headless/嵌入）**且**去 egui 化完成 | 需求事实 + `cargo tree -p rustmark-core \| grep egui` 为 0 |
| math/mermaid → render crate | mermaid/math 年变更 ≥10 提交或合计 >8k 行（现 5.6k） | git log + wc -l |
| 新 crate 合并回退 | 新建 crate <1k 行且 12mo 变更 <5 提交（防过度拆分） | git log |
| shared 通用件再拆分 | pattern+lru+text 合计 >1,500 行或出现第 4 个非领域通用模块 | wc -l |
| state 瘦身叫停 | 审计发现纯帧内字段 <10%（收益不抵改动风险） | 字段清单 |

## 8. 待补充信息

1. **core 的复用意图**：是否计划 CLI / headless / 嵌入消费者？这直接决定 §3.3④（infra
   去 egui 化 + 独立）从 P2 升 P1，也决定「core 可复用」是否值得付出端口化成本。
2. **icons / rich 是否有跨项目复用计划**：决定 §3.3① 的变体 A/B 与 render crate 取舍。
3. **团队规模与并行计划**：ui crate 触发条件①。
4. **mermaid / math 路线图**：还要继续加渲染器吗（render crate 优先级）。
5. **具体摩擦案例**：有没有哪个功能让你明确感到「被 core 拖累」（编译等待、合并冲突、
   测试难写）？一个真实案例比全部统计更能校准优先级。

---

*附：本评审与 `docs/workspace-split-assessment-2026-10-24.md`（workspace 级拆分评估）、
`docs/architecture-review-2026-10-23.md`（层内问题清单）互为补充；三者结论不冲突——
前者管 crate 方向，中者管层内纪律，本文管 core 内部粒度。*
