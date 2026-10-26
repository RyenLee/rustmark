// 架构边界校验器 —— 由 `build.rs` 与 `src/bin/check-arch.rs` 通过 `include!` 共用。
//
// # 它是怎么判的（词法级，不解析 AST，也不引入额外依赖）
//
// 1. **词法**：源码 → token 流。行注释、块注释（含嵌套）、字符串字面量、字符字面量
//    全部剔除——否则「注释里提到 `crate::domain`」「错误信息里写了 `crate::domain`」
//    会被误判成跨层依赖。
// 2. **`use` 展开**：`use` 声明按 use-tree 语法展开成**完整路径**，覆盖
//    `pub use` / `use a::{b, c}` / `use a::b as c` / `use a::*` / 跨行 / 前导 `::`。
//    `extern crate a;`（2015 版式，`#[macro_use]` 场景）同等对待。
// 3. **限定路径**：非 `use` 位置的 `a::b::C` 同样参与判定——
//    `fn f(d: crate::domain::x::Y)` 也是跨层依赖。
// 4. **匹配**：与各层禁止清单做「路径段前缀」匹配（`crate::domain` 命中
//    `crate::domain::x::Y`，但不命中 `crate::domainfoo`）。
// 5. **层判定**：先剥掉末段 `.rs` 再取首段。自由位置是**白名单**
//    （`main.rs` / `lib.rs` / `bin/**`）；其它顶层名字判「未知层」并报错——
//    默认放行等于新目录静默逃过全部层禁令。
// 6. **模块布局**：扫描范围内出现 `mod.rs` 一律报错（`CONSTR-03`：一级模块
//    用 `xxx.rs`，子模块放 `xxx/`）。这条不看源码内容、只看文件名，故在
//    `scan_impl` 里逐个文件判，不进 `check_file`——它没有词法可言。
//
// # 豁免（`arch-allow`）
//
// 行尾注释 `// arch-allow: <对象>` **只豁免该对象**（不再整行放行），
// 且必须登记在 `docs/architecture-constraints.md` 的豁免表里（校验器逐条核对）。
//
// 豁免窗口分两种形状：**行尾**豁免只作用于本条语句；**独立成行**的豁免额外
// 作用于紧邻的下一行。少了这个区分，行尾豁免会漏到下一行，把一条独立的
// 违规一并压掉。
//
// # 帧预约唯一出口（T11 / ADR-009）
//
// `request_repaint_after` 在 `src/` 里**只允许**出现在 `app/frame/scheduler.rs`。
// 这条规则对**所有**文件生效（含组合根 `app/`）——调用点自己预约帧，
// 正是历史上「定时功能完全不生效」那三次的根因。它**不设豁免**。

use std::fs;
use std::path::Path;

/// 一处架构违规。
#[derive(Debug)]
pub struct Violation {
    /// 相对路径（便于 CI 日志直接阅读）。
    pub file: String,
    /// 行号（1-based）。
    pub line: usize,
    /// 违规说明。
    pub detail: String,
}

const DOMAIN_FORBIDDEN: &[&str] = &[
    "egui",
    "eframe",
    "rfd",
    "syntect",
    "pulldown_cmark",
    "std::fs",
    "std::io",
    "anyhow",
    "crate::app",
    "crate::application",
    "crate::infrastructure",
    "crate::presentation",
];

const APPLICATION_FORBIDDEN: &[&str] = &[
    "egui",
    "eframe",
    "rfd",
    "syntect",
    "pulldown_cmark",
    "std::fs",
    "std::io",
    "crate::app",
    "crate::infrastructure",
    "crate::presentation",
];

/// `infrastructure` 禁 `egui`（V11-63 棘轮守卫，2026-10-25）。
///
/// # 为什么 infrastructure 不该出现 egui
///
/// §10.2 四问之④：「egui 只应出现在表现适配器」。infrastructure 是右侧
/// 适配器（FS / 网络 / 解码 / 高亮），产物应是字节与端口类型；一旦它直接
/// 产 `egui::FontDefinitions` / `ColorImage`，「core 可 headless 复用」
/// 就不成立——消费者为了拿字体装配结果被迫拖进整个 egui。
///
/// # 从棘轮到无条件（V11-63 全量端口化的终点）
///
/// 存量泄漏最初落在 5 个文件（fonts 系 3 + viewport_capture 系 2），一刀切禁
/// 会让它们同时报错、规则无法遵守。2026-10-25 先落地为**棘轮**：文件级放行
/// 清单 `V11_63_KNOWN_EGUI_LEAKS` 只放行那 5 个文件，未登记文件（含同目录
/// 新文件）的新增引用照常拦截——渗漏只能缩小不能扩大。
///
/// 随后两阶段端口化把渗漏清零：阶段 A 用 `PortFrame`（RGBA8 字节）替掉
/// `ColorImage`（viewport_capture 系 2 文件），阶段 B 用 `PortFontDefinitions`
/// 一族端口类型替掉 `egui::FontDefinitions`（fonts 系 3 文件），egui 类型
/// 只在组合根 `src/app/frame.rs` 的适配器里出现。
///
/// **清单已删、分支已删**：本条现在是**无条件生效**的硬规则，不再有任何
/// 文件级例外。留着空清单只会给下一个人「这里还能登记豁免」的错觉；
/// 真要再加例外，必须同时改本文件、约束文档 §4.4 与验收用例——成本高于
/// 老老实实做端口化，这正是想要的激励方向。
const INFRASTRUCTURE_FORBIDDEN: &[&str] = &[
    "crate::app",
    "crate::application",
    "crate::presentation",
    "egui",
];

const PRESENTATION_FORBIDDEN: &[&str] = &["crate::app", "crate::domain", "crate::infrastructure"];

/// `presentation/state/` 的额外禁令（S6-12）。
///
/// # 为什么要单独一条
///
/// [`PRESENTATION_FORBIDDEN`] 已经禁掉了整个 `crate::domain`，
/// 因此本条**在当前代码里不会新增任何拦截**。它的价值是**防未来松动**：
/// 若哪天有人为了图方便把 `PRESENTATION_FORBIDDEN` 放宽
/// （例如「状态层要读个文档摘要应该没事」），`state/` 这条独立规则
/// 仍在原地，把破坏挡在视图层之外。
///
/// 代价是**一条当前不生效的规则**。这个代价是值得付的：
/// 它的失效场景是「有人同时改了规则**和**代码」，
/// 而那正是最需要守卫的时刻。
const PRESENTATION_STATE_FORBIDDEN: &[&str] = &["crate::domain::document"];

/// `presentation/state/` 不得回指 `views/`（R-13）。
///
/// # 为什么这是表现层内部最重要的一条方向
///
/// `views/` 读 `state/` 是**单向**的：视图采集状态、按状态绘制、
/// 把用户操作写回状态。若 `state/` 反过来依赖 `views/`，数据流立刻成环——
/// 「改一个开关要先构造视图」这类代码会出现，而立即模式框架下视图每帧
/// 重建，环会以「状态被视图的渲染副作用污染」的形式发作，极难定位。
///
/// 实测存量（2026-10-20）：`state/` 对 `views` 的引用 **0 处**（命中 2 处
/// 全在文档注释里），故这条规则当前**不改变任何编译结果**——它的价值
/// 与 [`PRESENTATION_STATE_FORBIDDEN`] 相同：防未来松动。
const PRESENTATION_STATE_VIEWS_FORBIDDEN: &[&str] = &["crate::presentation::views"];

/// `presentation/rich/` 不得依赖的四个兄弟子层（R-13）。
///
/// ADR-007 已用编译期手段保证 `rich/` 对 egui 零依赖（自绘排版），
/// 本项把「不得依赖 `views/` / `state/` / `theme/` / `widgets/`」机械化。
///
/// # 为什么 `rich/` 要比别的叶子更严
///
/// 它是**渲染算法**：输入一段结构化内容 + 度量，输出字形位置。
/// 一旦它能读 `state/`，「排版结果随 UI 状态变化」就会进来——同一段
/// 文档在侧栏开着与关着时排版不同，而这种差异不报错、也看不出原因。
/// 依赖 `theme/` 同理：配色该由调用方给参数，不是自己查。
const PRESENTATION_RICH_FORBIDDEN: &[&str] = &[
    "crate::presentation::views",
    "crate::presentation::state",
    "crate::presentation::theme",
    "crate::presentation::widgets",
];

/// `presentation/widgets/` 不得依赖 `views/`（R-13）。
///
/// 控件是**被视图复用**的叶子。它读视图就会形成「叶子→根」的反向边，
/// 而复用关系一旦成环，控件再也抽不出来单独测试。
const PRESENTATION_WIDGETS_FORBIDDEN: &[&str] = &["crate::presentation::views"];

/// `presentation/icons.rs` 不得依赖 `views/`（R-13）。
///
/// 字形图标是**最底层的叶子**（渲染断言直接比对字形矩形），
/// 它连 `widgets/` 都不该碰，更别提 `views/`。
const PRESENTATION_ICONS_FORBIDDEN: &[&str] = &["crate::presentation::views"];

/// `shared` 禁止的外部依赖与层引用。
///
/// # `std::process` 为什么在这里（R-03）
///
/// `shared` 自称「零外部依赖」，但 `system.rs` 曾在此用 `Command` 起进程，
/// 文件头自认「`std::process` 不在禁止清单，故在此落位」——那是**绕规则
/// 落位**：规则说这里不该有 IO，就改规则迁就代码。R-03 已把执行端移到
/// `infrastructure::system`，此处补上禁令，防止回退。
///
/// 端口（trait）与纯逻辑校验可以留在 `shared`——它们不产生 IO。
const SHARED_FORBIDDEN: &[&str] = &[
    "egui",
    "eframe",
    "rfd",
    "syntect",
    "pulldown_cmark",
    "std::process",
    "crate::app",
    "crate::domain",
    "crate::application",
    "crate::infrastructure",
    "crate::presentation",
];

/// 集成测试（`tests/**`）禁止的**库内部视角**前缀。
///
/// # 为什么测试不套用任何层的禁令
///
/// 测试是**消费者**，不是层：`tests/app/` 下的用例天然要同时看见四层
/// （跨层断言只能放那儿），按 `tests/` 的目录名去套 `src/` 的层禁令
/// 既不成立也会误判。
///
/// # 那这条规则拦什么
///
/// 拦「作者以为自己在 `src/` 里写代码」：`crate::` 在集成测试里指向的是
/// **测试 crate 自身**，写 `use crate::domain::x::Y;` 说明心智模型错了，
/// 正确写法是 `rustmark::domain::x::Y`（公开 API）。这类写法编译就过不了，
/// 所以它不是「编译能过的偷看」，而是**从 `src/` 复制粘贴过来时**原样带
/// 进来的坏味道——留着它，将来某天 `tests/support/` 里长出个同名模块
/// 就会静默变成真的内部引用。
///
/// # 为什么不禁 `crate::support` 之类
///
/// `crate::support` / `crate::golden_fixture` 指向**测试自己的**共享模块
/// （`tests/support/`、`tests/integration/golden_fixture.rs`），是集成测试
/// 的正常组织方式。禁令只列**库层名**，天然放行它们。
///
/// 实测存量（2026-10-20）：`tests/` 下 `crate::<库层名>` 的真实引用为 **0**
/// （`arch_violation_fixture.rs` 里出现的那些全是**字符串字面量**中的测试
/// 数据，词法阶段即被剔除）。
const TESTS_FORBIDDEN: &[&str] = &[
    "crate::app",
    "crate::domain",
    "crate::application",
    "crate::infrastructure",
    "crate::shared",
    "crate::presentation",
];

/// 定时任务「到期帧」预约的唯一出口（T11 / ADR-009）。
///
/// # 为什么这是一条架构规则，而不是编码风格
///
/// egui 是**事件驱动重绘**：没有输入、没有动画时不产生新帧。任何「等 X 毫秒
/// 后再做一件事」的逻辑，都必须有人在那个时刻预约一帧，否则功能静默失效
/// （问题 068：自动保存完全不生效）。历史上这个预约是**逐处手写**的，
/// 于是统计防抖、高亮补算、消息 TTL 三处都漏了，只靠编辑区光标闪烁兜底。
///
/// 现在只在 `FrameScheduler::flush` 里预约，调用点只 `arm` 到期点。
/// 这里用文本扫描守住它：若哪天有人在别处直接写 `request_repaint_after`，
/// 「漏预约」的坑就会以完全相同的形式回来，而它不产生编译错误、单测也未必覆盖。
const REPAINT_RESERVATION: &str = "request_repaint_after";
const REPAINT_RESERVATION_HOME: &str = "app/frame/scheduler.rs";

/// 模块布局禁令（`CONSTR-03`）：禁止 `mod.rs`。
///
/// # 为什么必须机械化
///
/// `REQUIREMENTS.md` 的 `CONSTR-03` 与 `CONTRIBUTING.md` 都写明「一级模块用
/// `xxx.rs`，子模块放 `xxx/`」，但这条约定**没有任何编译期后果**：写成
/// `foo/mod.rs` 时 `cargo check` / `clippy` / 全部单测照过，只有人眼评审能发现。
/// 2026-10-22 复核时 `src/infrastructure/fonts/` 与 `tests/support/` 两处
/// `mod.rs` 就是这么活下来的——规则在文档里，却不在门禁里。
///
/// 与帧预约那条相同：**不设豁免**。文件布局没有「个别情况确实需要」的例外，
/// 一旦开了口子，下一个 `mod.rs` 就会以「这个模块比较特殊」的理由出现。
const FORBIDDEN_MOD_FILE: &str = "mod.rs";

/// 领域层禁止的「直接打印」宏家族。
///
/// `print!` / `println!` 写 stdout，`eprint!` / `eprintln!` / `dbg!` 写 stderr。
/// 旧实现只查 `println!` 字面量——`print!` 漏网、`eprintln!` 被当成 `println!`
/// 报错（文案不准），且行尾注释里提到 `println!` 会被误报。
const PRINT_MACROS: &[&str] = &["print", "println", "eprint", "eprintln", "dbg"];

/// 豁免注释的标记串。
const ARCH_ALLOW: &str = "arch-allow:";

/// 豁免登记表在 `docs/architecture-constraints.md` 里的区段标记。
const REGISTRY_BEGIN: &str = "<!-- arch-allow:registry -->";
const REGISTRY_END: &str = "<!-- /arch-allow:registry -->";

/// `shared` 准入登记表在 `docs/architecture-constraints.md` 里的区段标记
/// （§4.5 三问，D-6）。与豁免表同级：`shared/` 下每个文件必须登记。
const SHARED_REGISTRY_BEGIN: &str = "<!-- shared:registry -->";
const SHARED_REGISTRY_END: &str = "<!-- /shared:registry -->";

// ============================================================================
// 层判定
// ============================================================================

/// 文件所属的层。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    Domain,
    Application,
    Infrastructure,
    Presentation,
    /// `presentation/state/` 子层（父层禁令**叠加**其上）。
    PresentationState,
    Shared,
    /// 组合根：允许依赖全部层（`app.rs` / `app/**`）。
    App,
    /// 组合根之外的自由文件（`main.rs` / `lib.rs` / `bin/**`）：不受层级限制。
    Free,
    /// `presentation/views/` 子层（R-13 细粒度化）。
    ///
    /// 视图层可读 `state/`（那是它存在的理由），故**不**禁 `state`。
    PresentationViews,
    /// `presentation/rich/` 子层（R-13）：自绘排版（数学 / Mermaid）。
    ///
    /// ADR-007 已用编译期手段保证它对 egui 零依赖，本项把「不得依赖
    /// `views/` / `state/` / `theme/` / `widgets/`」机械化。
    PresentationRich,
    /// `presentation/widgets/` 子层（R-13）：可复用控件。
    PresentationWidgets,
    /// `presentation/icons.rs`（R-13）：字形图标出口。
    PresentationIcons,
    /// 集成测试（`tests/**`）：**消费者**，不是层。
    ///
    /// 测试可以引用任何层的公开 API（`rustmark::domain::…`），但**不许**以
    /// `crate::domain::…` 这种**库内部视角**书写——`crate::` 在集成测试里
    /// 指的是**测试 crate 自身**，写 `crate::domain` 说明作者的心智模型是
    /// 「我在 `src/` 里」，而那正是「绕过公开 API 偷看内部」的起点。
    Tests,
    /// 未登记的顶层位置——**不是自由文件，是覆盖缺口**。
    ///
    /// `main.rs` / `lib.rs` / `bin/` 是约定好的自由位置；其它顶层名字
    /// （`src/foo.rs` / `src/utils/`）不在约束文档的模块树里。判 Unknown
    /// 并直接报错：否则新目录会**静默**逃过全部层禁令——缺口要响，不要静。
    Unknown,
}

impl Layer {
    /// 层名（用于违规文案）。
    fn label(self) -> &'static str {
        match self {
            Layer::Domain => "domain",
            Layer::Application => "application",
            Layer::Infrastructure => "infrastructure",
            Layer::Presentation => "presentation",
            Layer::PresentationState => "presentation/state",
            Layer::PresentationViews => "presentation/views",
            Layer::PresentationRich => "presentation/rich",
            Layer::PresentationWidgets => "presentation/widgets",
            Layer::PresentationIcons => "presentation/icons",
            Layer::Shared => "shared",
            Layer::App => "app",
            Layer::Free => "free",
            Layer::Tests => "tests",
            Layer::Unknown => "unknown",
        }
    }
}

/// 由源文件相对 `src/` 的路径判定所属层。
///
/// # 为什么要把 `.rs` 后缀剥掉再判
///
/// Rust 的层级根模块文件叫 `src/domain.rs`（与 `src/domain/` 同级），
/// 它**属于 domain 层**。旧实现按路径首段判定，首段是 `"domain.rs"` 而不是
/// `"domain"` → 整份文件逃过所有检查。
///
/// 同理 `src/presentation/state.rs` 是 `presentation/state` 子层的根模块，
/// 按同一套规则落子层。
///
/// # 自由位置是**白名单**，不是默认值
///
/// `main.rs` / `lib.rs` / `bin/**` 是约定好的自由位置（crate 入口与工具
/// 二进制）。其它顶层名字一律判 [`Layer::Unknown`] 并报错——默认放行等于
/// 「新目录静默逃过全部层禁令」，而这类缺口不产生编译错误、没人会注意到。
///
/// # `presentation` 内部的细粒度（R-13）
///
/// `presentation` 不是一个均匀的层：`views/` 是编排者，`state/` 是它读的
/// 状态，`rich/` / `widgets/` / `icons.rs` 是被复用的叶子。四类之间必须
/// **单向**（`state` 不得回指 `views`，叶子不得回指 `views`），否则
/// 「状态层反过来驱动视图」会把立即模式的数据流缠成一团。
///
/// 判定时 `parts.get(1)` 同时覆盖两种形态：
/// - `presentation/state/foo.rs` → `parts = ["presentation", "state", "foo"]`
/// - `presentation/state.rs`（**层根文件**）→ `parts = ["presentation", "state"]`
///   第二段同样是 `"state"`，故同一套规则覆盖根文件——这正是方案风险点
///   里点名的「必须对层根文件同样生效」。
fn layer_of(rel: &str) -> Layer {
    let mut parts: Vec<&str> = rel.split('/').collect();
    if let Some(stem) = parts.last().and_then(|l| l.strip_suffix(".rs")) {
        let idx = parts.len() - 1;
        parts[idx] = stem;
    }
    let head = parts.first().copied().unwrap_or("");
    match head {
        "domain" => Layer::Domain,
        "application" => Layer::Application,
        "infrastructure" => Layer::Infrastructure,
        "shared" => Layer::Shared,
        "presentation" => match parts.get(1).copied() {
            Some("state") => Layer::PresentationState,
            Some("views") => Layer::PresentationViews,
            Some("rich") => Layer::PresentationRich,
            Some("widgets") => Layer::PresentationWidgets,
            // `icons.rs` 是**单文件**子层：`parts = ["presentation", "icons"]`。
            Some("icons") => Layer::PresentationIcons,
            _ => Layer::Presentation,
        },
        "app" => Layer::App,
        // crate 入口与 `src/bin/` 工具：约定好的自由位置。
        "bin" => Layer::Free,
        "main" | "lib" if parts.len() == 1 => Layer::Free,
        _ => Layer::Unknown,
    }
}

/// 取某层的禁用前缀。
///
/// **子层是叠加而非替换**：`presentation/state` 先取父层
/// [`PRESENTATION_FORBIDDEN`]，再把 [`PRESENTATION_STATE_FORBIDDEN`] 与
/// [`PRESENTATION_STATE_VIEWS_FORBIDDEN`] 接在后面。若写成只返回子层名单，
/// `state/` 就不再受 `crate::infrastructure` 等父层禁令约束——那是**放宽**
/// 而非收紧，与「子层更严」的意图正好相反。
fn forbidden_for(layer: Layer) -> Vec<&'static str> {
    match layer {
        Layer::Domain => DOMAIN_FORBIDDEN.to_vec(),
        Layer::Application => APPLICATION_FORBIDDEN.to_vec(),
        Layer::Infrastructure => INFRASTRUCTURE_FORBIDDEN.to_vec(),
        Layer::Presentation => PRESENTATION_FORBIDDEN.to_vec(),
        Layer::Shared => SHARED_FORBIDDEN.to_vec(),
        // 集成测试：只禁「库内部视角」，不套任何层的禁令（见 TESTS_FORBIDDEN）。
        Layer::Tests => TESTS_FORBIDDEN.to_vec(),
        // —— R-13：presentation 内部细粒度 ——
        //
        // 全部沿用「子层叠加父层」：先取 PRESENTATION_FORBIDDEN，
        // 再接本子层自己的名单。只返回子层名单等于**放宽**
        // （`crate::infrastructure` 等父层禁令会失效）。
        Layer::PresentationState => {
            let mut v = PRESENTATION_FORBIDDEN.to_vec();
            v.extend_from_slice(PRESENTATION_STATE_FORBIDDEN);
            v.extend_from_slice(PRESENTATION_STATE_VIEWS_FORBIDDEN);
            v
        }
        Layer::PresentationViews => PRESENTATION_FORBIDDEN.to_vec(),
        Layer::PresentationRich => {
            let mut v = PRESENTATION_FORBIDDEN.to_vec();
            v.extend_from_slice(PRESENTATION_RICH_FORBIDDEN);
            v
        }
        Layer::PresentationWidgets => {
            let mut v = PRESENTATION_FORBIDDEN.to_vec();
            v.extend_from_slice(PRESENTATION_WIDGETS_FORBIDDEN);
            v
        }
        Layer::PresentationIcons => {
            let mut v = PRESENTATION_FORBIDDEN.to_vec();
            v.extend_from_slice(PRESENTATION_ICONS_FORBIDDEN);
            v
        }
        // 组合根 / 自由文件 / 未登记位置：没有可判的层禁令。
        // （Unknown 的「报未知层」在 `check_file` 里做——它要先于层禁令返回。）
        Layer::App | Layer::Free | Layer::Unknown => Vec::new(),
    }
}

/// 各层允许的「更窄豁免前缀」。
///
/// # 为什么需要这个机制
///
/// 表现层的视图函数普遍以 `&EditorApp` 接收应用状态
/// （`crate::app::frame::EditorApp`）——这是**被认可的下行通道**：
/// 组合根把状态句柄交给视图，视图读状态、发动作。
///
/// 若一刀切禁掉 `crate::app`，54 个表现层文件会同时报错。那不是「更严的
/// 架构」，而是**一条没人能遵守的规则**——规则一旦无法遵守，例外就会
/// 蔓延成惯例，最后连真正要拦的反向依赖也一起淹掉。
///
/// 但**不能**因此放开整个 `crate::app`：表现层只许碰 `app::frame`
/// （状态句柄与对话框枚举），不许碰 `bootstrap` / `crash_log` /
/// `dialog_runner` / `startup_timing` 这些**启动与基础设施编排**的东西——
/// 那些是组合根的私事，视图伸手去拿就等于绕过了组合根。
///
/// # R-13 起 `views/` 也要这份豁免
///
/// `views/` 独立成子层后，`allowed_for` 若不同步跟上，
/// **35 个视图文件会同时报错**（实测：全是 `use crate::app::frame::EditorApp`
/// 与 `frame::PendingDialog`）——那不是「更严的架构」，而是规则没跟上
/// 自己的细化。方案的既定原则（「超预期则放宽到具体文件而非放弃规则」）
/// 在这里落地为：**豁免跟着子层细化走，窄前缀本身不变**。
fn allowed_for(layer: Layer) -> &'static [&'static str] {
    match layer {
        Layer::Presentation
        | Layer::PresentationState
        | Layer::PresentationViews
        | Layer::PresentationRich
        | Layer::PresentationWidgets
        | Layer::PresentationIcons => &["crate::app::frame"],
        _ => &[],
    }
}

/// 是否表现层（含其全部子层）。R-12 的「不得直写 app 状态字段」只对
/// 表现层生效——`tests/` 与 `src/` 其它层不套这条规则。
fn is_presentation_layer(layer: Layer) -> bool {
    matches!(
        layer,
        Layer::Presentation
            | Layer::PresentationState
            | Layer::PresentationViews
            | Layer::PresentationRich
            | Layer::PresentationWidgets
            | Layer::PresentationIcons
    )
}

// ============================================================================
// 词法
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokKind {
    Ident,
    /// `::`
    PathSep,
    LBrace,
    RBrace,
    LParen,
    RParen,
    Comma,
    Semi,
    Star,
    /// `!`
    Bang,
    /// `.`（与 `Other` 分开：字段访问的 `x.y` 不该被当成路径起始）。
    Dot,
    /// `&`（与 `Other` 分开）。
    ///
    /// `Other` 的 `text` 恒为空，无法区分 `&` 与 `<` / `+` 等单字符——
    /// 而 `tests/presentation/state_borrow_guard_test.rs`（D-3 阶段 2 的
    /// 字段级可变借用清单）要在 token 流里认出 `&mut app.x.y`，
    /// 必须有一个可辨识的取引用 token。它不参与路径判定（那几处
    /// match 只认 `Ident` / `PathSep` / `Dot` / `Assign`），故加它
    /// 不改变任何既有判定结果。
    Amp,
    /// `=`（赋值；与 `==` 区分开，R-12 直写守卫用）。
    Assign,
    /// `==`（相等比较；R-12 不得误伤判据里的 `==`）。
    EqEq,
    Other,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokKind,
    /// 仅 `Ident` 有值。
    text: String,
    line: usize,
}

/// 一行 `// arch-allow: <对象>` 指令。
#[derive(Debug, Clone)]
struct Allow {
    line: usize,
    /// 豁免对象（如 `egui` / `crate::domain`）。
    name: String,
    /// 注释是否**独立成行**（该行在它之前没有代码 token）。
    ///
    /// 这个标记决定豁免窗口的形状，两者 deliberately 不同：
    ///
    /// - **行尾豁免**（`use egui; // arch-allow: egui`）只作用于**本条语句**；
    /// - **独立成行的豁免**额外作用于**紧邻的下一行**（写在 `use egui;`
    ///   上面的那种）。
    ///
    /// 少了这个区分，行尾豁免会顺着「起点前一行也认」漏到下一行，把一条
    /// **独立的**违规一并压掉——豁免写在第 1 行，第 2 行的同类违规照样报到。
    own_line: bool,
}

struct Lexed {
    tokens: Vec<Token>,
    allows: Vec<Allow>,
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: usize,
    /// 最近一次 push 的 token 所在行（0 = 还没有 token）。
    ///
    /// 用于区分「独立成行的注释」与「行尾注释」——豁免窗口的形状取决于这个
    /// 区分（见 [`Allow::own_line`]）。
    last_tok_line: usize,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Lexer {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            line: 1,
            last_tok_line: 0,
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn bump_char(&mut self) -> Option<char> {
        let c = self.peek_char()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn run(mut self) -> Lexed {
        let mut tokens: Vec<Token> = Vec::new();
        let mut allows: Vec<Allow> = Vec::new();
        while self.pos < self.bytes.len() {
            let Some(c) = self.peek_char() else { break };
            match c {
                '/' if self.bytes.get(self.pos + 1) == Some(&b'/') => {
                    self.skip_line_comment(&mut allows);
                }
                '/' if self.bytes.get(self.pos + 1) == Some(&b'*') => {
                    self.skip_block_comment(&mut allows);
                }
                'r' | 'b' if self.is_raw_string_start() => self.skip_raw_string(),
                '"' => self.skip_string(),
                '\'' => self.skip_char_or_lifetime(),
                c if c.is_ascii_digit() => self.skip_number(),
                c if is_ident_start(c) => self.read_ident(&mut tokens),
                ':' if self.bytes.get(self.pos + 1) == Some(&b':') => {
                    self.pos += 2;
                    self.push(&mut tokens, TokKind::PathSep, String::new());
                }
                '{' => {
                    self.pos += 1;
                    self.push(&mut tokens, TokKind::LBrace, String::new());
                }
                '}' => {
                    self.pos += 1;
                    self.push(&mut tokens, TokKind::RBrace, String::new());
                }
                '(' => {
                    self.pos += 1;
                    self.push(&mut tokens, TokKind::LParen, String::new());
                }
                ')' => {
                    self.pos += 1;
                    self.push(&mut tokens, TokKind::RParen, String::new());
                }
                ',' => {
                    self.pos += 1;
                    self.push(&mut tokens, TokKind::Comma, String::new());
                }
                ';' => {
                    self.pos += 1;
                    self.push(&mut tokens, TokKind::Semi, String::new());
                }
                '*' => {
                    self.pos += 1;
                    self.push(&mut tokens, TokKind::Star, String::new());
                }
                '!' => {
                    self.pos += 1;
                    self.push(&mut tokens, TokKind::Bang, String::new());
                }
                // `=` 与 `==` 必须区分（R-12 直写守卫）：`app.x.y =` 是赋值要拦，
                // `app.x.y == z` 是判据里的相等比较要放行。
                '=' if self.bytes.get(self.pos + 1) == Some(&b'=') => {
                    self.pos += 2;
                    self.push(&mut tokens, TokKind::EqEq, String::new());
                }
                '=' => {
                    self.pos += 1;
                    self.push(&mut tokens, TokKind::Assign, String::new());
                }
                _ => {
                    // 空白**不产生 token**——否则 `use egui;` 里 `use` 与 `egui`
                    // 之间的空格会把这条声明拦腰截断（`parse_use_tree` 读到
                    // `Other` 就返回空路径，整条 `use` 声明随之漏检）。
                    // 但换行**必须**计入行号，故走 `bump_char`。
                    if c.is_whitespace() {
                        self.bump_char();
                        continue;
                    }
                    // `.` 单独标记：字段访问的 `x.y` 与路径起始要能区分开。
                    // `&` 单独标记：给借用清单测试留可辨识的取引用 token（见 TokKind::Amp）。
                    let kind = match c {
                        '.' => TokKind::Dot,
                        '&' => TokKind::Amp,
                        _ => TokKind::Other,
                    };
                    self.push(&mut tokens, kind, String::new());
                    self.bump_char();
                }
            }
        }
        Lexed { tokens, allows }
    }

    fn push(&mut self, tokens: &mut Vec<Token>, kind: TokKind, text: String) {
        self.last_tok_line = self.line;
        tokens.push(Token {
            kind,
            text,
            line: self.line,
        });
    }

    fn is_raw_string_start(&self) -> bool {
        let mut i = self.pos;
        if self.bytes.get(i) == Some(&b'b') {
            i += 1;
        }
        if self.bytes.get(i) != Some(&b'r') {
            return false;
        }
        i += 1;
        while self.bytes.get(i) == Some(&b'#') {
            i += 1;
        }
        self.bytes.get(i) == Some(&b'"')
    }

    fn skip_line_comment(&mut self, allows: &mut Vec<Allow>) {
        // 文档注释（`///` / `//!`）不产生豁免——文档里提到标记串不等于豁免。
        let is_doc = matches!(self.bytes.get(self.pos + 2), Some(b'/') | Some(b'!'));
        // 本行在注释之前有没有代码 token：有 → 行尾注释；没有 → 独立成行。
        let own_line = self.last_tok_line != self.line;
        let start = self.pos + 2;
        while let Some(c) = self.peek_char() {
            if c == '\n' {
                break;
            }
            self.bump_char();
        }
        if !is_doc {
            collect_allow(&self.src[start..self.pos], self.line, own_line, allows);
        }
    }

    fn skip_block_comment(&mut self, allows: &mut Vec<Allow>) {
        let is_doc = matches!(self.bytes.get(self.pos + 2), Some(b'*') | Some(b'!'));
        let own_line = self.last_tok_line != self.line;
        let start = self.pos;
        let start_line = self.line;
        self.pos += 2;
        // 深度**从 1 起算**——开头的 `/*` 已经被上面消费掉了。
        // 从 0 起算的话，遇到第一个 `*/` 就会把 depth 减成 -1，
        // `usize` 下溢直接 panic；就算不 panic，也不会在注释结束时停下，
        // 于是注释**后面**的代码全被当成注释吞掉。
        let mut depth = 1usize;
        while self.pos < self.bytes.len() {
            if self.bytes[self.pos] == b'/' && self.bytes.get(self.pos + 1) == Some(&b'*') {
                depth += 1;
                self.pos += 2;
            } else if self.bytes[self.pos] == b'*' && self.bytes.get(self.pos + 1) == Some(&b'/') {
                depth -= 1;
                self.pos += 2;
                if depth == 0 {
                    break;
                }
            } else {
                self.bump_char();
            }
        }
        if !is_doc {
            collect_allow(&self.src[start..self.pos], start_line, own_line, allows);
        }
    }

    fn skip_string(&mut self) {
        self.bump_char();
        while let Some(c) = self.peek_char() {
            if c == '\\' {
                self.bump_char();
                self.bump_char();
                continue;
            }
            self.bump_char();
            if c == '"' {
                break;
            }
        }
    }

    fn skip_raw_string(&mut self) {
        let mut i = self.pos;
        if self.bytes.get(i) == Some(&b'b') {
            i += 1;
        }
        i += 1; // `r`
        let mut hashes = 0usize;
        while self.bytes.get(i) == Some(&b'#') {
            hashes += 1;
            i += 1;
        }
        i += 1; // 开引号
        self.pos = i;
        while self.pos < self.bytes.len() {
            if self.bytes[self.pos] == b'"' {
                let after = self.pos + 1;
                let mut h = 0usize;
                while h < hashes && self.bytes.get(after + h) == Some(&b'#') {
                    h += 1;
                }
                if h == hashes {
                    self.pos = after + hashes;
                    return;
                }
            }
            self.bump_char();
        }
    }

    /// `'x'` / `'\n'` 是字符字面量；`'a` / `'static` 是生命周期。
    fn skip_char_or_lifetime(&mut self) {
        match self.bytes.get(self.pos + 1).copied() {
            Some(b'\\') => {
                // 转义字符字面量（`'\\'` / `'\n'` / `'\''` / `'\0'`）：
                // **定长消费**——引号 + 反斜杠 + 被转义的那一个字符 + 引号。
                //
                // 曾经的实现是「循环找闭合引号」，对 `'\\'` 会这样翻车：
                // 吃掉第一个 `\` 后，循环把第二个 `\` 当成新的转义起始，
                // bump 两次连闭合引号一起吞掉，循环找不到闭合引号便不停，
                // 一路吃到文件里下一个 `'`——字符字面量之后的全部 token
                // 静默丢失（跨层引用藏在后面就全漏检）。2026-10-25 由
                // 守卫②的变异验证发现：`path.rs` 的
                // `rsplit(['/', '\\'])` 正好触发该路径。
                self.pos += 1; // 反斜杠
                self.bump_char();
                self.bump_char(); // 被转义的那一个字符
                if self.peek_char() == Some('\'') {
                    self.bump_char(); // 闭合引号
                }
            }
            Some(_) => {
                let ch = self.src[self.pos + 1..].chars().next();
                if let Some(ch) = ch {
                    let len = ch.len_utf8();
                    if self.bytes.get(self.pos + 1 + len) == Some(&b'\'') {
                        self.pos += 1 + len + 1;
                        return;
                    }
                }
                // 生命周期：只吃掉引号，其后的标识符照常读。
                self.bump_char();
            }
            None => {
                self.bump_char();
            }
        }
    }

    fn skip_number(&mut self) {
        while let Some(c) = self.peek_char() {
            if c.is_alphanumeric() || c == '_' || c == '.' {
                self.bump_char();
            } else {
                break;
            }
        }
    }

    fn read_ident(&mut self, tokens: &mut Vec<Token>) {
        let start = self.pos;
        while let Some(c) = self.peek_char() {
            if is_ident_continue(c) {
                self.bump_char();
            } else {
                break;
            }
        }
        let text = self.src[start..self.pos].to_string();
        self.push(tokens, TokKind::Ident, text);
    }
}

/// 词法层的调试出口：把源码解析成可读的 token 序列。
///
/// 词法层是「哪些东西会被判成路径」的唯一真源，出问题时需要一个
/// 不用猜的窗口。返回形如 `1:Ident(use) 1:Ident(egui) 2:Semi(;)` 的多行文本；
/// 豁免指令另起一行，形如 `allow@2:egui(trailing)` / `allow@1:egui(own_line)`
/// ——`own_line` 决定豁免窗口的形状，调试窗口问题时必须看得见。
///
/// 只在测试目标里编译——`build.rs` 与 `check-arch` bin 都用不到它，
/// 留着会变成 `dead_code` 告警（门禁是 `-D warnings`）。
///
/// `dead_code` 的豁免是必要的：`check-arch` bin 自己也带一个 test target，
/// 那个单元里没有用例调用它（真正用它的用例在 `tests/` 下）。
#[cfg(test)]
#[allow(dead_code)]
pub fn lex_dump(src: &str) -> String {
    let lexed = Lexer::new(src).run();
    let mut s = String::new();
    for t in &lexed.tokens {
        s.push_str(&format!("{}:{:?}({})\n", t.line, t.kind, t.text));
    }
    for a in &lexed.allows {
        let shape = if a.own_line { "own_line" } else { "trailing" };
        s.push_str(&format!("allow@{}:{}({})\n", a.line, a.name, shape));
    }
    s
}

/// 从一段注释文本里抽取 `arch-allow:` 指令。
///
/// 格式：`// arch-allow: <对象>[, <对象>]*`。
///
/// # 解析规则为什么不能「按空白切」
///
/// 豁免后面常跟理由文字，其中可能有 ASCII 词（如「已在 architecture.md 登记」）。
/// 按空白切会把 `architecture.md` 也当成豁免对象 → 凭空多出一条无效豁免。
///
/// # 于是这里的规则是
///
/// 1. 按 `,` 切分（多对象用逗号分隔）；
/// 2. 每段取**开头**的 `[A-Za-z0-9_:]` 连续片段作为对象名；
/// 3. 若该片段**紧跟一个 `.`**，说明它其实是文件名的一部分（`architecture.md`）
///    而不是对象名 → 丢弃这一段。
///
/// 于是 `egui（已在 architecture.md 登记）` → `egui`，
/// 而 `egui, architecture.md 是登记处` → 只认 `egui`。
///
/// `own_line` 见 [`Allow::own_line`]：它决定这条豁免能罩住哪些行。
fn collect_allow(text: &str, line: usize, own_line: bool, out: &mut Vec<Allow>) {
    // 只取**第一个** `arch-allow:` 之后的片段——一条注释里写多个标记串
    // 是笔误，按第一个处理即可。
    let Some(part) = text.split(ARCH_ALLOW).nth(1) else {
        return;
    };
    for piece in part.split(',') {
        let trimmed = piece.trim_start();
        let end = trimmed
            .find(|c: char| !is_path_char(c))
            .unwrap_or(trimmed.len());
        let name = trimmed[..end].trim_end();
        if name.is_empty() {
            continue;
        }
        // 紧跟 `.` → 这段是文件名（如 `architecture.md`），不是对象名。
        if trimmed[end..].starts_with('.') {
            continue;
        }
        out.push(Allow {
            line,
            name: name.to_string(),
            own_line,
        });
    }
}
fn is_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == ':'
}

// ============================================================================
// `use` 树展开
// ============================================================================

/// 一条展开后的完整路径（以及它末段所在的行号）。
type FlatPath = (String, usize);

/// 解析 `use` 声明，展开成完整路径列表。
///
/// 返回 `(路径列表, 结束位置)`。覆盖：
/// - `use a::b::C;`
/// - `pub use a::b::C;` / `pub(crate) use …`（`use` 关键字本身会被识别）
/// - `use a::{b, c};` / `use a::{self, b};` / 嵌套组
/// - `use a::b as c;`（别名不影响依赖对象）
/// - `use a::*;`（glob）
/// - 前导 `::`（`use ::a::b;`）
fn parse_use_tree(tokens: &[Token], start: usize, prefix: &[String]) -> (Vec<FlatPath>, usize) {
    let mut i = start;
    if tokens.get(i).map(|t| t.kind) == Some(TokKind::PathSep) {
        i += 1;
    }
    if tokens.get(i).map(|t| t.kind) == Some(TokKind::LBrace) {
        return parse_use_group(tokens, i, prefix);
    }

    let mut segs: Vec<String> = Vec::new();
    let mut leaf_line = tokens.get(i).map(|t| t.line).unwrap_or(0);
    while let Some(t) = tokens.get(i) {
        if t.kind != TokKind::Ident {
            break;
        }
        segs.push(t.text.clone());
        leaf_line = t.line;
        i += 1;
        if tokens.get(i).map(|t| t.kind) != Some(TokKind::PathSep) {
            break;
        }
        match tokens.get(i + 1).map(|t| t.kind) {
            Some(TokKind::Ident) => i += 1,
            Some(TokKind::LBrace) => {
                let mut full = prefix.to_vec();
                full.extend(segs.iter().cloned());
                return parse_use_group(tokens, i + 1, &full);
            }
            Some(TokKind::Star) => {
                i += 2;
                let mut full = prefix.to_vec();
                full.extend(segs.iter().cloned());
                let mut path = full.join("::");
                path.push_str("::*");
                i = skip_use_alias(tokens, i);
                return (vec![(path, leaf_line)], i);
            }
            _ => {
                i += 1;
                break;
            }
        }
    }

    let leaf = if segs.len() == 1 && segs[0] == "self" {
        if prefix.is_empty() {
            "self".to_string()
        } else {
            prefix.join("::")
        }
    } else {
        let mut full = prefix.to_vec();
        full.extend(segs.iter().cloned());
        full.join("::")
    };
    i = skip_use_alias(tokens, i);
    (vec![(leaf, leaf_line)], i)
}

/// 解析 `use a::{ … }` 的花括号组。`brace_idx` 指向 `{` 本身。
fn parse_use_group(
    tokens: &[Token],
    brace_idx: usize,
    prefix: &[String],
) -> (Vec<FlatPath>, usize) {
    let mut out: Vec<FlatPath> = Vec::new();
    let mut i = brace_idx + 1;
    while let Some(t) = tokens.get(i) {
        match t.kind {
            TokKind::RBrace => {
                i += 1;
                break;
            }
            TokKind::Comma => {
                i += 1;
            }
            TokKind::Semi => break,
            _ => {
                let (paths, next) = parse_use_tree(tokens, i, prefix);
                out.extend(paths);
                i = if next > i { next } else { i + 1 };
            }
        }
    }
    (out, i)
}

/// 跳过 `as 别名` 部分（别名不改变依赖对象本身）。
fn skip_use_alias(tokens: &[Token], mut i: usize) -> usize {
    if tokens.get(i).map(|t| t.text.as_str()) == Some("as") {
        i += 1;
        if tokens.get(i).map(|t| t.kind) == Some(TokKind::Ident) {
            i += 1;
        }
    }
    i
}

// ============================================================================
// 违例候选
// ============================================================================

/// 一条「疑似违规」：路径 + 命中的禁止项 + 报告行号 + 豁免适用窗口。
struct Candidate {
    path: String,
    banned: &'static str,
    /// 报告用行号。
    line: usize,
    /// 豁免指令可出现的行号窗口（含两端）。
    ///
    /// 窗口只是**必要条件**：行尾豁免必须落在窗口内；独立成行的豁免额外
    /// 认「窗口起点前一行」（见 [`Allow::own_line`] 与 `check_file` 里的判定）。
    win_start: usize,
    win_end: usize,
    /// 来源标注：`use` / `extern crate` / `引用`（非 use 位置的限定路径）。
    source: &'static str,
}

impl Candidate {
    /// 违规文案里的来源标注。
    fn kind(&self) -> &'static str {
        self.source
    }
}

/// 路径是否命中禁止项（按**路径段**前缀匹配）。
///
/// `crate::domain` 命中 `crate::domain::x::Y`，但不命中 `crate::domainfoo`；
/// `egui` 命中 `egui::Context`。
fn matches_ban(path: &str, banned: &str) -> bool {
    path == banned
        || (path.len() > banned.len()
            && path.starts_with(banned)
            && path.as_bytes()[banned.len()] == b':')
}

/// 豁免对象名是否覆盖某个禁止项。
///
/// 写 `crate::domain` 能豁免 `crate::domain::document`（子层那条），
/// 写 `crate::domain::document` 也能豁免父层的 `crate::domain`——
/// 两种写法都认，避免「豁免写细了反而没生效」。
fn allow_covers(name: &str, banned: &str) -> bool {
    name == banned
        || (banned.len() > name.len()
            && banned.starts_with(name)
            && banned.as_bytes()[name.len()] == b':')
        || (name.len() > banned.len()
            && name.starts_with(banned)
            && name.as_bytes()[banned.len()] == b':')
}

// ============================================================================
// 豁免登记表
// ============================================================================

/// `docs/architecture-constraints.md` 里的豁免登记表。
#[derive(Debug, Default)]
struct Registry {
    /// `(文件相对 src 的路径, 豁免对象)`。
    rows: Vec<(String, String)>,
}

impl Registry {
    fn contains(&self, file: &str, name: &str) -> bool {
        self.rows.iter().any(|(f, n)| f == file && n == name)
    }
}

/// 读取豁免登记表。登记表路径约定为 `<manifest>/docs/architecture-constraints.md`。
fn load_registry(src_root: &Path) -> Registry {
    let manifest = src_root.parent().unwrap_or(src_root);
    load_registry_at(manifest)
}

/// 按 **manifest** 加载豁免登记表。
///
/// # 为什么必须按 manifest 而不是按扫描根
///
/// D-3 阶段 4a workspace 化后，core 根的 parent 是 `crates/rustmark-core`——
/// 那里没有 `docs/`，逐根加载会让 core 侧全部豁免变成「未登记」。
/// 登记表描述的是**整个工程**的豁免，与根无关，故按 manifest 载入一次、
/// 所有根共用（[`scan_project`] 的做法）。
fn load_registry_at(manifest: &Path) -> Registry {
    let path = manifest.join("docs").join("architecture-constraints.md");
    let Ok(text) = fs::read_to_string(&path) else {
        return Registry::default();
    };
    parse_registry(&text)
}

fn parse_registry(text: &str) -> Registry {
    let rows = parse_marked_table(text, REGISTRY_BEGIN, REGISTRY_END)
        .into_iter()
        .filter_map(|cells| {
            let name = cells.get(1)?;
            if name.is_empty() {
                return None;
            }
            Some((normalize_registry_file(&cells[0]), name.clone()))
        })
        .collect();
    Registry { rows }
}

/// 解析标记对之间的 Markdown 表格，返回每行的单元格（已 trim、去反引号）。
///
/// 表头行（首格 `文件`）与分隔行（`---`）不是登记项，统一跳过——
/// 两张登记表（豁免表 / shared 准入表）的行走规则只有「取哪几列」的差别，
/// 行的语法共用这一处，避免两份遍历各自漂移。
fn parse_marked_table(text: &str, begin: &str, end: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut in_table = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with(begin) {
            in_table = true;
            continue;
        }
        if t.starts_with(end) {
            in_table = false;
            continue;
        }
        if !in_table || !t.starts_with('|') {
            continue;
        }
        let cells: Vec<String> = t
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().trim_matches('`').trim().to_string())
            .collect();
        let Some(file) = cells.first() else {
            continue;
        };
        if file.is_empty() || file.starts_with('-') || file == "文件" {
            continue;
        }
        rows.push(cells);
    }
    rows
}

/// `shared` 准入登记表（§4.5 三问；D-6）：`shared/` 下已登记的文件。
#[derive(Debug, Default)]
struct SharedRegistry {
    /// 层相对路径（如 `shared/theme.rs`）。
    files: Vec<String>,
}

impl SharedRegistry {
    fn contains(&self, rel: &str) -> bool {
        self.files.iter().any(|f| f == rel)
    }
}

/// 按 manifest 读取 `shared` 准入登记表（与豁免表同一文件、不同区段）。
///
/// 路径按 manifest 而非扫描根，理由与 [`load_registry_at`] 相同：
/// core 根的 parent 没有 `docs/`。
fn load_shared_registry_at(manifest: &Path) -> SharedRegistry {
    let path = manifest.join("docs").join("architecture-constraints.md");
    let Ok(text) = fs::read_to_string(&path) else {
        return SharedRegistry::default();
    };
    parse_shared_registry(&text)
}

fn parse_shared_registry(text: &str) -> SharedRegistry {
    let files = parse_marked_table(text, SHARED_REGISTRY_BEGIN, SHARED_REGISTRY_END)
        .into_iter()
        .filter_map(|cells| cells.into_iter().next())
        .map(|file| normalize_shared_file(&file))
        .collect();
    SharedRegistry { files }
}

/// 登记表里的文件列允许写成 `src/domain/x.rs` 或 `domain/x.rs`，统一成后者。
fn normalize_registry_file(raw: &str) -> String {
    let s = raw.replace('\\', "/");
    let s = s.strip_prefix("./").unwrap_or(s.as_str());
    s.strip_prefix("src/").unwrap_or(s).to_string()
}

/// shared 登记表的文件列允许写成 `shared/x.rs`、`src/shared/x.rs` 或
/// `crates/rustmark-core/src/shared/x.rs`，统一成**根内相对**的 `shared/x.rs`
/// （与层判定 [`layer_of`] 的输入同口径）。
fn normalize_shared_file(raw: &str) -> String {
    let s = normalize_registry_file(raw);
    s.strip_prefix("crates/rustmark-core/src/")
        .unwrap_or(&s)
        .to_string()
}

// ============================================================================
// 主扫描
// ============================================================================

fn collect_rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}

/// D-4 视图文件拆分阈值（docs/architecture-constraints.md:447）：`presentation/views/`
/// 下单文件 **> 600 行须拆子目录**（入口 `foo.rs` + 子模块 `foo/bar.rs`，
/// `pub use` 重导出零变化）。
///
/// 此前这是**评审纪律**（constraints.md:463「当前未机械校验」），现为机械校验，
/// 由 [`scan_project`] 在 CI（`build.rs`）与本地门禁（`check-arch` bin）生效。
/// 排除项：测试文件——`tests.rs` 与任何 `tests/` 目录。D-4 针对生产视图代码，
/// 测试模块（含 `preview_view/tests.rs` 这种 600+ 行的单测外置文件）不受此限。
const VIEW_FILE_LINE_LIMIT: usize = 600;

fn check_view_file_sizes(views_root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    if !views_root.is_dir() {
        return out;
    }
    let mut files = Vec::new();
    collect_rs_files(views_root, &mut files);
    for path in &files {
        // 排除测试文件：任意深度的 `tests.rs`（即 `#[cfg(test)] mod tests;`
        // 约定文件）与任意深度的 `tests/` 目录。D-4 针对生产视图代码，
        // 测试模块（含 `preview_view/tests.rs` 这种嵌套 600+ 行单测外置文件）
        // 不受此限。注意 `tests.rs` 作为文件名不等于目录段 `tests`，须分别判定。
        let rel = path
            .strip_prefix(views_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let is_tests_file = path
            .file_name()
            .map(|n| n == "tests.rs")
            .unwrap_or(false);
        let in_tests_dir = rel.split('/').any(|seg| seg == "tests");
        if is_tests_file || in_tests_dir {
            continue;
        }
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        // 以 `\n` 计数（与 `wc -l` 口径一致；CRLF 下仍是逻辑行数）。
        let lines = bytes.iter().filter(|&&b| b == b'\n').count();
        if lines > VIEW_FILE_LINE_LIMIT {
            out.push(Violation {
                file: format!("src/presentation/views/{rel}"),
                line: 1,
                detail: format!(
                    "D-4 视图文件拆分阈值：`{}` 共 {} 行 > 600 行上限 —— \
                     须拆子目录（入口 `foo.rs` + 子模块 `foo/bar.rs`，`pub use` 重导出零变化）",
                    rel, lines
                ),
            });
        }
    }
    out
}

/// 扫描 `src_root` 下的全部 `.rs` 文件，返回违规列表（空表示通过）。
///
/// **签名保持不变**（`build.rs` 与 `check-arch` bin 之外的调用点、以及
/// `tests/arch_violation_fixture.rs` 的既有用例都只用它）。要连 `tests/`
/// 一起扫，用 [`scan_with_tests`]。
///
/// 之所以仍保留：`scan` 是「只扫 `src/`」这一口径的显式表达，fixture 的
/// 「`scan` 不报、`scan_with_tests` 报」正是 R-02 验收判据的形状。
#[allow(dead_code)]
pub fn scan(src_root: &Path) -> Vec<Violation> {
    scan_impl(src_root, None)
}

/// 扫描 `src_root` **与** `tests_root`，返回违规列表（空表示通过）。
///
/// # 两侧的层判定不同
///
/// - `src/` 走 [`layer_of`]（按目录判层，未知顶层名字报「未知层」）；
/// - `tests/` 下**全部**文件统一判 [`Layer::Tests`]——测试是消费者不是层，
///   且 `tests/` 的目录名（`tests/domain/`、`tests/presentation/`）是
///   **测试目标的划分**，与 `src/` 的层毫无关系。若按目录名判层，
///   `tests/presentation/main.rs` 会被套上表现层禁令，
///   `tests/domain/main.rs` 会被套上领域层禁令——那是**误判**。
///
/// `tests/` 侧的文件在报告里带 `tests/` 前缀，与 `src/` 侧的可区分。
///
/// `#[allow(dead_code)]` 理由与 [`scan`] 相同：真实工程入口（`build.rs` /
/// `check-arch`）自 D-3 阶段 4a 起改用 [`scan_project`]，本函数只剩
/// fixture 的合成目录用例在用——那些 include 点没用到时不该以
/// dead_code 告警（`-D warnings` 会把它升成编译失败）。
#[allow(dead_code)]
pub fn scan_with_tests(src_root: &Path, tests_root: &Path) -> Vec<Violation> {
    scan_impl(src_root, Some(tests_root))
}

/// 扫描**整个 workspace**（D-3 阶段 4a 拆分后的口径）：
/// shell `src/` + `crates/rustmark-core/src/` +
/// `crates/rustmark-actions/src/`（V11-57）+
/// `crates/rustmark-icons/src/`（V11-58）+
/// `crates/rustmark-ui/src/`（V11-62）+
/// `crates/rustmark-shared/src/`（TD-04，2026-10-26）+ `tests/`。
///
/// # 七个根、两种记键
///
/// - src 根**按各自根的层相对路径**判层、查豁免表
///   （core 的 `domain/x.rs` 与拆分前同键，登记表与既有禁令表零改动）；
///   actions 根例外：整个根判 [`Layer::Free`]（零依赖词汇契约 crate，
///   文件名不在任何层名里，不覆盖会误报「未知层」）；
///   icons 根**无需**强制：单文件 `lib.rs` 落在 crate 入口白名单里
///   （`layer_of` 对 `lib.rs` 自然判 Free），将来若长出第二个文件，
///   「未知层」警报会逼着给它定层——这正是缺口要响的意图；
///   ui 根同样**不设**强制层：批次 1 起保留 `presentation/` 路径前缀，
///   `layer_of` 按前缀判对应子层（`presentation/widgets.rs` →
///   [`Layer::PresentationWidgets`]），层禁令表零改动，views 若被误搬进
///   ui 会落在 `presentation/views` 子层自动触发既有禁令（fixture 第十七节）；
/// - core 侧违规只在**报告**时补 `crates/rustmark-core/src/` 前缀——
///   层判定绝不能看该前缀，否则首段 `crates` 会把整个 core 判成
///   「未知层」而全量误报；
/// - `tests/` 侧与 [`scan_with_tests`] 完全一致（全判 [`Layer::Tests`]、
///   rel 带 `tests/` 前缀）。
///
/// # 登记表按 manifest 加载一次、六个 src 根共用
///
/// core 根的 parent 是 `crates/rustmark-core`，那里没有 `docs/`——
/// 沿用 [`load_registry`] 的「parent 即 manifest」推断会让 core 侧豁免
/// 全部变成「未登记」，故显式 [`load_registry_at`]（manifest）。
///
/// # `shared` 准入登记只在本入口查（§4.5 三问 / D-6）
///
/// `shared/` 下的每个文件须登记在约束文档的 shared 登记表里，未登记即违规
/// （文件级、报首行）。合成目录用例（[`scan`] / [`scan_with_tests`]）**不查**
/// ——那里没有登记表语境，边界由 fixture 用例双向钉住。
///
/// [`scan`] / [`scan_with_tests`] 的签名与口径**保持不变**：fixture 的
/// 合成目录用例靠它们表达「只扫一个根」的范围差异；真实工程的入口
/// （`build.rs` / `check-arch` / fixture 的零违规守卫）一律用本函数。
pub fn scan_project(manifest: &Path) -> Vec<Violation> {
    let registry = load_registry_at(manifest);
    let shared_registry = load_shared_registry_at(manifest);
    let mut violations = Vec::new();

    // src 根们：层判定用**根内相对路径**，报告字段补显示前缀。
    //
    // 第三根（V11-57）：`rustmark-actions` 是零依赖词汇契约 crate，整个根
    // 判 Free（`forced_layer`）——它的文件名（`action.rs` / `action/`）不在
    // 任何层名里，不覆盖会落 Unknown 层误报（「未知层」是覆盖缺口的警报，
    // 对已登记的自由 crate 只是噪音）。Free 只豁免**层禁令**；与层无关的
    // 规则（禁 `mod.rs` / 帧预约唯一出口 / 豁免体检）在第三根照旧生效，
    // 由 fixture 第十六节钉住。
    //
    // 第四根（V11-58）：`rustmark-icons` 是向量图标资源 crate（单文件
    // `lib.rs` + 4 个规格单测），`layer_of` 对 `lib.rs` 自然判 Free，故
    // **不**设 `forced_layer`。它与层无关的规则（禁 `mod.rs` / 帧预约）
    // 与其他根同一套代码路径生效。
    //
    // 第五根（V11-62 批次 1）：`rustmark-ui` 是表现层非视图 crate（当前
    // 仅 widgets）。保留 `presentation/` 路径前缀，`layer_of` 按前缀判
    // 对应子层，故同样**不**设 `forced_layer`——层判定零改动即等价于
    // 「widgets 还在 core」的旧判定，而 views 若被误搬进 ui 会落在
    // `presentation/views` 子层自动触发既有禁令（ADR-013 决策 3 的结构
    // 防护：views 留 root 不靠人记，靠路径判层自动生效）。
    //
    // 「actions 不得引用 core / 任何 crate」刻意**不**做成词法禁令：core
    // 已依赖 actions，反向加普通依赖即 Cargo 循环依赖（解析期报错，比词法
    // 规则更强）；「获得任意依赖」由 workspace 拓扑断言兜底
    // （docs/workspace-topology.expected.txt）。词法规则只多挡
    // dev-dependency 一种形状，不值得为之新增层类别——校验器教条是
    // 「确需新的层类别，先改约束文档与校验器」。
    for (root, display_prefix, forced_layer) in [
        (manifest.join("src"), "", None),
        (
            manifest.join("crates").join("rustmark-core").join("src"),
            "crates/rustmark-core/src/",
            None,
        ),
        (
            manifest.join("crates").join("rustmark-actions").join("src"),
            "crates/rustmark-actions/src/",
            Some(Layer::Free),
        ),
        (
            manifest.join("crates").join("rustmark-icons").join("src"),
            "crates/rustmark-icons/src/",
            None,
        ),
        (
            manifest.join("crates").join("rustmark-ui").join("src"),
            "crates/rustmark-ui/src/",
            None,
        ),
        // 第六根（TD-04，2026-10-26）：共享内核抽为 `rustmark-shared`。
        // 文件保留 `shared/` 路径前缀（与 ui 保留 `presentation/` 同理），
        // `layer_of("shared/x.rs")` 自然判 [`Layer::Shared`]——不设强制层，
        // §4.5 登记守卫的 `rel.starts_with("shared/")` 也随路径原样生效。
        (
            manifest.join("crates").join("rustmark-shared").join("src"),
            "crates/rustmark-shared/src/",
            None,
        ),
    ] {
        let mut files = Vec::new();
        collect_rs_files(&root, &mut files);
        files.sort();
        for path in &files {
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let display = format!("{display_prefix}{rel}");
            if let Some(v) = check_mod_rs(path, &display) {
                violations.push(v);
            }
            let layer = forced_layer.unwrap_or_else(|| layer_of(&rel));
            // shared 准入登记（§4.5 三问 / D-6）：只挂在**真实工程**口径上。
            // `scan` / `scan_with_tests` 的合成目录用例没有登记表语境，
            // 边界由 fixture 的「合成扫描不套用登记制」用例钉住。
            // 判据限定 `shared/` **目录内**的文件：层根 `shared.rs` 只有
            // `pub mod` 声明（无实现内容），不属三问的对象。TD-04 抽出独立
            // crate 后路径前缀保留，登记键与判定逐字不变。
            if rel.starts_with("shared/") && !shared_registry.contains(&rel) {
                violations.push(Violation {
                    file: display.clone(),
                    line: 1,
                    detail: format!(
                        "shared 准入三问未登记（§4.5 / D-6）：`{rel}` 不在 \
                         docs/architecture-constraints.md 的 shared 登记表里——进 `shared/` 前 \
                         须答 ①是否 ≥2 层需要 ②是否零 IO ③是否无可归属的层，登记一行方可准入"
                    ),
                });
            }
            let Ok(content) = fs::read_to_string(path) else {
                continue;
            };
            let lexed = Lexer::new(&content).run();
            for mut v in check_file(&rel, layer, &lexed, &registry) {
                // 层判定与豁免键已按层相对路径算完，最后才补显示前缀。
                v.file = display.clone();
                violations.push(v);
            }
        }
    }

    // tests 根：与 scan_with_tests 同口径（前缀进 rel、全判 Tests 层）。
    let tests_root = manifest.join("tests");
    let mut test_files = Vec::new();
    collect_rs_files(&tests_root, &mut test_files);
    test_files.sort();
    for path in &test_files {
        let rel = path
            .strip_prefix(&tests_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let rel = format!("tests/{rel}");
        if let Some(v) = check_mod_rs(path, &rel) {
            violations.push(v);
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let lexed = Lexer::new(&content).run();
        violations.extend(check_file(&rel, Layer::Tests, &lexed, &registry));
    }

    // D-4 视图文件拆分阈值（constraints.md:447）：`presentation/views/` 下生产视图
    // 源文件 > 600 行须拆子目录。此前仅评审纪律，现机械校验（见 `check_view_file_sizes`）。
    violations.extend(check_view_file_sizes(
        &manifest.join("src").join("presentation").join("views"),
    ));

    violations
}

/// 模块布局（`CONSTR-03`）：`mod.rs` 一律不许出现。
///
/// 判据只有文件名，与源码内容无关——所以它在 `scan_impl` 里按路径判，
/// 而不是在 [`check_file`] 里扫描 token。`src/` 与 `tests/` 两侧同样适用
/// （`tests/support/mod.rs` 与 `src/infrastructure/fonts/mod.rs` 都是本轮
/// 实际整改掉的存量）。
///
/// `rel` 是报告用相对路径（`tests/` 侧已带前缀），故文案与其它违规一致。
fn check_mod_rs(path: &Path, rel: &str) -> Option<Violation> {
    let name = path.file_name()?.to_str()?;
    if name != FORBIDDEN_MOD_FILE {
        return None;
    }
    Some(Violation {
        file: rel.to_string(),
        line: 1,
        detail: format!(
            "禁止 {FORBIDDEN_MOD_FILE}（CONSTR-03）：一级模块用 `xxx.rs`、子模块放 `xxx/`\
             （如 `fonts.rs` 与 `fonts/assembly.rs` 共存）；`mod.rs` 会让模块树与文件树脱节，\
             且编译器不会因此报错，只能由本规则守住"
        ),
    })
}

fn scan_impl(src_root: &Path, tests_root: Option<&Path>) -> Vec<Violation> {
    let registry = load_registry(src_root);
    let mut violations = Vec::new();

    let mut files = Vec::new();
    collect_rs_files(src_root, &mut files);
    files.sort();

    for path in &files {
        let rel = path
            .strip_prefix(src_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(v) = check_mod_rs(path, &rel) {
            violations.push(v);
        }
        let layer = layer_of(&rel);
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let lexed = Lexer::new(&content).run();
        violations.extend(check_file(&rel, layer, &lexed, &registry));
    }

    if let Some(tests_root) = tests_root {
        let mut test_files = Vec::new();
        collect_rs_files(tests_root, &mut test_files);
        test_files.sort();
        for path in &test_files {
            let rel = path
                .strip_prefix(tests_root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            // 带 `tests/` 前缀：报告里一眼能看出违规在测试侧，
            // 且不会与 `src/presentation/…` 之类的同名路径混淆。
            let rel = format!("tests/{rel}");
            if let Some(v) = check_mod_rs(path, &rel) {
                violations.push(v);
            }
            let Ok(content) = fs::read_to_string(path) else {
                continue;
            };
            let lexed = Lexer::new(&content).run();
            violations.extend(check_file(&rel, Layer::Tests, &lexed, &registry));
        }
    }

    violations
}

/// 统一的违规报告文本。
///
/// `build.rs` 与 `check-arch` bin 共用同一份格式——两处各写一遍必然漂移。
pub fn format_report(violations: &[Violation]) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "架构边界校验失败，共 {} 处违规：\n",
        violations.len()
    ));
    for v in violations {
        s.push_str(&format!("  {}:{} —— {}\n", v.file, v.line, v.detail));
    }
    s.push_str("\n规则见 docs/architecture-constraints.md\n");
    s
}

fn check_file(rel: &str, layer: Layer, lexed: &Lexed, registry: &Registry) -> Vec<Violation> {
    let mut violations: Vec<Violation> = Vec::new();
    let tokens = &lexed.tokens;

    // ---- 定时任务的帧预约只允许一个出口（T11 / ADR-009）----
    // 对**所有**文件生效（含组合根 app/）：调用点自己预约帧，
    // 正是历史上「定时功能完全不生效」的根因。
    for (idx, t) in tokens.iter().enumerate() {
        if t.kind == TokKind::Ident
            && t.text == REPAINT_RESERVATION
            && rel != REPAINT_RESERVATION_HOME
        {
            violations.push(Violation {
                file: rel.to_string(),
                line: t.line,
                detail: format!(
                    "定时任务的帧预约必须走 FrameScheduler（{REPAINT_RESERVATION} 只允许出现在 \
                     {REPAINT_RESERVATION_HOME}）：调用点只 `arm` 到期点，否则「新增定时功能\
                     →就漏预约」的老问题会原样复发"
                ),
            });
        }
        // 领域层禁止直接向标准输出/错误输出打印。
        if layer == Layer::Domain
            && t.kind == TokKind::Ident
            && PRINT_MACROS.contains(&t.text.as_str())
            && tokens.get(idx + 1).map(|n| n.kind) == Some(TokKind::Bang)
        {
            violations.push(Violation {
                file: rel.to_string(),
                line: t.line,
                detail: format!(
                    "领域层禁止 {}!（应通过错误或领域事件表达；标准输出会被诊断信息污染）",
                    t.text
                ),
            });
        }
    }

    // ---- R-12：表现层不得直写 app 状态字段（赋值形态）----
    //
    // `app.<state>.<field> =` 是 R-11 消除掉的「视图直接改 EditorApp 字段」
    // 形状。R-11 把它收敛到 `EditorApp` 的方法入口，本规则把「零直写」
    // 机械化：将来有人图省事在视图里直接 `app.x.y = z`，编译照过、
    // 单测未必覆盖，这条把它钉死。
    //
    // 已知边界（必须放行）：
    // - `&mut app.<state>.<field>`（egui 控件必须拿 `&mut String` /
    //   `&mut bool`，如 `TextEdit::singleline(&mut app.x.y)` /
    //   `ui.checkbox(&mut app.x.y, …)`）——那是**借用**，序列以 `)` 收尾，
    //   不以 `=` 收尾，自然不命中。
    // - `app.x.y == z` 是**比较**不是赋值，用 `EqEq` 与 `Assign` 区分开，
    //   不能误伤判据里的相等比较（`!=` 是 `Bang` 后跟 `Assign`，同样不命中）。
    // R-12-ANCHOR-START
    if is_presentation_layer(layer) {
        let mut k = 0;
        while k < tokens.len() {
            if tokens[k].kind == TokKind::Ident && tokens[k].text == "app" {
                // 构造点访问路径 `app.<ident>( .<ident> )*`。
                let mut path = String::from("app");
                let mut j = k;
                while tokens.get(j + 1).map(|n| n.kind) == Some(TokKind::Dot)
                    && tokens.get(j + 2).map(|n| n.kind) == Some(TokKind::Ident)
                {
                    path.push('.');
                    path.push_str(&tokens[j + 2].text);
                    j += 2;
                }
                // 至少一段点访问 + 紧随其后的单个 `=`（不是 `==` / `!=`）。
                if tokens.get(j + 1).map(|n| n.kind) == Some(TokKind::Assign) {
                    violations.push(Violation {
                        file: rel.to_string(),
                        line: tokens[k].line,
                        detail: format!(
                            "presentation 层不得直写 app 状态字段（{path} = …）：视图应通过 \
                             EditorApp 的方法入口写状态，不得直接改字段（见架构改造 R-11；\
                             已知边界：`&mut {path}` 借用与 `{path} == …` 比较）"
                        ),
                    });
                    k = j + 2;
                    continue;
                }
            }
            k += 1;
        }
    }
    // R-12-ANCHOR-END

    // ---- 未登记的顶层位置：覆盖缺口，必须响 ----
    //
    // 放在帧预约检查**之后**：那条规则与层无关，未知文件同样受它约束。
    if layer == Layer::Unknown {
        violations.push(Violation {
            file: rel.to_string(),
            line: 1,
            detail: format!(
                "未知层：{rel} 不在架构约束的模块树里（docs/architecture-constraints.md）。\
                 要么把它归入既有层（domain / application / infrastructure / shared / \
                 presentation / app），要么它是 src/bin/ 工具；若确实需要新的层类别，\
                 先改约束文档与校验器，再放代码"
            ),
        });
        return violations;
    }

    // ---- 依赖路径判定 ----
    //
    // 没有层禁令的层（组合根 / 自由文件）跳过候选收集，但**豁免体检照跑**——
    // 那些位置写 `arch-allow` 同样是噪音，应当报出来而不是被静默忽略。
    let mut used_allow = vec![false; lexed.allows.len()];
    let forbidden = forbidden_for(layer);
    if !forbidden.is_empty() {
        let candidates = collect_candidates(tokens, &module_path_of(rel), &forbidden, allowed_for(layer));
        for cand in &candidates {
            let hit = lexed
                .allows
                .iter()
                .enumerate()
                .find(|(_, a)| {
                    allow_covers(&a.name, cand.banned)
                        && (cand.win_start <= a.line && a.line <= cand.win_end
                            // 独立成行的豁免额外罩住紧邻的下一行；行尾豁免不罩——
                            // 否则第 1 行的行尾豁免会把第 2 行的独立违规一并压掉。
                            || (a.own_line && cand.win_start == a.line + 1))
                })
                .map(|(idx, _)| idx);
            match hit {
                Some(idx) => {
                    used_allow[idx] = true;
                }
                None => {
                    let reason = if layer == Layer::Tests {
                        "（集成测试是消费者不是层：引用库请写公开 API `rustmark::…`；\
                         `crate::` 在测试里指向测试 crate 自身，写 `crate::<层名>` \
                         说明是从 src/ 复制粘贴过来的库内部视角）"
                    } else if layer == Layer::PresentationState
                        && PRESENTATION_STATE_FORBIDDEN.contains(&cand.banned)
                    {
                        "（UI State 只放视图状态：开关 / 滚动 / 焦点 / 临时输入；\
                         业务数据一律来自 DTO，见 docs/06 表现层与交互实现）"
                    } else {
                        ""
                    };
                    violations.push(Violation {
                        file: rel.to_string(),
                        line: cand.line,
                        detail: format!(
                            "{} 层禁止依赖 {}（{} {}）{}",
                            layer.label(),
                            cand.banned,
                            cand.kind(),
                            cand.path,
                            reason
                        ),
                    });
                }
            }
        }
    }

    // ---- 豁免指令自身的体检 ----
    //
    // 对**所有**层生效（含组合根 / 自由文件）：那些位置没有层禁令，
    // 因此那里的 `arch-allow` 必然是无对象的噪音——报出来，而不是
    // 因为「提前返回」被静默忽略（三态表才对所有文件成立）。
    for (idx, a) in lexed.allows.iter().enumerate() {
        if used_allow[idx] {
            if !registry.contains(rel, &a.name) {
                violations.push(Violation {
                    file: rel.to_string(),
                    line: a.line,
                    detail: format!(
                        "arch-allow 豁免未登记：{}（须在 docs/architecture-constraints.md 的豁免表里登记 \
                         「{} | {}」）",
                        a.name, rel, a.name
                    ),
                });
            }
            continue;
        }
        // 该行确实有帧预约违规时，说清楚「这条规则不设豁免」——
        // 否则读者会以为是窗口写错了，去挪注释位置而不是删掉预约。
        let repaint_here = violations
            .iter()
            .any(|v| v.line == a.line && v.detail.contains(REPAINT_RESERVATION));
        let detail = if repaint_here {
            format!(
                "arch-allow 豁免无效：{}（该行确实有违规，但「{REPAINT_RESERVATION} 只允许出现在 \
                 {REPAINT_RESERVATION_HOME}」这条规则不设豁免）",
                a.name
            )
        } else {
            format!(
                "arch-allow 豁免无效：{}（该行没有依赖它的违规，豁免要么写错、要么已过时）",
                a.name
            )
        };
        violations.push(Violation {
            file: rel.to_string(),
            line: a.line,
            detail,
        });
    }

    violations
}

/// 该文件对应的**模块路径**（含开头的 `crate`）。
///
/// # 为什么这条路走得通
///
/// 本工程**禁 `mod.rs`**（[`check_mod_rs`]），于是「文件路径 → 模块路径」
/// 是一一对应：`domain/document/aggregate.rs` 就是
/// `crate::domain::document::aggregate`。有 `mod.rs` 就得先把 `mod x;`
/// 声明表建起来才能数 `super` 跳几层，而声明表只有解析器看得见——
/// 词法器看不到。禁 `mod.rs` 顺带让这道门变得可算，属于规则复用。
fn module_path_of(rel: &str) -> Vec<String> {
    let stem = rel.strip_suffix(".rs").unwrap_or(rel);
    // crate 入口文件自身就是 `crate`（`lib.rs` / `main.rs`）。
    if !stem.contains('/') && matches!(stem, "lib" | "main") {
        return vec!["crate".to_string()];
    }
    let mut segs = vec!["crate".to_string()];
    segs.extend(stem.split('/').map(|s| s.to_string()));
    segs
}

/// 内联模块的最大假定嵌套层数。
///
/// `#[cfg(test)] mod tests { use super::…; }` 里 `super` 相对的是那个内联
/// 模块，比文件本身深一层。词法器不知道自己在第几层内联模块里，故每个文件
/// 深度都往深了多试几层。
///
/// **宁深勿浅**：多试一层顶多在极端嵌套下多报一次（且报的两条指向同一行的
/// 同一条禁令，去重后仍是一条），少试一层就让跨层写法溜过去了——漏报才是
/// 这道门要防的事。取 2 是因为三层内联模块在 Rust 里已属罕见写法。
const MAX_INLINE_MODULE_DEPTH: usize = 2;

/// 相对路径（`super::…` / `self::…`）是否成立。
fn is_relative_path(path: &str) -> bool {
    path.starts_with("super::") || path.starts_with("self::") || path == "super" || path == "self"
}

/// 把相对路径解析成绝对路径；无法解析（弹出到 `crate` 之上）返回 `None`。
///
/// `inline_depth`：假定该处位于文件模块之下第几层内联模块。
fn relative_to_absolute(module: &[String], path: &str, inline_depth: usize) -> Option<String> {
    let mut base: Vec<String> = module.to_vec();
    // 内联模块的层级名不可知，用占位段占位（只参与 `super` 的弹出计数）。
    base.resize(base.len() + inline_depth, "<inline>".to_string());

    let segs: Vec<&str> = path.split("::").collect();
    let mut i = 0usize;
    while i < segs.len() {
        match segs[i] {
            // `super` 弹出一层；已经到 `crate` 就说明这行源码本身编译不过，
            // 交编译器管，这里不把它当成依赖。
            "super" => {
                if base.len() <= 1 {
                    return None;
                }
                base.pop();
                i += 1;
            }
            // `self` 不改变当前模块。
            "self" => i += 1,
            _ => break,
        }
    }
    // 全是 `super`/`self` 或压根不相对：前者不成依赖，后者 caller 不该调用本函数。
    if i == 0 || i >= segs.len() {
        return None;
    }
    // 还剩占位段说明没能弹出真实的位置（模块树比假定浅），这条猜测作废。
    if base.iter().any(|s| s == "<inline>") {
        return None;
    }
    Some(format!("{}::{}", base.join("::"), segs[i..].join("::")))
}

/// 收集全部「疑似违规」候选。
fn collect_candidates(
    tokens: &[Token],
    module: &[String],
    forbidden: &[&'static str],
    allowed: &[&'static str],
) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        let t = &tokens[i];
        // `use` 声明：整体展开成完整路径。
        if t.kind == TokKind::Ident && t.text == "use" {
            let (paths, end) = parse_use_tree(tokens, i + 1, &[]);
            let stmt_end = end
                .checked_sub(1)
                .and_then(|k| tokens.get(k))
                .map(|x| x.line)
                .unwrap_or(t.line);
            for (path, line) in paths {
                push_path_candidates(
                    &mut out,
                    &path,
                    line,
                    t.line,
                    stmt_end,
                    "use",
                    module,
                    forbidden,
                    allowed,
                );
            }
            i = if end > i { end } else { i + 1 };
            continue;
        }
        // `extern crate egui;` / `extern crate egui as e;`
        // （2015 版式依赖声明，`#[macro_use]` 重导出场景仍会出现）
        // ——与 `use` 同等对待：只认 `use` 会让这条整类漏检。
        // 注意 `extern "C" { … }` / `extern "C" fn` 的 `extern` 后面跟的是
        // 字符串（不产生 token）或 `{` / `fn`，到不了这里。
        if t.kind == TokKind::Ident
            && t.text == "extern"
            && tokens.get(i + 1).map(|n| n.text.as_str()) == Some("crate")
        {
            let (paths, end) = parse_use_tree(tokens, i + 2, &[]);
            let stmt_end = end
                .checked_sub(1)
                .and_then(|k| tokens.get(k))
                .map(|x| x.line)
                .unwrap_or(t.line);
            for (path, line) in paths {
                push_path_candidates(
                    &mut out,
                    &path,
                    line,
                    t.line,
                    stmt_end,
                    "extern crate",
                    module,
                    forbidden,
                    allowed,
                );
            }
            i = if end > i { end } else { i + 1 };
            continue;
        }
        // 其它位置的限定路径：`a::b::C`。
        //
        // 「首段前面是 `.`」的排除项：`x.y::z` 里 `y::z` 不是一条路径，
        // 而是字段访问后接了一段路径片段——把 `y::z` 单独拎出来判，
        // 会造出源码里根本不存在的「依赖」。
        if t.kind == TokKind::Ident
            && tokens.get(i + 1).map(|n| n.kind) == Some(TokKind::PathSep)
            && tokens.get(i + 2).map(|n| n.kind) == Some(TokKind::Ident)
            && !(i > 0 && tokens.get(i - 1).map(|n| n.kind) == Some(TokKind::Dot))
        {
            let start = i;
            let mut j = i + 1;
            while tokens.get(j).map(|n| n.kind) == Some(TokKind::PathSep)
                && tokens.get(j + 1).map(|n| n.kind) == Some(TokKind::Ident)
            {
                j += 2;
            }
            let segs: Vec<String> = tokens[start..j]
                .iter()
                .filter(|x| x.kind == TokKind::Ident)
                .map(|x| x.text.clone())
                .collect();
            let path = segs.join("::");
            push_path_candidates(
                &mut out, &path, t.line, t.line, t.line, "引用", module, forbidden, allowed,
            );
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

/// 一条依赖路径的候选分发：相对路径先**解析成绝对路径**再判，其余照原样判。
///
/// # 为什么相对路径必须单独走一遍
///
/// 禁令表全是**绝对**词条（`crate::infrastructure`），而 Rust 允许用
/// `super::super::super::infrastructure::x` 表达同一条边——语义完全相同，
/// 字面第一段却是 `super`，任何绝对词条都匹配不上。那等于「换个拼写就能绕过
/// 全部层禁令」，且编译照过、单测照绿：层禁令从一道门变成一个门帘。
///
/// 故判定前先归一到绝对形态（[`relative_to_absolute`]）；报告里保留源码拼写
/// 并附解析结果，读者要能对着自己写的那一行看懂为什么它被判违规。
#[allow(clippy::too_many_arguments)]
fn push_path_candidates(
    out: &mut Vec<Candidate>,
    path: &str,
    line: usize,
    win_start: usize,
    win_end: usize,
    source: &'static str,
    module: &[String],
    forbidden: &[&'static str],
    allowed: &[&'static str],
) {
    if !is_relative_path(path) {
        push_candidates(out, path, line, win_start, win_end, source, forbidden, allowed);
        return;
    }
    // 同一行同一条禁令只报一次：多个假定深度可能都落在同一条例令上，
    // 取**最浅**的那次（最贴近源码直觉的那种写法）。
    let mut emitted: Vec<&'static str> = Vec::new();
    for depth in 0..=MAX_INLINE_MODULE_DEPTH {
        let Some(abs) = relative_to_absolute(module, path, depth) else {
            continue;
        };
        let mut staged = Vec::new();
        push_candidates(
            &mut staged,
            &abs,
            line,
            win_start,
            win_end,
            source,
            forbidden,
            allowed,
        );
        for cand in staged {
            if emitted.contains(&cand.banned) {
                continue;
            }
            emitted.push(cand.banned);
            out.push(Candidate {
                // 报告里保留源码拼写，同时给出解析结果——读者要能对着自己
                // 写的那一行看懂「为什么这条算法上算违规」。
                path: format!("{path}（解析为 {abs}）"),
                ..cand
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_candidates(
    out: &mut Vec<Candidate>,
    path: &str,
    line: usize,
    win_start: usize,
    win_end: usize,
    source: &'static str,
    forbidden: &[&'static str],
    allowed: &[&'static str],
) {
    // D-3 阶段 4a（workspace 化）：`rustmark_core::X` 与 `crate::X` 是
    // 同一条边的两种拼写——root 靠 re-export 把 core 拼回 `crate::`，
    // 二者指向同一个模块。故把 `rustmark_core::` 归一为 `crate::` 后匹配
    // **同一张**禁令表：否则把违规改写成 `rustmark_core::` 前缀即可绕过
    // 全部禁令（词法只认字面路径，认不出这两种拼写等价）。
    //
    // 两处合法拼接点天然不触发、无需豁免：
    // - `lib.rs` 是 Free 层（无禁令，候选收集直接跳过）；
    // - `presentation.rs` 的 `rustmark_core::presentation::{…}` 归一为
    //   `crate::presentation::{…}`，不在表现层禁令（app / domain /
    //   infrastructure）之内。
    //
    // 报告里 `path` 保留**源码原拼写**（读者要能对上自己写的那一行），
    // `banned` 仍是禁令表里的原词条（豁免匹配与文案都按表词条走）。
    let normalized = path
        .strip_prefix("rustmark_core::")
        .map(|rest| format!("crate::{rest}"));
    let effective = normalized.as_deref().unwrap_or(path);
    for &banned in forbidden {
        if !matches_ban(effective, banned) {
            continue;
        }
        // 命中了更窄的允许前缀 → 放行（如表现层的 `crate::app::frame`）。
        if allowed.iter().any(|a| matches_ban(effective, a)) {
            continue;
        }
        out.push(Candidate {
            path: path.to_string(),
            banned,
            line,
            win_start,
            win_end,
            source,
        });
    }
}

// 注：本测试模块须置于文件末尾——clippy `items_after_test_module` 禁止
// `#[cfg(test)]` 模块后跟生产项；此前它位于 `check_view_file_sizes` 之后、
// `scan`/`scan_project` 等生产函数之前，触发该 lint（在 `include!` 本文件的
// 集成测试目标下尤其明显）。挪到末尾即符合「测试模块在文件尾」的惯用法。
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod view_size_tests {
    use super::*;
    use std::fs;

    fn write_file(dir: &Path, rel: &str, lines: usize) {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let content: String = (0..lines).map(|i| format!("// line {i}\n")).collect();
        fs::write(&p, content).unwrap();
    }

    #[test]
    fn oversized_view_file_is_flagged() {
        let base = std::env::temp_dir().join("arch_check_d4_test");
        let _ = fs::remove_dir_all(&base);
        let views = base.join("views");
        write_file(&views, "big.rs", 601);
        write_file(&views, "small.rs", 599);
        write_file(&views, "foo/sub.rs", 601);
        write_file(&views, "tests.rs", 601);
        write_file(&views, "tests/inner.rs", 601);
        // 嵌套测试文件（真实工程形态：`preview_view/tests.rs`）。
        write_file(&views, "preview_view/tests.rs", 601);
        write_file(&views, "preview_view/tests/inner.rs", 601);
        let v = check_view_file_sizes(&views);
        let flagged: Vec<&str> = v.iter().map(|x| x.file.as_str()).collect();
        assert!(flagged.iter().any(|f| f.ends_with("big.rs")), "big.rs 应被标记: {flagged:?}");
        assert!(flagged.iter().any(|f| f.ends_with("foo/sub.rs")), "foo/sub.rs 应被标记: {flagged:?}");
        assert!(!flagged.iter().any(|f| f.ends_with("small.rs")), "small.rs 不应被标记");
        assert!(!flagged.iter().any(|f| f.ends_with("tests.rs")), "tests.rs 应豁免");
        assert!(!flagged.iter().any(|f| f.contains("tests/inner")), "tests/ 应豁免");
        assert!(!flagged.iter().any(|f| f.contains("preview_view/tests")), "嵌套 tests.rs/tests/ 应豁免");
        let _ = fs::remove_dir_all(&base);
    }
}
