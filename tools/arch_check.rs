// 架构边界校验器 —— 由 `build.rs` 与 `src/bin/check-arch.rs` 通过 `include!` 共用。
//
// # 它是怎么判的（词法级，不解析 AST，也不引入额外依赖）
//
// 1. **词法**：源码 → token 流。行注释、块注释（含嵌套）、字符串字面量、字符字面量
//    全部剔除——否则「注释里提到 `crate::domain`」「错误信息里写了 `crate::domain`」
//    会被误判成跨层依赖。
// 2. **`use` 展开**：`use` 声明按 use-tree 语法展开成**完整路径**，覆盖
//    `pub use` / `use a::{b, c}` / `use a::b as c` / `use a::*` / 跨行 / 前导 `::`。
// 3. **限定路径**：非 `use` 位置的 `a::b::C` 同样参与判定——
//    `fn f(d: crate::domain::x::Y)` 也是跨层依赖。
// 4. **匹配**：与各层禁止清单做「路径段前缀」匹配（`crate::domain` 命中
//    `crate::domain::x::Y`，但不命中 `crate::domainfoo`）。
//
// # 豁免（`arch-allow`）
//
// 行尾注释 `// arch-allow: <对象>` **只豁免该对象**（不再整行放行），
// 且必须登记在 `docs/architecture-constraints.md` 的豁免表里（校验器逐条核对）。
//
// # 帧预约唯一出口（T11 / ADR-009）
//
// `request_repaint_after` 在 `src/` 里**只允许**出现在 `app/frame/scheduler.rs`。
// 这条规则对**所有**文件生效（含组合根 `app/`）——调用点自己预约帧，
// 正是历史上「定时功能完全不生效」那三次的根因。

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

const INFRASTRUCTURE_FORBIDDEN: &[&str] =
    &["crate::app", "crate::application", "crate::presentation"];

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

const SHARED_FORBIDDEN: &[&str] = &[
    "egui",
    "eframe",
    "rfd",
    "syntect",
    "pulldown_cmark",
    "crate::app",
    "crate::domain",
    "crate::application",
    "crate::infrastructure",
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
            Layer::Shared => "shared",
            Layer::App => "app",
            Layer::Free => "free",
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
/// `main.rs` / `lib.rs` / `bin/` 属于组合根或工具，不做限制（`Layer::Free`）。
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
        "presentation" => {
            if parts.get(1).copied() == Some("state") {
                Layer::PresentationState
            } else {
                Layer::Presentation
            }
        }
        "app" => Layer::App,
        _ => Layer::Free,
    }
}

/// 取某层的禁用前缀。
///
/// **子层是叠加而非替换**：`presentation/state` 先取父层
/// [`PRESENTATION_FORBIDDEN`]，再把 [`PRESENTATION_STATE_FORBIDDEN`]
/// 接在后面。若写成只返回子层名单，`state/` 就不再受
/// `crate::infrastructure` 等父层禁令约束——那是**放宽**而非收紧，
/// 与「子层更严」的意图正好相反。
fn forbidden_for(layer: Layer) -> Vec<&'static str> {
    match layer {
        Layer::Domain => DOMAIN_FORBIDDEN.to_vec(),
        Layer::Application => APPLICATION_FORBIDDEN.to_vec(),
        Layer::Infrastructure => INFRASTRUCTURE_FORBIDDEN.to_vec(),
        Layer::Presentation => PRESENTATION_FORBIDDEN.to_vec(),
        Layer::PresentationState => {
            let mut v = PRESENTATION_FORBIDDEN.to_vec();
            v.extend_from_slice(PRESENTATION_STATE_FORBIDDEN);
            v
        }
        Layer::Shared => SHARED_FORBIDDEN.to_vec(),
        Layer::App | Layer::Free => Vec::new(),
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
fn allowed_for(layer: Layer) -> &'static [&'static str] {
    match layer {
        Layer::Presentation | Layer::PresentationState => &["crate::app::frame"],
        _ => &[],
    }
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
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Lexer {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            line: 1,
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
                    let kind = if c == '.' {
                        TokKind::Dot
                    } else {
                        TokKind::Other
                    };
                    self.push(&mut tokens, kind, String::new());
                    self.bump_char();
                }
            }
        }
        Lexed { tokens, allows }
    }

    fn push(&self, tokens: &mut Vec<Token>, kind: TokKind, text: String) {
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
        let start = self.pos + 2;
        while let Some(c) = self.peek_char() {
            if c == '\n' {
                break;
            }
            self.bump_char();
        }
        if !is_doc {
            collect_allow(&self.src[start..self.pos], self.line, allows);
        }
    }

    fn skip_block_comment(&mut self, allows: &mut Vec<Allow>) {
        let is_doc = matches!(self.bytes.get(self.pos + 2), Some(b'*') | Some(b'!'));
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
            collect_allow(&self.src[start..self.pos], start_line, allows);
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
                self.pos += 1;
                self.bump_char();
                while let Some(c) = self.peek_char() {
                    if c == '\'' {
                        self.bump_char();
                        break;
                    }
                    if c == '\\' {
                        self.bump_char();
                        self.bump_char();
                        continue;
                    }
                    self.bump_char();
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
/// 不用猜的窗口。返回形如 `1:Ident(use) 1:Ident(egui) 2:Semi(;)` 的多行文本。
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
        s.push_str(&format!("allow@{}:{}\n", a.line, a.name));
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
fn collect_allow(text: &str, line: usize, out: &mut Vec<Allow>) {
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
    /// 豁免指令可出现的行号窗口（含两端；起点前一行的豁免也认）。
    win_start: usize,
    win_end: usize,
    /// 来自 `use` 声明（true）还是其它位置的路径引用（false）。
    from_use: bool,
}

impl Candidate {
    /// 违规文案里的来源标注。
    fn kind(&self) -> &'static str {
        if self.from_use { "use" } else { "引用" }
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
    let path = manifest.join("docs").join("architecture-constraints.md");
    let Ok(text) = fs::read_to_string(&path) else {
        return Registry::default();
    };
    parse_registry(&text)
}

fn parse_registry(text: &str) -> Registry {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut in_table = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with(REGISTRY_BEGIN) {
            in_table = true;
            continue;
        }
        if t.starts_with(REGISTRY_END) {
            in_table = false;
            continue;
        }
        if !in_table || !t.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = t.trim_matches('|').split('|').map(|c| c.trim()).collect();
        if cells.len() < 2 {
            continue;
        }
        let file = cells[0].trim_matches('`').trim();
        let name = cells[1].trim_matches('`').trim();
        if file.is_empty() || name.is_empty() || file.starts_with('-') || file == "文件" {
            continue;
        }
        rows.push((normalize_registry_file(file), name.to_string()));
    }
    Registry { rows }
}

/// 登记表里的文件列允许写成 `src/domain/x.rs` 或 `domain/x.rs`，统一成后者。
fn normalize_registry_file(raw: &str) -> String {
    let s = raw.replace('\\', "/");
    let s = s.strip_prefix("./").unwrap_or(s.as_str());
    s.strip_prefix("src/").unwrap_or(s).to_string()
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

/// 扫描 `src_root` 下的全部 `.rs` 文件，返回违规列表（空表示通过）。
pub fn scan(src_root: &Path) -> Vec<Violation> {
    let registry = load_registry(src_root);
    let mut files = Vec::new();
    collect_rs_files(src_root, &mut files);
    files.sort();

    let mut violations = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(src_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let layer = layer_of(&rel);
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let lexed = Lexer::new(&content).run();
        violations.extend(check_file(&rel, layer, &lexed, &registry));
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

    let forbidden = forbidden_for(layer);
    if forbidden.is_empty() {
        // 组合根 / 自由文件：没有层级禁令 → 没有可豁免的东西。
        return violations;
    }

    // ---- 依赖路径判定 ----
    let candidates = collect_candidates(tokens, &forbidden, allowed_for(layer));
    let mut used_allow = vec![false; lexed.allows.len()];

    for cand in &candidates {
        let hit = lexed
            .allows
            .iter()
            .enumerate()
            .find(|(_, a)| {
                a.line + 1 >= cand.win_start
                    && a.line <= cand.win_end
                    && allow_covers(&a.name, cand.banned)
            })
            .map(|(idx, _)| idx);
        match hit {
            Some(idx) => {
                used_allow[idx] = true;
            }
            None => {
                let reason = if layer == Layer::PresentationState
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

    // ---- 豁免指令自身的体检 ----
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
        violations.push(Violation {
            file: rel.to_string(),
            line: a.line,
            detail: format!(
                "arch-allow 豁免无效：{}（该行没有依赖它的违规，豁免要么写错、要么已过时）",
                a.name
            ),
        });
    }

    violations
}

/// 收集全部「疑似违规」候选。
fn collect_candidates(
    tokens: &[Token],
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
                push_candidates(
                    &mut out, &path, line, t.line, stmt_end, true, forbidden, allowed,
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
            push_candidates(
                &mut out, &path, t.line, t.line, t.line, false, forbidden, allowed,
            );
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn push_candidates(
    out: &mut Vec<Candidate>,
    path: &str,
    line: usize,
    win_start: usize,
    win_end: usize,
    from_use: bool,
    forbidden: &[&'static str],
    allowed: &[&'static str],
) {
    for &banned in forbidden {
        if !matches_ban(path, banned) {
            continue;
        }
        // 命中了更窄的允许前缀 → 放行（如表现层的 `crate::app::frame`）。
        if allowed.iter().any(|a| matches_ban(path, a)) {
            continue;
        }
        out.push(Candidate {
            path: path.to_string(),
            banned,
            line,
            win_start,
            win_end,
            from_use,
        });
    }
}
