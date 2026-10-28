# ADR-013 · core 适配层驻留与叶层外移策略

- **状态**：已接受（2026-10-24；判据与触发条件已进 `architecture-constraints.md` §10，actions / icons 两步已落地）
- **决策者**：项目架构负责人
- **关联任务**：V11-56（规则化）/ V11-57 / V11-58（已落地）/ V11-59~61（审计与关闭项）/ V11-62~64（待触发）
- **关联文档**：`docs/architecture-constraints.md` §10、`docs/archive/architecture-reviews/core-structure-review-2026-10-24.md`、`docs/archive/architecture-reviews/state-field-audit.md`、`docs/workspace-topology.expected.txt`、`docs/metrics.md` §7、`tools/onboard_cost.py`

## 背景

评审（`docs/archive/architecture-reviews/core-structure-review-2026-10-24.md`）对 core 的臃肿判定成立，但性质是
**「crate 粒度错配」，不是「层内混乱」**：

1. **core 名不副实**：46,375 行 / 238 文件里，领域脊柱（domain + application +
   shared）仅 16,733 行 / 36.1%；**63.9% 是两类适配器叶层**——presentation
   18,818 行（40.6%）+ infrastructure 10,824 行（23.3%），二者在 core 内
   **零消费者**，存在意义全部由 root 兑现。
2. **拆分标准自证是编译便利**：`crates/rustmark-core/Cargo.toml:11-13` 注释
   原话「四层 + 表现层非视图子模块**编译所需的全部外部库**下沉到 core」——
   按编译便利切刀，不按内聚切刀。层内放置全部正确，错的是 crate 边界划在了
   「组合根」而非「领域核心」。
3. **「core 可独立复用」只兑现一半**：13 个直接外部依赖，headless / CLI / 嵌入
   消费者被迫链接 egui + syntect + ureq + rustls + rfd + image。而为 core 辩护的
   理由应是语义与纯度，**不是构建速度**——本仓实测冷构建 83s / 190 crate，core
   改动增量 4.6s、root 4.7s，crate 边界在此体量下买到的编译隔离约等于零。

同时有五项反向证据证明层内纪律确实好（domain 极纯、ropey 端口隔离、禁止方向
use 边全 0、infra→egui 5 个文件有文档化理由、shared 非常驻民有论证）——所以
**不动层，只动 crate 边界**。

判据缺位时，每次拆分都要重新辩论一遍「什么该住 core」；且 V11-62/63/64 都是
触发式任务，没有量化门槛就只能拍脑袋。本 ADR 把评审结论固化为项目规范。

## 决策

### 1. 模块归属四问 + 两条强信号成为项目规范（constraints §10.2）

依赖方向 / 变更频率 / 复用范围 / IO·框架耦合四问，加两条本项目实测强信号
（零内部消费者 = 叶层信号；state 零 serde = 无持久化理由）。任何「搬进 / 搬出
core」的提案先过四问，结论可复现——`icons`（V11-58）的抽查示例已写进
constraints §10.2，验收时任取一模块按表能复现评审结论。

### 2. 叶层外移的守卫形态：自由层 + Cargo 环 + 拓扑断言（三件套）

新 crate 不是法外之地：`arch_check` 扫描根与 `layer_of` 归一化随拆随扩
（actions / icons 两例）；零依赖契约 crate 判 Free 层，反向依赖由 **Cargo
循环依赖**结构性挡死，零依赖面由 `tests/workspace_topology_test.rs` 与
`docs/workspace-topology.expected.txt`（CI diff）兜底。**每次新增本地依赖边
= 显式评审事件。**

本节追认 V11-57 经用户裁定的「方案 b」，并推广到后续所有新 crate。

### 3. presentation 外移形态：**views 留 root 的中间 crate**

核心约束：`views ∈ root`、`state ∈ 另一 crate` 的跨 crate 字段保护
（R-11 / R-12）不得降级。备选与否决理由见文末表格。

### 4. 已落地两步：actions（V11-57）与 icons（V11-58）先行

零依赖契约（UiAction 词汇表）与纯资源（16×16 图标规格 + Painter 渲染器）是
风险最低的首拆对象，取评审变体 A（先抽 actions，icons 依赖 actions）——
变体 B（icons 直接依赖整个 core 取 `UiAction`）因「资源 crate 反向拖 222 行
传递依赖树、语义难看」否决。落地结果：25 个使用文件零 diff、root 公开路径
`crate::presentation::*` 经桥接零变化、测试总量守恒（icons 4 个规格单测
从 core lib 迁出为独立目标）、release 体积零差。这两步同时是三件套守卫的实证。

### 5. 「不为拆而拆」：过度拆分的回退判据，及其豁免

新建 crate <1k 行且 12mo 变更 <5 提交 → 评估合并回退。**豁免**：零依赖契约 /
纯资源 crate（actions 687 行、icons 613 行均 <1k 行，但回退它们等于把契约
塞回 core）。本项与「state 瘦身叫停」判据（V11-59 审计已触发并闭环，
V11-60 关闭）共同构成「拆分也有刹车」的两侧。

### 6. 后续 crate 一律沿用同一形态（含 2026-10-25 ~ 10-26 落地验证）

本 ADR 立下的三件套（自由层 / Cargo 环 / 拓扑断言）与「保留路径前缀」手法
在后续五个 crate 上被反复复用，**未新增任何一条校验规则**：

| crate | 决策依据 | 前缀策略 | 新增的守卫成本 |
|---|---|---|---|
| `rustmark-ui`（V11-62，五批次） | 决策 3（views 留 root） | 保 `presentation/` | 零——层禁令与视图误搬防护靠路径判层自动生效 |
| `rustmark-shared`（TD-04） | §10.3「shared 通用件再拆分」触发后维护者拍板**整份抽**（而非只抽 6 个通用件：前缀保留后两者代价相同，整抽避免「一半在内一半在外」的判层歧义） | 保 `shared/` | 仅一条依赖面断言（零 workspace 成员） |
| `rustmark-infra`（V11-64） | §10.3「infrastructure → infra」触发（Q-01 答「有计划」） | 保 `infrastructure/` | 仅一条依赖面断言（workspace 成员恰为 core） |

两条可复用的经验写回本 ADR：

1. **前缀保留是「零规则增量」的关键**。五个 crate 的搬迁都只改 crate 边界，
   `layer_of` 按根内相对路径判层，于是层禁令表、可见性白名单键、各专项守卫
   的扫描键**全部不用动**——否则每次搬迁都要同步改 5 处规则，规则漂移风险
   远高于收益。
2. **拆 crate 不是免费的**：两次纯搬迁各 +2,048 B / +3,072 B release 体积
   （跨 crate LTO 边界成本，见 metrics §2.8）。相对收益是**依赖面收窄**
   （core 外部依赖 12 → 4）与**编译面隔离**，不是产物变小。

## 后果

**收益**：

- 「什么该住 core」从逐次辩论变成查表（constraints §10.2 四问）；
- V11-62/63/64 有了量化启动门槛（§10.3），触发即有数据；
- 守卫三件套随新 crate 同步，「未知层」误报与依赖边静默增长都被机械挡住；
- actions / icons 落地证明外移可以做到公开路径零变化、测试总量守恒、
  release 体积零差。

**代价**：

- 每拆一个 crate 要同步改三处守卫（扫描根、`layer_of` 归一化、拓扑期望）
  与 CI 期望文件；
- 中间 crate 形态（ui）比「一步到位」多一次搬迁——state / theme / rich 等
  要先经 ui crate 再到最终形态；
- 触发条件里的「跨 crate 摩擦案例 ≥3」依赖人工记录，**目前书面记录为 0**，
  是 V11-62 的主要待补证据。

**已知不覆盖**：

- 领域层归属判定（哪个聚合放哪个上下文）不在本 ADR——层内放置评审已认定
  全部正确；
- 编译收益不做承诺（本仓体量下 ≈0，见背景第 3 条）；拆分的收益主张仅限
  语义、纯度与复用面；
- find engine 上浮 application（V11-61）维持降级记录，出现 CLI 批量替换
  需求时重启。

## 备选方案

| 方案 | 否决理由 |
|---|---|
| presentation 整体搬进 ui crate（含 views） | R-11/R-12 的跨 crate 字段保护降级为同 crate 内约束——「表现层不得直写 app 状态字段」将失去编译期拦截，这是本次改造最核心的保证，不能用 crate 数量换 |
| presentation 非视图部分搬回 root（core 只留四层脊柱） | views 已在 root，state 搬回即与 views 同 crate——R-11/R-12 的跨 crate 字段保护降级为测试保证（上轮已据此否决，评审 §3.3②）；附带 root 从 21.0k 行涨到 ≈39k 行，推翻 D-3 4a「shell 只留组合根、视图与拼接点」的既定约定 |
| 按上下文再拆（state / theme / rich / shortcuts / widgets 各自去各自该去的地方） | 会产生 3–4 个新 crate 与更多本地依赖边，而编译隔离收益在此体量下 ≈0（core 增量 5.92s 实测）；触发条件未满足前不预支复杂度 |
| 维持现状不拆 | 「core 名不副实」的认知成本持续付出：新人改 UI 状态要进 `rustmark-core`；headless 消费者被整条 UI + 网络栈拖死；且每次提案重复辩论 |
| 先做 infra 去 egui 化再拆 ui | 顺序倒置——infra 拆出的前提（真实非 UI 消费者）尚未出现，先去 egui 化是纯负担；评审报告 P1/P2 排序即为此 |
