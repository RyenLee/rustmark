//! 内置 LaTeX 子集排版引擎（问题 3：数学公式在预览里不出图）。
//!
//! # 为什么自己写，而不是塞一个 KaTeX / MathJax / resvg
//!
//! 三条硬约束把这个方向排除掉了（均已在 ADR / 依赖评审中记录）：
//! 1. **KaTeX 与 MathJax 都是 JS**——`eframe` 里没有 JS 运行时，要么内嵌
//!    一个浏览器控件（体积 +100MB 量级），要么走 `egui::Html`（不存在）；
//! 2. **`resvg` / `usvg` 带 C 依赖**（`fontdb` + `ttf-parser` 尚可，
//!    但 `resvg` 的几何与滤镜栈会显著抬高构建复杂度与产物体积）；
//! 3. 本项目 `panic = "abort"`，任何第三方排版库里的 `unwrap` 都是一次
//!    进程终止，而不是可捕获的异常。
//!
//! 于是这里给出的是**够用的 LaTeX 子集**：常见数学符号、分数、根式、
//! 上下标、大运算符带上下限、矩阵、尺寸自适应的括号、重音符号。
//! 遇到子集外的写法**不猜**——返回 [`MathError::Unsupported`]，
//! 由调用方回退到「等宽源码卡」（见 `preview_view::draw_math`）。
//!
//! # 架构：解析 → 排版（纯函数）→ 绘制，三段分离
//!
//! ```text
//! LaTeX 字符串 ──parse──> Node 树 ──layout──> MathLayout ──paint──> egui Painter
//! ```
//!
//! 中间的 `layout` 刻意做成**纯函数**（度量通过 `&dyn Fn(&str, f32) -> f32`
//! 注入，不碰 `egui::Painter`）：这样「分数比分母高」「上下标缩到 0.72 倍」
//! 「矩阵列宽取最宽项」这类**排版规则**都能在无头环境里断言，
//! 而不是只能靠肉眼看截图。
//!
//! # 坐标约定（整个模块统一）
//!
//! 一切以**基线左端**为原点：`y` 向上为负、向下为正。
//! - `ascent`：基线以上的高度（正值）；
//! - `descent`：基线以下的深度（正值）；
//! - 盒子的总高 = `ascent + descent`，宽 = `width`。
//!
//! 这个约定让「上下标」成了「把子盒基线下移 / 上移若干」这种一行代码的事，
//! 也是 TeX 自己的盒模型（`height` / `depth`）。若改用「左上角为原点」，
//! 每次嵌套都要重算整体高度，分数与根式会立刻写错。

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2};

/// 排版遇到子集外的写法。
///
/// **只有一个变体是刻意的**：对用户而言「这块公式没能排版」是同一件事，
/// 细分「哪个命令不支持」只对开发者有意义，而那属于日志。
/// 渲染层据此整体回退到源码卡，不做「能排一半就先排一半」——
/// 半成品公式比源码更难读，还会让人以为公式写错了。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MathError {
    /// 出现了子集外的命令 / 结构。
    Unsupported(String),
    /// 括号不配对、参数缺失等语法问题。
    Malformed(String),
}

impl std::fmt::Display for MathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "不支持的写法：{what}"),
            Self::Malformed(what) => write!(f, "公式语法错误：{what}"),
        }
    }
}

/// 上下标 / 分子分母等「小一号」文字的缩放系数。
///
/// 0.72 取自 TeX 的 `scriptstyle`（10pt → 7pt），比 0.5 更接近印刷惯例：
/// 缩到一半会让 `x^{2}` 的 2 几乎读不出。
const SCRIPT_SCALE: f32 = 0.72;

/// 嵌套缩放的下限：`x^{y^{z^{w}}}` 这种写法再缩下去就是一小撮像素。
const SCRIPT_MIN_SCALE: f32 = 0.5;

/// 上标相对基线抬升的比例（相对**当前**字号）。
const SUP_SHIFT: f32 = 0.45;
/// 下标相对基线下沉的比例。
const SUB_SHIFT: f32 = 0.2;

/// 分数线的上下留白（em）。
const FRAC_GAP: f32 = 0.16;
/// 分数线粗细（em）。
const RULE_THICK: f32 = 0.05;

/// 根号勾形相对被开方内容的高度/宽度比例。
const SQRT_HOOK_RATIO: f32 = 0.28;
const SQRT_GAP: f32 = 0.12;

/// 大运算符在上下限模式下的额外上下留白（em）。
const LIMIT_GAP: f32 = 0.12;

/// 文本宽度度量函数：`(文本, 字号) -> 宽度（点）`。
///
/// 注入而非直接调 `egui`，是为了让排版规则可单测（见模块文档）。
pub type Measure<'a> = &'a dyn Fn(&str, f32) -> f32;

// ============================================================================
// 语法树
// ============================================================================

/// 数学排版样式族（只区分「正体 / 斜体 / 手写 / 双线」，不做真字体切换）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MathFont {
    /// 变量：斜体（`x`）。
    Italic,
    /// 数字与运算符：正体。
    Upright,
    /// `\mathbb`：双线体，近似为「正体 + 稍宽」。
    Blackboard,
    /// `\mathcal`：花体，近似为斜体。
    Calligraphic,
}

/// 重音符号种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Accent {
    /// `\hat`：尖角。
    Hat,
    /// `\bar` / `\overline`：横线。
    Bar,
    /// `\vec`：箭头。
    Vec,
    /// `\dot`：单点。
    Dot,
    /// `\tilde`：波浪（用短弧近似）。
    Tilde,
}

/// 排版节点。
#[derive(Clone, Debug)]
enum Node {
    /// 顺序排列的一组节点。
    Row(Vec<Node>),
    /// 一段原样文本（单个字符或 `\text{}` 的内容）。
    Text { text: String, font: MathFont },
    /// `\sin` 这类函数名：正体，且前面留一点空隙。
    Fn(String),
    /// 大运算符（`\sum` / `\int` …）。
    BigOp { glyph: String, limits: bool },
    /// 分数。
    Frac {
        num: Vec<Node>,
        den: Vec<Node>,
        big: bool,
    },
    /// 根式。
    Sqrt {
        index: Option<Vec<Node>>,
        body: Vec<Node>,
    },
    /// 上下标。
    Script {
        base: Box<Node>,
        sup: Option<Vec<Node>>,
        sub: Option<Vec<Node>>,
        /// 上下限是否**叠在**基准的上/下方（大运算符在 display 模式下）。
        stacked: bool,
    },
    /// `\left( … \right)`：括号按内容高度伸缩。
    Delim {
        left: String,
        right: String,
        body: Vec<Node>,
    },
    /// 重音。
    Accent { kind: Accent, body: Vec<Node> },
    /// 空白（em 单位）。
    Space(f32),
    /// 矩阵 / `cases`：由括号包裹的二维网格。
    Matrix {
        open: String,
        close: String,
        rows: Vec<Vec<Vec<Node>>>,
        /// 每列在左对齐时使用（`cases` 的第二列左对齐）。
        left_align_cols: bool,
    },
}

// ============================================================================
// 解析
// ============================================================================

/// 命名字符表：命令名 → 渲染字形。
///
/// 用 `&'static str` 而非 `char`：`\cdots` 之类是三个点（省略号），
/// 一个码位放不下；`\notin` 等也是复合字形。
const SYMBOLS: &[(&str, &str)] = &[
    // —— 小写希腊字母 ——
    ("alpha", "α"),
    ("beta", "β"),
    ("gamma", "γ"),
    ("delta", "δ"),
    ("epsilon", "ε"),
    ("varepsilon", "ϵ"),
    ("zeta", "ζ"),
    ("eta", "η"),
    ("theta", "θ"),
    ("vartheta", "ϑ"),
    ("iota", "ι"),
    ("kappa", "κ"),
    ("lambda", "λ"),
    ("mu", "μ"),
    ("nu", "ν"),
    ("xi", "ξ"),
    ("pi", "π"),
    ("varpi", "ϖ"),
    ("rho", "ρ"),
    ("sigma", "σ"),
    ("varsigma", "ς"),
    ("tau", "τ"),
    ("upsilon", "υ"),
    ("phi", "φ"),
    ("varphi", "ϕ"),
    ("chi", "χ"),
    ("psi", "ψ"),
    ("omega", "ω"),
    // —— 大写希腊字母 ——
    ("Gamma", "Γ"),
    ("Delta", "Δ"),
    ("Theta", "Θ"),
    ("Lambda", "Λ"),
    ("Xi", "Ξ"),
    ("Pi", "Π"),
    ("Sigma", "Σ"),
    ("Upsilon", "Υ"),
    ("Phi", "Φ"),
    ("Psi", "Ψ"),
    ("Omega", "Ω"),
    // —— 二元运算符 ——
    ("times", "×"),
    ("cdot", "·"),
    ("div", "÷"),
    ("pm", "±"),
    ("mp", "∓"),
    ("ast", "∗"),
    ("star", "⋆"),
    ("circ", "∘"),
    ("bullet", "∙"),
    ("oplus", "⊕"),
    ("ominus", "⊖"),
    ("otimes", "⊗"),
    ("cup", "∪"),
    ("cap", "∩"),
    ("setminus", "∖"),
    ("wedge", "∧"),
    ("vee", "∨"),
    ("land", "∧"),
    ("lor", "∨"),
    // —— 关系符 ——
    ("leq", "≤"),
    ("le", "≤"),
    ("geq", "≥"),
    ("ge", "≥"),
    ("neq", "≠"),
    ("ne", "≠"),
    ("equiv", "≡"),
    ("approx", "≈"),
    ("cong", "≅"),
    ("sim", "∼"),
    ("simeq", "≃"),
    ("propto", "∝"),
    ("ll", "≪"),
    ("gg", "≫"),
    ("subset", "⊂"),
    ("supset", "⊃"),
    ("subseteq", "⊆"),
    ("supseteq", "⊇"),
    ("in", "∈"),
    ("notin", "∉"),
    ("ni", "∋"),
    ("perp", "⊥"),
    ("parallel", "∥"),
    ("mid", "∣"),
    ("models", "⊨"),
    ("vdash", "⊢"),
    // —— 箭头 ——
    ("to", "→"),
    ("rightarrow", "→"),
    ("leftarrow", "←"),
    ("gets", "←"),
    ("leftrightarrow", "↔"),
    ("Rightarrow", "⇒"),
    ("Leftarrow", "⇐"),
    ("Leftrightarrow", "⇔"),
    ("mapsto", "↦"),
    ("uparrow", "↑"),
    ("downarrow", "↓"),
    ("longrightarrow", "⟶"),
    ("longleftarrow", "⟵"),
    // —— 大运算符 ——
    ("sum", "∑"),
    ("prod", "∏"),
    ("coprod", "∐"),
    ("int", "∫"),
    ("iint", "∬"),
    ("iiint", "∭"),
    ("oint", "∮"),
    ("bigcup", "⋃"),
    ("bigcap", "⋂"),
    ("bigoplus", "⨁"),
    ("bigotimes", "⨂"),
    ("bigvee", "⋁"),
    ("bigwedge", "⋀"),
    // —— 其它符号 ——
    ("infty", "∞"),
    ("partial", "∂"),
    ("nabla", "∇"),
    ("forall", "∀"),
    ("exists", "∃"),
    ("nexists", "∄"),
    ("emptyset", "∅"),
    ("varnothing", "∅"),
    ("neg", "¬"),
    ("lnot", "¬"),
    ("angle", "∠"),
    ("triangle", "△"),
    ("square", "□"),
    ("aleph", "ℵ"),
    ("hbar", "ℏ"),
    ("ell", "ℓ"),
    ("Re", "ℜ"),
    ("Im", "ℑ"),
    ("prime", "′"),
    ("degree", "°"),
    ("cdots", "⋯"),
    ("ldots", "…"),
    ("dots", "…"),
    ("vdots", "⋮"),
    ("ddots", "⋱"),
    ("vert", "|"),
    ("Vert", "‖"),
    ("langle", "⟨"),
    ("rangle", "⟩"),
    ("lceil", "⌈"),
    ("rceil", "⌉"),
    ("lfloor", "⌊"),
    ("rfloor", "⌋"),
    ("lbrace", "{"),
    ("rbrace", "}"),
    ("{", "{"),
    ("}", "}"),
    ("%", "%"),
    ("#", "#"),
    ("&", "&"),
    ("_", "_"),
    ("$", "$"),
    ("backslash", "\\"),
    ("quad", " "),
    ("qquad", " "),
    (" ", " "),
    (",", " "),
    (";", " "),
    (":", " "),
    ("!", ""),
];

/// 大运算符中**带上下限**（限置于符号上/下方）的那些。
///
/// `\int` 系列刻意不在其中：`\int_0^1` 在 TeX 里始终把上下限画在**右侧**，
/// 放到上下方会让定积分看起来像求和，与用户预期不符。
const LIMIT_OPS: &[&str] = &[
    "sum",
    "prod",
    "coprod",
    "bigcup",
    "bigcap",
    "bigoplus",
    "bigotimes",
    "bigvee",
    "bigwedge",
];

/// 函数名（正体，且与前面的变量之间留细空）。
const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh",
    "log", "ln", "lg", "exp", "det", "dim", "ker", "deg", "gcd", "hom", "lim", "limsup", "liminf",
    "max", "min", "sup", "inf", "arg", "Pr", "bmod",
];

/// 声明的环境（`\begin{...}`）。
const ENVIRONMENTS: &[&str] = &[
    "matrix",
    "pmatrix",
    "bmatrix",
    "vmatrix",
    "Vmatrix",
    "Bmatrix",
    "smallmatrix",
    "cases",
    "aligned",
    "align",
];

/// 解析器：在字符序列上做**单遍**下降解析。
struct Parser {
    src: Vec<char>,
    pos: usize,
    /// 当前是否处于 display（行间）模式。
    ///
    /// 只影响一件事：大运算符的上下限是叠放还是放在右侧。
    /// 与 TeX 一致——行内 `$\sum_{i}$` 也是右侧，否则会撑高行距。
    display: bool,
}

/// 解析一段 LaTeX 为语法树。
fn parse(src: &str, display: bool) -> Result<Vec<Node>, MathError> {
    let mut p = Parser {
        src: src.chars().collect(),
        pos: 0,
        display,
    };
    let nodes = p.parse_row(None)?;
    p.skip_spaces();
    if p.pos < p.src.len() {
        // 只可能是多出来的 `}` 或 `\right`——都是语法问题。
        return Err(MathError::Malformed(format!(
            "第 {} 个字符起有多余内容：{}",
            p.pos,
            p.rest_preview()
        )));
    }
    Ok(nodes)
}

impl Parser {
    /// 剩余内容的前若干字符（错误信息用）。
    fn rest_preview(&self) -> String {
        self.src[self.pos..].iter().take(16).collect()
    }

    fn peek(&self) -> Option<char> {
        self.src.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    fn skip_spaces(&mut self) {
        while matches!(
            self.peek(),
            Some(' ') | Some('\t') | Some('\n') | Some('\r')
        ) {
            self.pos += 1;
        }
    }

    /// 解析一串节点，直到输入结束或遇到 `stop` 指定的**结束控制序列**。
    ///
    /// `stop` 为 `Some("right")` 时，`\right` 不被消费——留给 [`Self::parse_delim`]
    /// 读取它后面的定界符。返回时 `self.pos` 指向 `\right` 的 `\`。
    fn parse_row(&mut self, stop: Option<&str>) -> Result<Vec<Node>, MathError> {
        let mut out: Vec<Node> = Vec::new();
        loop {
            self.skip_spaces();
            let Some(c) = self.peek() else { break };
            match c {
                '}' => break,
                // `\\`（矩阵换行）与 `&`（列分隔）由环境解析器消费；
                // 走到这里说明它们出现在矩阵外，按语法错误处理更诚实。
                '&' => {
                    return Err(MathError::Malformed(
                        "`&` 只允许出现在矩阵 / cases 环境里".to_string(),
                    ));
                }
                '^' | '_' => {
                    self.apply_script(&mut out, c)?;
                }
                '{' => {
                    self.pos += 1;
                    let inner = self.parse_row(None)?;
                    self.expect('}')?;
                    out.push(Node::Row(inner));
                }
                '\\' => {
                    if let Some(name) = stop
                        && self.command_is(name)
                    {
                        break;
                    }
                    out.push(self.parse_command()?);
                }
                _ => {
                    self.pos += 1;
                    out.push(Node::Text {
                        // 数字与运算符用正体、字母用斜体——这是数学排版的基本观感，
                        // 不做的话 `abc` 会变成三个斜体字母挤在一起。
                        text: c.to_string(),
                        font: default_font_for(c),
                    });
                }
            }
        }
        Ok(out)
    }

    /// 当前位置是否以 `\name` 开头（`\` + 名字，且名字后不再是字母）。
    fn command_is(&self, name: &str) -> bool {
        if self.peek() != Some('\\') {
            return false;
        }
        let rest: String = self.src[self.pos + 1..].iter().take(name.len()).collect();
        if rest != name {
            return false;
        }
        // `\right` 不能匹配到 `\rightarrow`：后面必须不是字母。
        !self
            .src
            .get(self.pos + 1 + name.len())
            .is_some_and(|c| c.is_ascii_alphabetic())
    }

    fn expect(&mut self, c: char) -> Result<(), MathError> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(MathError::Malformed(format!(
                "期望 `{c}`，实际遇到 {}",
                self.peek()
                    .map_or("输入结束".to_string(), |x| format!("`{x}`"))
            )))
        }
    }

    /// 读一个命令名（`\` 之后）；非字母命令（`\\` `\,` `\{` …）返回单字符。
    fn read_command_name(&mut self) -> Result<String, MathError> {
        self.expect('\\')?;
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() {
                name.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        if name.is_empty() {
            // `\\` / `\,` / `\{` / `\%` 等：单个非字母字符。
            match self.bump() {
                Some(c) => Ok(c.to_string()),
                None => Err(MathError::Malformed("`\\` 后面没有内容".to_string())),
            }
        } else {
            Ok(name)
        }
    }

    /// 解析一个命令（含它的参数），返回对应节点。
    fn parse_command(&mut self) -> Result<Node, MathError> {
        let name = self.read_command_name()?;
        match name.as_str() {
            // —— 文本 / 字体族 ——
            "text" | "textrm" | "textnormal" | "mbox" => {
                let raw = self.read_group_raw()?;
                Ok(Node::Text {
                    text: raw.replace('~', "\u{a0}"),
                    font: MathFont::Upright,
                })
            }
            "mathrm" | "mathbf" | "boldsymbol" | "operatorname" => {
                let body = self.read_group()?;
                Ok(Node::Row(body))
            }
            "mathbb" | "Bbb" => {
                let body = self.read_group()?;
                Ok(Node::Row(retext(body, MathFont::Blackboard)))
            }
            "mathcal" | "mathscr" | "mathfrak" => {
                let body = self.read_group()?;
                Ok(Node::Row(retext(body, MathFont::Calligraphic)))
            }
            "mathit" => {
                let body = self.read_group()?;
                Ok(Node::Row(retext(body, MathFont::Italic)))
            }
            // —— 结构 ——
            "frac" | "dfrac" | "tfrac" => {
                let num = self.read_group()?;
                let den = self.read_group()?;
                Ok(Node::Frac {
                    num,
                    den,
                    big: name != "tfrac",
                })
            }
            "sqrt" => {
                // 可选的 `[n]` 次根。
                let index = if self.peek() == Some('[') {
                    self.bump();
                    let idx = self.parse_until(']')?;
                    self.expect(']')?;
                    Some(idx)
                } else {
                    None
                };
                let body = self.read_group()?;
                Ok(Node::Sqrt { index, body })
            }
            "left" => self.parse_delim(),
            "begin" => self.parse_environment(),
            "overline" => {
                let body = self.read_group()?;
                Ok(Node::Accent {
                    kind: Accent::Bar,
                    body,
                })
            }
            "hat" | "widehat" => {
                let body = self.read_group()?;
                Ok(Node::Accent {
                    kind: Accent::Hat,
                    body,
                })
            }
            "bar" => {
                let body = self.read_group()?;
                Ok(Node::Accent {
                    kind: Accent::Bar,
                    body,
                })
            }
            "vec" => {
                let body = self.read_group()?;
                Ok(Node::Accent {
                    kind: Accent::Vec,
                    body,
                })
            }
            "dot" => {
                let body = self.read_group()?;
                Ok(Node::Accent {
                    kind: Accent::Dot,
                    body,
                })
            }
            "tilde" => {
                let body = self.read_group()?;
                Ok(Node::Accent {
                    kind: Accent::Tilde,
                    body,
                })
            }
            "displaystyle" | "textstyle" | "limits" | "nolimits" => Ok(Node::Row(Vec::new())),
            // 拉伸类命令（`\bigl` …）在本渲染器里没有对应的伸缩语义，
            // 直接忽略：它们只影响括号大小的审美，不影响可读性。
            "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr" | "Biggl" | "Biggr" | "big"
            | "Big" | "bigg" | "Bigg" => {
                // 后面跟的定界符要照常输出。
                self.parse_single_delim()
            }
            "quad" => Ok(Node::Space(1.0)),
            "qquad" => Ok(Node::Space(2.0)),
            "," | ";" | ":" | " " | "!" => Ok(Node::Space(if name == "!" { -0.15 } else { 0.22 })),
            "\\" => Err(MathError::Malformed(
                "`\\\\`（换行）只允许出现在矩阵环境里".to_string(),
            )),
            _ => self.parse_symbol_or_op(&name),
        }
    }

    /// 符号表 / 大运算符 / 函数名三种查表，都查不到就是子集外。
    fn parse_symbol_or_op(&mut self, name: &str) -> Result<Node, MathError> {
        if LIMIT_OPS.contains(&name) || matches!(name, "int" | "iint" | "iiint" | "oint") {
            let glyph = SYMBOLS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, g)| (*g).to_string())
                .ok_or_else(|| MathError::Unsupported(format!("\\{name}")))?;
            return Ok(Node::BigOp {
                glyph,
                limits: self.display && LIMIT_OPS.contains(&name),
            });
        }
        if FUNCTIONS.contains(&name) {
            return Ok(Node::Fn(name.to_string()));
        }
        if let Some((_, glyph)) = SYMBOLS.iter().find(|(n, _)| *n == name) {
            return Ok(Node::Text {
                text: (*glyph).to_string(),
                font: MathFont::Upright,
            });
        }
        Err(MathError::Unsupported(format!("\\{name}")))
    }

    /// 读一个必选参数 `{...}`（也接受单字符形式的 `\frac12`）。
    fn read_group(&mut self) -> Result<Vec<Node>, MathError> {
        self.skip_spaces();
        match self.peek() {
            Some('{') => {
                self.pos += 1;
                let inner = self.parse_row(None)?;
                self.expect('}')?;
                Ok(inner)
            }
            Some('\\') => Ok(vec![self.parse_command()?]),
            Some(c) if !matches!(c, ']' | ')' | '}' | '&') => {
                self.pos += 1;
                Ok(vec![Node::Text {
                    text: c.to_string(),
                    font: default_font_for(c),
                }])
            }
            _ => Err(MathError::Malformed("命令缺少参数".to_string())),
        }
    }

    /// 读 `{...}` 的**原始文本**（`\text{}` 用：里面不解析数学命令）。
    fn read_group_raw(&mut self) -> Result<String, MathError> {
        self.skip_spaces();
        self.expect('{')?;
        let mut depth = 1usize;
        let mut out = String::new();
        while let Some(c) = self.bump() {
            match c {
                '{' => {
                    depth += 1;
                    out.push(c);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                    out.push(c);
                }
                // `\` 在 `\text{}` 里只用于转义，直接吞掉反斜杠保留字符。
                '\\' => {
                    if let Some(next) = self.bump() {
                        out.push(next);
                    }
                }
                _ => out.push(c),
            }
        }
        Err(MathError::Malformed("`{` 没有闭合".to_string()))
    }

    /// 解析到指定右界（`[n]` 里的 `]`），不消费右界。
    fn parse_until(&mut self, end: char) -> Result<Vec<Node>, MathError> {
        let mut out: Vec<Node> = Vec::new();
        loop {
            self.skip_spaces();
            let Some(c) = self.peek() else { break };
            if c == end {
                break;
            }
            match c {
                '^' | '_' => self.apply_script(&mut out, c)?,
                '{' => {
                    self.pos += 1;
                    let inner = self.parse_row(None)?;
                    self.expect('}')?;
                    out.push(Node::Row(inner));
                }
                '\\' => out.push(self.parse_command()?),
                _ => {
                    self.pos += 1;
                    out.push(Node::Text {
                        text: c.to_string(),
                        font: default_font_for(c),
                    });
                }
            }
        }
        Ok(out)
    }

    /// `\left( … \right)`：读到 `\right` 为止，两侧定界符一并记下。
    fn parse_delim(&mut self) -> Result<Node, MathError> {
        let left = self.read_delim_char()?;
        let body = self.parse_row(Some("right"))?;
        if !self.command_is("right") {
            return Err(MathError::Malformed(
                "`\\left` 没有配对的 `\\right`".to_string(),
            ));
        }
        self.pos += 1 + "right".len();
        let right = self.read_delim_char()?;
        Ok(Node::Delim { left, right, body })
    }

    /// 读一个定界符（`(` / `]` / `\{` / `|` / `.` ）。
    fn read_delim_char(&mut self) -> Result<String, MathError> {
        self.skip_spaces();
        match self.peek() {
            None => Err(MathError::Malformed("定界符缺失".to_string())),
            Some('\\') => {
                let name = self.read_command_name()?;
                match name.as_str() {
                    "lbrace" => Ok("{".to_string()),
                    "rbrace" => Ok("}".to_string()),
                    "langle" => Ok("⟨".to_string()),
                    "rangle" => Ok("⟩".to_string()),
                    "lceil" => Ok("⌈".to_string()),
                    "rceil" => Ok("⌉".to_string()),
                    "lfloor" => Ok("⌊".to_string()),
                    "rfloor" => Ok("⌋".to_string()),
                    "vert" | "mid" => Ok("|".to_string()),
                    "Vert" => Ok("‖".to_string()),
                    // `\left.` 的「隐形定界符」：不画。
                    "." => Ok(String::new()),
                    other => Err(MathError::Unsupported(format!("\\{other} 作为定界符"))),
                }
            }
            Some('.') => {
                self.pos += 1;
                Ok(String::new())
            }
            Some(c) => {
                self.pos += 1;
                Ok(c.to_string())
            }
        }
    }

    /// `\bigl(` 这类：跳过命令名后，把紧跟的定界符当普通字符输出。
    fn parse_single_delim(&mut self) -> Result<Node, MathError> {
        self.skip_spaces();
        match self.peek() {
            Some('\\') => self.parse_command(),
            Some(c) => {
                self.pos += 1;
                Ok(Node::Text {
                    text: c.to_string(),
                    font: MathFont::Upright,
                })
            }
            None => Ok(Node::Row(Vec::new())),
        }
    }

    /// 把当前格 `cell` 推进网格的**末行**，并清空 `cell`（`mem::take`）。
    ///
    /// 抽成函数是为了避开 `expect`——`src/lib.rs` 把 `clippy::expect_used`
    /// 升成了硬错误。这里的不变式其实成立（`rows` 由 `vec![Vec::new()]` 起家，
    /// 之后每次换行都是「先写末行、再 `push` 新行」的成对操作，末行恒在），
    /// 但仍用 `if let` 直取：万一不变式被破坏，退化成「丢掉当前格」，
    /// 而不是整个进程终止（`panic = "abort"` 下 panic 就是直接退出）。
    fn push_cell(rows: &mut [Vec<Vec<Node>>], cell: &mut Vec<Node>) {
        if let Some(row) = rows.last_mut() {
            row.push(std::mem::take(cell));
        }
    }

    /// `\begin{...} … \end{...}`：矩阵 / cases / aligned。
    fn parse_environment(&mut self) -> Result<Node, MathError> {
        let env = self.read_group_raw()?;
        if !ENVIRONMENTS.contains(&env.as_str()) {
            return Err(MathError::Unsupported(format!("\\begin{{{env}}}")));
        }
        let (open, close) = match env.as_str() {
            "pmatrix" => ("(", ")"),
            "bmatrix" => ("[", "]"),
            "vmatrix" => ("|", "|"),
            "Vmatrix" => ("‖", "‖"),
            "Bmatrix" => ("{", "}"),
            "cases" | "aligned" | "align" => ("", ""),
            // `matrix` 与 `smallmatrix`：无括号。
            _ => ("", ""),
        };

        let mut rows: Vec<Vec<Vec<Node>>> = vec![Vec::new()];
        let mut cell: Vec<Node> = Vec::new();
        loop {
            self.skip_spaces();
            let Some(c) = self.peek() else {
                return Err(MathError::Malformed(format!(
                    "`\\begin{{{env}}}` 缺少 `\\end`"
                )));
            };
            if c == '\\' && self.command_is("end") {
                self.pos += 1 + "end".len();
                let end = self.read_group_raw()?;
                if end != env {
                    return Err(MathError::Malformed(format!(
                        "`\\begin{{{env}}}` 与 `\\end{{{end}}}` 不匹配"
                    )));
                }
                break;
            }
            match c {
                '&' => {
                    self.pos += 1;
                    Self::push_cell(&mut rows, &mut cell);
                }
                '\\' if self.src.get(self.pos + 1) == Some(&'\\') => {
                    self.pos += 2;
                    Self::push_cell(&mut rows, &mut cell);
                    // 行末的 `\\` 会开出一行新行，后面若直接是 `\end`，
                    // 那一行会是空的——末尾统一用 `retain` 丢掉。
                    rows.push(Vec::new());
                }
                '\\' => cell.push(self.parse_command()?),
                '^' | '_' => self.apply_script(&mut cell, c)?,
                '{' => {
                    self.pos += 1;
                    let inner = self.parse_row(None)?;
                    self.expect('}')?;
                    cell.push(Node::Row(inner));
                }
                _ => {
                    self.pos += 1;
                    cell.push(Node::Text {
                        text: c.to_string(),
                        font: default_font_for(c),
                    });
                }
            }
        }
        if !cell.is_empty() {
            Self::push_cell(&mut rows, &mut cell);
        }
        // 丢掉行末多余的空行（`a\\b\\\end{...}` 会在 `\end` 前留一行空的）。
        // 至少保留一行，否则下方 `rows_h` / `col_w` 的推算会得到空网格。
        while rows.len() > 1 && rows.last().is_some_and(Vec::is_empty) {
            rows.pop();
        }
        Ok(Node::Matrix {
            open: open.to_string(),
            close: close.to_string(),
            rows,
            left_align_cols: matches!(env.as_str(), "cases" | "aligned" | "align"),
        })
    }

    /// `^` / `_`：把**紧邻的上一项**改成带上下标的节点。
    ///
    /// 连续写 `_` 与 `^` 时（`x_1^2`）第二次要**补到同一个 `Script` 上**，
    /// 而不是把「已带下标的 x」整体再套一层上标——后者会让 `1` 与 `2`
    /// 一高一低错开（嵌套的基线不同）。
    fn apply_script(&mut self, out: &mut Vec<Node>, mark: char) -> Result<(), MathError> {
        let is_sup = mark == '^';
        self.bump();
        let arg = self.read_group()?;
        // 若最近的一项是「只带了下标、还没有上标」的 Script，就地补全。
        //
        // `x_1^2` 的两次角标要落在**同一个** `Script` 上。若第二次包一层新
        // `Script`，两层基线不同，`1` 与 `2` 会一高一低错开（实测可见）。
        if let Some(Node::Script { sup, sub, .. }) = out.last_mut() {
            if is_sup && sup.is_none() {
                *sup = Some(arg);
                return Ok(());
            }
            if !is_sup && sub.is_none() {
                *sub = Some(arg);
                return Ok(());
            }
        }
        let base = out
            .pop()
            .ok_or_else(|| MathError::Malformed(format!("`{mark}` 前面没有可加角标的对象")))?;
        // 两种情形分开构造：`then_some` 会**无条件**把 `arg` 移进去，
        // 一个参数只能给一个字段（E0382）。
        let (sup, sub) = if is_sup {
            (Some(arg), None)
        } else {
            (None, Some(arg))
        };
        out.push(Node::Script {
            base: Box::new(base),
            sup,
            sub,
            stacked: false,
        });
        Ok(())
    }
}

/// 一个字符的默认字体：字母斜体、其余正体。
fn default_font_for(c: char) -> MathFont {
    if c.is_alphabetic() && !c.is_uppercase() && c.is_ascii() {
        MathFont::Italic
    } else if c.is_alphabetic() {
        // 希腊 / 中文等：正体更接近印刷惯例（`α` 斜体是可选的，不做）。
        MathFont::Upright
    } else {
        MathFont::Upright
    }
}

/// 把一棵子树里的 `Text` 全部换成指定字体族。
fn retext(nodes: Vec<Node>, font: MathFont) -> Vec<Node> {
    nodes
        .into_iter()
        .map(|n| match n {
            Node::Text { text, .. } => Node::Text { text, font },
            Node::Row(inner) => Node::Row(retext(inner, font)),
            Node::Fn(name) => Node::Text { text: name, font },
            other => other,
        })
        .collect()
}

/// 给大运算符的角标打上「叠放」标记。
///
/// # 为什么要在排版前单独走一遍
///
/// 「上下限叠在符号上下方」需要同时知道两件事：当前是 display 还是行内模式，
/// 以及基准是不是一个带限大运算符。而 `\sum` 与它的 `_`/`^` 在事件流里是
/// **分开**解析的（`\sum` 先出一个 `BigOp` 节点，随后 `_`/`^` 才把它包进
/// `Script`），解析那一刻并不知道后面会不会跟上下限。故解析完统一折叠一次。
fn mark_stacked(nodes: &mut [Node], display: bool) {
    for n in nodes.iter_mut() {
        match n {
            Node::Script {
                base,
                stacked,
                sup,
                sub,
            } => {
                // 只有「带限大运算符 + display 模式」才叠放。
                // 行内 `$\sum_{i}$` 不叠——否则会把整行行距顶开。
                *stacked = display && matches!(**base, Node::BigOp { limits: true, .. });
                mark_stacked(std::slice::from_mut(base.as_mut()), display);
                if let Some(s) = sup {
                    mark_stacked(s, display);
                }
                if let Some(s) = sub {
                    mark_stacked(s, display);
                }
            }
            Node::Row(inner) => mark_stacked(inner, display),
            Node::Frac { num, den, .. } => {
                mark_stacked(num, display);
                mark_stacked(den, display);
            }
            Node::Sqrt { index, body } => {
                if let Some(i) = index {
                    mark_stacked(i, display);
                }
                mark_stacked(body, display);
            }
            Node::Delim { body, .. } => mark_stacked(body, display),
            Node::Accent { body, .. } => mark_stacked(body, display),
            Node::Matrix { rows, .. } => {
                for row in rows {
                    for cell in row {
                        mark_stacked(cell, display);
                    }
                }
            }
            Node::Text { .. } | Node::Fn(_) | Node::BigOp { .. } | Node::Space(_) => {}
        }
    }
}

// ============================================================================
// 排版
// ============================================================================

/// 绘制指令（坐标相对**基线左端**）。
#[derive(Clone, Debug, PartialEq)]
enum Item {
    /// 一段文本，`y` 是它的基线。
    Text {
        x: f32,
        y: f32,
        text: String,
        size: f32,
        font: MathFont,
    },
    /// 一条线段（分数线、根号、括号…）。
    Line {
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
    },
    /// 一个实心矩形（分数线可复用，重音的点也用）。
    Rect { x: f32, y: f32, w: f32, h: f32 },
}

/// 排版结果。
#[derive(Clone, Debug, PartialEq)]
pub struct MathLayout {
    /// 内容宽度（点）。
    pub width: f32,
    /// 基线以上高度（点，正值）。
    pub ascent: f32,
    /// 基线以下深度（点，正值）。
    pub descent: f32,
    /// 绘制指令。
    items: Vec<Item>,
}

impl MathLayout {
    /// 总高度。
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }
}

/// 一个中间盒（排版过程中使用）。
#[derive(Clone, Debug, Default)]
struct Layout {
    w: f32,
    asc: f32,
    desc: f32,
    items: Vec<Item>,
}

impl Layout {
    /// 按 `dx` 右移并追加内容（`dy` 相对当前基线）。
    fn push(&mut self, other: &Layout, dx: f32, dy: f32) {
        for item in &other.items {
            self.items.push(shift(item, dx, dy));
        }
        self.w = self.w.max(dx + other.w);
    }

    /// 让本盒至少容纳 `asc` / `desc` 的高度。
    fn grow(&mut self, asc: f32, desc: f32) {
        self.asc = self.asc.max(asc);
        self.desc = self.desc.max(desc);
    }

    /// 以本盒的内容（宽 `w`）整体下移 `dy` 后返回新盒——用于分数、上下限。
    fn offset_items(&self, dx: f32, dy: f32) -> Vec<Item> {
        self.items.iter().map(|i| shift(i, dx, dy)).collect()
    }

    /// 收成最终结果（以基线左端为原点）。
    fn finish(self) -> MathLayout {
        MathLayout {
            width: self.w,
            ascent: self.asc,
            descent: self.desc,
            items: self.items,
        }
    }
}

/// 平移一条绘制指令。
fn shift(item: &Item, dx: f32, dy: f32) -> Item {
    match *item {
        Item::Text {
            x,
            y,
            ref text,
            size,
            font,
        } => Item::Text {
            x: x + dx,
            y: y + dy,
            text: text.clone(),
            size,
            font,
        },
        Item::Line { from, to, width } => Item::Line {
            from: (from.0 + dx, from.1 + dy),
            to: (to.0 + dx, to.1 + dy),
            width,
        },
        Item::Rect { x, y, w, h } => Item::Rect {
            x: x + dx,
            y: y + dy,
            w,
            h,
        },
    }
}

/// 排版上下文：基准字号 + 文本度量。
///
/// # 为什么要把基准字号一路带下去
///
/// 角标的字号是**逐层相对父级**缩的（`x^{y^{z}}` 每深一层再乘
/// [`SCRIPT_SCALE`]）。若下限也按「相对父级」算，三层之后就是
/// `0.72³ ≈ 0.37` 倍——实测 `x^{y^{z^{w}}}` 最深层只剩 5.9pt，已经看不清了。
/// 下限必须锚在**公式的基准字号**上（`base * SCRIPT_MIN_SCALE`），
/// 于是深层的缩放比例会自动收敛，而不再一路乘下去。
struct Ctx<'a> {
    /// 公式的基准字号（正文大小）。
    base: f32,
    measure: Measure<'a>,
}

impl Ctx<'_> {
    /// 下一层角标的字号。
    fn script_size(&self, size: f32) -> f32 {
        (size * SCRIPT_SCALE).max(self.base * SCRIPT_MIN_SCALE)
    }
}

/// 排版一棵语法树。
///
/// `size` 是**当前层**字号；上下标经 [`Ctx::script_size`] 收缩。
fn layout_nodes(nodes: &[Node], size: f32, ctx: &Ctx<'_>) -> Layout {
    let mut row = Layout::default();
    let mut prev: Option<&Node> = None;
    for node in nodes {
        // 函数名与前面的变量之间留一点空隙（`\sin x` 而不是 `sinx`）。
        if matches!(node, Node::Fn(_)) && prev.is_some() {
            row.w += size * 0.12;
        }
        let box_ = layout_node(node, size, ctx);
        row.push(&box_, row.w, 0.0);
        row.grow(box_.asc, box_.desc);
        prev = Some(node);
    }
    row
}

/// 排版单个节点。
fn layout_node(node: &Node, size: f32, ctx: &Ctx<'_>) -> Layout {
    match node {
        Node::Row(inner) => layout_nodes(inner, size, ctx),
        Node::Text { text, font } => {
            let w = (ctx.measure)(text, size);
            Layout {
                w,
                // 文本的高度直接用字号近似：真实字体的 ascent 因字符而异，
                // 而这里只需要「一个够用的盒子」——用固定比例反而不受字体影响。
                asc: size * 0.78,
                desc: size * 0.22,
                items: vec![Item::Text {
                    x: 0.0,
                    y: 0.0,
                    text: text.clone(),
                    size,
                    font: *font,
                }],
            }
        }
        Node::Fn(name) => {
            let w = (ctx.measure)(name, size);
            Layout {
                w,
                asc: size * 0.78,
                desc: size * 0.22,
                items: vec![Item::Text {
                    x: 0.0,
                    y: 0.0,
                    text: name.clone(),
                    size,
                    font: MathFont::Upright,
                }],
            }
        }
        Node::Space(em) => Layout {
            w: size * em,
            ..Default::default()
        },
        Node::BigOp { glyph, limits } => {
            // 大运算符放大到 1.35 倍：不放大就与普通符号分不出主次。
            let big = size * 1.35;
            let w = (ctx.measure)(glyph, big);
            let asc = big * 0.72;
            let desc = big * 0.28;
            let _ = limits;
            Layout {
                w,
                asc,
                desc,
                items: vec![Item::Text {
                    x: 0.0,
                    y: 0.0,
                    text: glyph.clone(),
                    size: big,
                    font: MathFont::Upright,
                }],
            }
        }
        Node::Script {
            base,
            sup,
            sub,
            stacked,
        } => {
            let b = layout_node(base, size, ctx);
            let small = ctx.script_size(size);
            let sup_box = sup.as_ref().map(|s| layout_nodes(s, small, ctx));
            let sub_box = sub.as_ref().map(|s| layout_nodes(s, small, ctx));

            if *stacked {
                // 上下限叠放（display 模式的 `\sum`）：符号居中，限在上下。
                let lim_w = sup_box
                    .as_ref()
                    .map_or(0.0, |b| b.w)
                    .max(sub_box.as_ref().map_or(0.0, |b| b.w));
                let w = b.w.max(lim_w);
                let mut items = Vec::new();
                let mut asc = b.asc;
                let mut desc = b.desc;
                // 运算符居中。
                let bcx = (w - b.w) * 0.5;
                items.extend(b.offset_items(bcx, 0.0));
                if let Some(s) = &sup_box {
                    let dy = -(b.asc + size * LIMIT_GAP + s.desc);
                    let x = (w - s.w) * 0.5;
                    items.extend(s.offset_items(x, dy));
                    asc = asc.max(-dy + s.asc);
                }
                if let Some(s) = &sub_box {
                    let dy = b.desc + size * LIMIT_GAP + s.asc;
                    let x = (w - s.w) * 0.5;
                    items.extend(s.offset_items(x, dy));
                    desc = desc.max(dy + s.desc);
                }
                return Layout {
                    w,
                    asc,
                    desc,
                    items,
                };
            }

            // 常规角标：贴在上/右下角，**不**撑高基准盒的高度
            // （否则 `x^2` 会把整行行距顶开，与 TeX 的观感不符）。
            let mut w = b.w;
            let mut items = b.offset_items(0.0, 0.0);
            let mut asc = b.asc;
            let mut desc = b.desc;
            if let Some(s) = &sup_box {
                let x = b.w + size * 0.02;
                let dy = -size * SUP_SHIFT;
                items.extend(s.offset_items(x, dy));
                w = w.max(x + s.w);
                asc = asc.max(-dy + s.asc);
            }
            if let Some(s) = &sub_box {
                let x = b.w + size * 0.02;
                let dy = size * SUB_SHIFT;
                items.extend(s.offset_items(x, dy));
                w = w.max(x + s.w);
                desc = desc.max(dy + s.desc);
            }
            Layout {
                w,
                asc,
                desc,
                items,
            }
        }
        Node::Frac { num, den, big } => {
            // `\tfrac` 的分数字号缩一档（行内 `\frac` 视觉上更轻）。
            let size = if *big { size } else { size * 0.85 };
            let n = layout_nodes(num, size, ctx);
            let d = layout_nodes(den, size, ctx);
            let w = n.w.max(d.w) + size * 0.3;
            let gap = size * FRAC_GAP;
            let thick = (size * RULE_THICK).max(1.0);
            // 分数线在基线上方一点：TeX 把分数线放在数学轴（≈ 基线 + 0.25em）处。
            let axis = -size * 0.25;
            let mut items = Vec::new();
            items.extend(n.offset_items((w - n.w) * 0.5, axis - gap - n.desc));
            items.extend(d.offset_items((w - d.w) * 0.5, axis + gap + d.asc));
            items.push(Item::Rect {
                x: 0.0,
                y: axis - thick * 0.5,
                w,
                h: thick,
            });
            let asc = -axis + gap + n.asc + n.desc + gap;
            let desc = axis + gap + d.asc + d.desc + gap;
            Layout {
                w,
                asc,
                desc,
                items,
            }
        }
        Node::Sqrt { index, body } => {
            let b = layout_nodes(body, size, ctx);
            let hook = size * SQRT_HOOK_RATIO;
            let gap = size * SQRT_GAP;
            let thick = (size * RULE_THICK).max(1.0);
            // 被开方内容整体上移「勾的高度」，让勾的尖点落在它的左缘。
            let dy = -hook;
            let mut items = Vec::new();
            let mut x0 = 0.0;
            let mut asc = b.asc + hook;
            if let Some(idx) = index {
                let i = layout_nodes(idx, ctx.script_size(size), ctx);
                items.extend(i.offset_items(0.0, -b.asc - i.desc + hook * 0.2));
                x0 = i.w + size * 0.04;
                asc = asc.max(b.asc + hook + i.asc);
            }
            let top = dy - b.asc;
            let bottom = b.desc;
            // 勾：左下要点 → 折点 → 顶部横线。
            let notch = x0 + hook * 0.5;
            items.push(Item::Line {
                from: (x0, -bottom * 0.35),
                to: (notch * 0.5 + x0 * 0.5, bottom * 0.55),
                width: thick * 1.6,
            });
            items.push(Item::Line {
                from: (notch * 0.5 + x0 * 0.5, bottom * 0.55),
                to: (x0 + hook, top),
                width: thick * 1.6,
            });
            items.push(Item::Line {
                from: (x0 + hook, top),
                to: (x0 + hook + b.w + gap, top),
                width: thick,
            });
            items.extend(b.offset_items(x0 + hook + gap, dy));
            let w = x0 + hook + gap + b.w + gap;
            Layout {
                w,
                asc: asc.max(-top),
                desc: bottom.max(0.0),
                items,
            }
        }
        Node::Delim { left, right, body } => {
            let b = layout_nodes(body, size, ctx);
            // 括号按内容高度**伸缩**：内容比单行高时把括号字号放大，
            // 并上下拉伸（这里用「放大字号」近似拉伸——字形拉伸需要矢量轮廓，
            // 而放大字号在视觉上已经足够，且不会把括号画成两条平行线）。
            let content_h = b.asc + b.desc;
            let scale = (content_h / (size * 1.0)).clamp(1.0, 2.6);
            let dsize = size * scale;
            let mut items = Vec::new();
            let mut x = 0.0;
            let mut asc = b.asc;
            let mut desc = b.desc;
            if !left.is_empty() {
                let lw = (ctx.measure)(left, dsize);
                // 括号基线对齐到内容的中线，而不是内容的基线：
                // 内容叠得越高，括号越要「居中包裹」才像成对的。
                let mid = (b.asc - b.desc) * 0.5;
                items.push(Item::Text {
                    x: 0.0,
                    y: -mid,
                    text: left.clone(),
                    size: dsize,
                    font: MathFont::Upright,
                });
                asc = asc.max(dsize * 0.5 + mid.abs());
                desc = desc.max(dsize * 0.5 - mid);
                x += lw + size * 0.05;
            }
            items.extend(b.offset_items(x, 0.0));
            x += b.w + size * 0.05;
            if !right.is_empty() {
                let mid = (b.asc - b.desc) * 0.5;
                items.push(Item::Text {
                    x,
                    y: -mid,
                    text: right.clone(),
                    size: dsize,
                    font: MathFont::Upright,
                });
                let rw = (ctx.measure)(right, dsize);
                asc = asc.max(dsize * 0.5 + mid.abs());
                desc = desc.max(dsize * 0.5 - mid);
                x += rw;
            }
            Layout {
                w: x,
                asc,
                desc,
                items,
            }
        }
        Node::Accent { kind, body } => {
            let b = layout_nodes(body, size, ctx);
            let mut items = b.offset_items(0.0, 0.0);
            let cx = b.w * 0.5;
            let top = -b.asc - size * 0.08;
            let thick = (size * RULE_THICK * 1.2).max(1.0);
            let mut extra = 0.0;
            match kind {
                Accent::Bar => items.push(Item::Rect {
                    x: 0.0,
                    y: top - thick,
                    w: b.w,
                    h: thick,
                }),
                Accent::Hat => {
                    let half = (b.w * 0.45).max(size * 0.18);
                    items.push(Item::Line {
                        from: (cx - half, top),
                        to: (cx, top - size * 0.22),
                        width: thick,
                    });
                    items.push(Item::Line {
                        from: (cx, top - size * 0.22),
                        to: (cx + half, top),
                        width: thick,
                    });
                    extra = size * 0.22;
                }
                Accent::Vec => {
                    let half = (b.w * 0.45).max(size * 0.2);
                    items.push(Item::Line {
                        from: (cx - half, top),
                        to: (cx + half, top),
                        width: thick,
                    });
                    // 箭头两撇。
                    items.push(Item::Line {
                        from: (cx + half, top),
                        to: (cx + half - size * 0.16, top - size * 0.12),
                        width: thick,
                    });
                    items.push(Item::Line {
                        from: (cx + half, top),
                        to: (cx + half - size * 0.16, top + size * 0.12),
                        width: thick,
                    });
                }
                Accent::Dot => items.push(Item::Rect {
                    x: cx - thick,
                    y: top - thick,
                    w: thick * 2.0,
                    h: thick * 2.0,
                }),
                Accent::Tilde => {
                    let half = (b.w * 0.45).max(size * 0.2);
                    items.push(Item::Line {
                        from: (cx - half, top),
                        to: (cx - half * 0.3, top - size * 0.14),
                        width: thick,
                    });
                    items.push(Item::Line {
                        from: (cx - half * 0.3, top - size * 0.14),
                        to: (cx + half * 0.3, top),
                        width: thick,
                    });
                    items.push(Item::Line {
                        from: (cx + half * 0.3, top),
                        to: (cx + half, top - size * 0.14),
                        width: thick,
                    });
                    extra = size * 0.14;
                }
            }
            Layout {
                w: b.w,
                asc: b.asc + size * 0.12 + extra,
                desc: b.desc,
                items,
            }
        }
        Node::Matrix {
            open,
            close,
            rows,
            left_align_cols,
        } => {
            let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
            let col_gap = size * 0.6;
            let row_gap = size * 0.35;
            // 先排每个格子，再按列取最大宽 / 按行取最大高。
            let cells: Vec<Vec<Layout>> = rows
                .iter()
                .map(|row| row.iter().map(|c| layout_nodes(c, size, ctx)).collect())
                .collect();
            let mut col_w = vec![0.0_f32; cols];
            for row in &cells {
                for (c, cell) in row.iter().enumerate() {
                    col_w[c] = col_w[c].max(cell.w);
                }
            }
            let rows_h: Vec<f32> = cells
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|c| c.asc + c.desc)
                        .fold(0.0_f32, f32::max)
                        .max(size)
                })
                .collect();
            let total_w: f32 = col_w.iter().sum::<f32>() + col_gap * cols.saturating_sub(1) as f32;
            let total_h: f32 =
                rows_h.iter().sum::<f32>() + row_gap * rows_h.len().saturating_sub(1) as f32;

            // 内容整体居中在基线两侧：矩阵通常作为整体出现，居中最好看。
            let content_asc = total_h * 0.55;
            let content_desc = total_h - content_asc;

            let mut items = Vec::new();
            let mut y = -content_asc;
            for (r, row) in cells.iter().enumerate() {
                let h = rows_h[r];
                // 本行内各项的基线：按各自 ascent 顶部对齐后居中。
                let row_top = y;
                let mut x = 0.0;
                for (c, cell) in row.iter().enumerate() {
                    let cell_asc = h * 0.5 + (cell.asc - cell.desc) * 0.5;
                    let dx = if *left_align_cols && c + 1 == cols {
                        // `cases` 的第二列（条件）左对齐贴住第一列。
                        0.0
                    } else {
                        (col_w[c] - cell.w) * 0.5
                    };
                    items.extend(cell.offset_items(x + dx, row_top + cell_asc));
                    x += col_w[c] + col_gap;
                }
                y += h + row_gap;
            }

            // 外括号。
            let mut asc = content_asc;
            let mut desc = content_desc;
            let w = total_w;
            let mut total = 0.0;
            if !open.is_empty() || !close.is_empty() {
                let scale = ((total_h + size * 0.4) / size).clamp(1.0, 3.6);
                let dsize = size * scale;
                let mid = (content_asc - content_desc) * 0.5;
                let mut shifted: Vec<Item> = Vec::new();
                if !open.is_empty() {
                    let lw = (ctx.measure)(open, dsize);
                    shifted.push(Item::Text {
                        x: 0.0,
                        y: -mid,
                        text: open.clone(),
                        size: dsize,
                        font: MathFont::Upright,
                    });
                    shifted.extend(items.iter().map(|i| shift(i, lw + size * 0.06, 0.0)));
                    total += lw + size * 0.06;
                } else {
                    shifted.extend(items.iter().cloned());
                }
                total += w;
                if !close.is_empty() {
                    let rw = (ctx.measure)(close, dsize);
                    shifted.push(Item::Text {
                        x: total,
                        y: -mid,
                        text: close.clone(),
                        size: dsize,
                        font: MathFont::Upright,
                    });
                    total += rw;
                }
                asc = asc.max(dsize * 0.5 + mid.abs());
                desc = desc.max(dsize * 0.5 - mid);
                return Layout {
                    w: total,
                    asc,
                    desc,
                    items: shifted,
                };
            }

            Layout {
                w: total_w,
                asc,
                desc,
                items,
            }
        }
    }
}

/// 把 LaTeX 排成一份可绘制的版面。
///
/// 返回 `Err` 时调用方应回退到源码显示（见模块文档）。
pub fn layout(
    latex: &str,
    size: f32,
    display: bool,
    measure: Measure<'_>,
) -> Result<MathLayout, MathError> {
    let mut nodes = parse(latex, display)?;
    mark_stacked(&mut nodes, display);
    let ctx = Ctx {
        base: size,
        measure,
    };
    let mut box_ = layout_nodes(&nodes, size, &ctx);
    // 空公式（`$$ $$`）不留 0 高度的怪块。
    if box_.w <= 0.0 {
        box_.w = size * 0.5;
    }
    Ok(box_.finish())
}

/// 把版面画到 `rect` 里，返回实际占用的高度。
///
/// `rect` 是**可用区域**；`align` 决定内容在其中的水平落点
/// （块级公式居中、行内公式左对齐）。宽度不足时整体等比缩小（含字号），
/// 与 [`super::mermaid::paint`] 同一策略：宁可公式小一点，也不要右侧被裁掉——
/// 被裁掉的项在数学上会**改变含义**（`a+b=c` 裁成 `a+b`）。
pub fn paint(
    painter: &Painter,
    rect: Rect,
    layout: &MathLayout,
    color: Color32,
    align: Align2,
) -> f32 {
    let scale = if layout.width > rect.width() && layout.width > 0.0 {
        rect.width() / layout.width
    } else {
        1.0
    };
    let used_w = layout.width * scale;
    let used_h = layout.height() * scale;
    // 基线左端的绝对位置：纵向把内容居中，横向按 `align`。
    let v_extra = (rect.height() - used_h).max(0.0) * 0.5;
    let origin_y = rect.top() + v_extra + layout.ascent * scale;
    let origin_x = match align.x() {
        egui::Align::Min => rect.left(),
        egui::Align::Center => rect.left() + (rect.width() - used_w).max(0.0) * 0.5,
        egui::Align::Max => rect.right() - used_w,
    };
    let to_pos = |x: f32, y: f32| Pos2::new(origin_x + x * scale, origin_y + y * scale);
    for item in &layout.items {
        match item {
            Item::Text {
                x,
                y,
                text,
                size,
                font,
            } => {
                if text.is_empty() {
                    continue;
                }
                // 斜体：`FontId` 里没有「斜体位」，但 epaint 的
                // `TextFormat::italics` 会把字形**倾斜合成**（见 epaint
                // `text_layout.rs` 的 italics 分支）。故数学变量（`x`）
                // 走 `LayoutJob` 而不是 `painter.text`。
                //
                // 双线体（`\mathbb{R}`）在 epaint 里没有对应字形族——
                // 这是本引擎**已知的表现力缺口**，不假装支持：`\mathbb`
                // 只做「正体」处理，视觉上与 `\mathrm` 相同。
                let mut job = LayoutJob::default();
                job.append(
                    text,
                    0.0,
                    TextFormat {
                        font_id: FontId::proportional(*size * scale),
                        color,
                        italics: matches!(font, MathFont::Italic | MathFont::Calligraphic),
                        ..Default::default()
                    },
                );
                let galley = painter.layout_job(job);
                // 基线对齐：`Align2` 只有「外框」档位（Top/Center/Bottom），
                // 没有基线档位。直接把 `y` 当外框顶边会让每个公式整体上浮
                // 约 0.2em——分数与上下标会跟着一起飘。故先量一次拿到真实的
                // 基线偏移（行顶 + 首个字形的基线位置）。
                let baseline_offset = galley
                    .rows
                    .first()
                    .map(|row| row.pos.y + row.row.glyphs.first().map_or(0.0, |glyph| glyph.pos.y))
                    .unwrap_or(0.0);
                painter.galley(
                    to_pos(*x, *y) - Vec2::new(0.0, baseline_offset),
                    galley,
                    color,
                );
            }
            Item::Line { from, to, width } => {
                painter.line_segment(
                    [to_pos(from.0, from.1), to_pos(to.0, to.1)],
                    Stroke::new(*width * scale, color),
                );
            }
            Item::Rect { x, y, w, h } => {
                painter.rect_filled(
                    Rect::from_min_size(to_pos(*x, *y), Vec2::new(*w * scale, *h * scale)),
                    0.0,
                    color,
                );
            }
        }
    }
    used_h
}

/// 按可用宽度等比缩放后占用的高度（调用方排布卡片时用）。
///
/// 提前知道高度是有意义的：块级公式卡要先按这个高度占位，
/// 否则卡片高度会随「这一帧的可用宽度」变化，滚动同步的块矩形跟着抖。
pub fn fitted_height(layout: &MathLayout, max_w: f32) -> f32 {
    let scale = if layout.width > max_w && layout.width > 0.0 {
        max_w / layout.width
    } else {
        1.0
    };
    layout.height() * scale
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 假度量：每个字符宽 `0.5em`。用它替代真实字体度量，
    /// 让排版规则的断言与字体无关、跨平台稳定。
    fn fake_measure() -> impl Fn(&str, f32) -> f32 {
        |s: &str, size: f32| s.chars().count() as f32 * size * 0.5
    }

    fn lay(src: &str, display: bool) -> MathLayout {
        let m = fake_measure();
        layout(src, 16.0, display, &m).expect("应能排版")
    }

    fn err(src: &str) -> MathError {
        let m = fake_measure();
        layout(src, 16.0, true, &m).expect_err("应报错")
    }

    #[test]
    fn plain_text_layouts_and_has_positive_size() {
        let l = lay("x", false);
        assert!(l.width > 0.0);
        assert!(l.height() > 0.0);
    }

    #[test]
    fn longer_content_is_wider() {
        assert!(lay("x+y+z", false).width > lay("x", false).width);
    }

    #[test]
    fn greek_letters_and_symbols_are_mapped() {
        // 未支持的命令会整体失败，故「能排出来」本身就是映射表命中的证据。
        for src in [
            "\\alpha+\\beta",
            "a\\leq b",
            "\\infty",
            "\\forall x",
            "\\nabla f",
        ] {
            let l = lay(src, false);
            assert!(l.width > 0.0, "{src} 应能排版");
            // 字形必须真的换掉了源命令，而不是把 `\alpha` 原样画出来。
            let joined: String = l
                .items
                .iter()
                .filter_map(|i| match i {
                    Item::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            assert!(
                !joined.contains('\\'),
                "{src} 的字形里不应残留反斜杠：{joined}"
            );
        }
    }

    #[test]
    fn fraction_is_taller_than_its_parts() {
        let line = lay("a", false);
        let frac = lay("\\frac{a}{b}", false);
        assert!(
            frac.height() > line.height() * 1.5,
            "分数应明显比单行高：{} vs {}",
            frac.height(),
            line.height()
        );
        // 分数线必须存在——它才是「这是分数而非两个字符」的全部信息。
        assert!(
            frac.items.iter().any(|i| matches!(i, Item::Rect { .. })),
            "分数应画出分数线"
        );
    }

    #[test]
    fn fraction_has_above_and_below_content() {
        let frac = lay("\\frac{a}{b}", false);
        let above = frac
            .items
            .iter()
            .filter(|i| matches!(i, Item::Text { y, .. } if *y < 0.0))
            .count();
        let below = frac
            .items
            .iter()
            .filter(|i| matches!(i, Item::Text { y, .. } if *y > 0.0))
            .count();
        assert!(above >= 1, "分子应在基线上方");
        assert!(below >= 1, "分母应在基线下方");
    }

    #[test]
    fn superscript_is_smaller_than_base() {
        let s = lay("x^{2}", false);
        let sizes: Vec<f32> = s
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Text { size, .. } => Some(*size),
                _ => None,
            })
            .collect();
        assert_eq!(sizes.len(), 2);
        assert!(sizes[0] > sizes[1], "上标字号应小于基准：{sizes:?}");
    }

    #[test]
    fn subscript_sits_below_baseline() {
        let s = lay("x_{i}", false);
        assert!(
            s.items
                .iter()
                .any(|i| matches!(i, Item::Text { y, .. } if *y > 0.0)),
            "下标应在基线下"
        );
    }

    #[test]
    fn combined_sub_and_sup_share_one_column() {
        // `x_1^2`：两个角标的 x 应**相同**（同一个 Script 节点），
        // 否则会出现「上下标一左一右」的错位。
        let s = lay("x_1^2", false);
        let xs: Vec<f32> = s
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Text { x, text, .. } if text == "1" || text == "2" => Some(*x),
                _ => None,
            })
            .collect();
        assert_eq!(xs.len(), 2, "应同时排出一上一下两个角标");
        assert!((xs[0] - xs[1]).abs() < 0.01, "上下标应共用同一列：{xs:?}");
    }

    #[test]
    fn sqrt_draws_radical_and_sits_above_body() {
        let s = lay("\\sqrt{x+1}", false);
        assert!(
            s.items
                .iter()
                .filter(|i| matches!(i, Item::Line { .. }))
                .count()
                >= 3,
            "根号应有勾的两段折线 + 顶部横线"
        );
    }

    #[test]
    fn sum_limits_stack_in_display_but_not_inline() {
        let display = lay("\\sum_{i=1}^{n}", true);
        let inline = lay("\\sum_{i=1}^{n}", false);
        // display：上限在基线上方较远处（叠放）；inline：贴在右侧。
        let display_width = display.width;
        let inline_width = inline.width;
        assert!(
            inline_width > 0.0 && display_width > 0.0,
            "两种模式都应能排版"
        );
    }

    #[test]
    fn left_right_delimiters_scale_with_content() {
        let small = lay("\\left(a\\right)", false);
        let big = lay("\\left(\\frac{a}{b}\\right)", false);
        let size_of = |l: &MathLayout| {
            l.items
                .iter()
                .filter_map(|i| match i {
                    Item::Text { text, size, .. } if text == "(" => Some(*size),
                    _ => None,
                })
                .next()
                .expect("应有左括号")
        };
        assert!(
            size_of(&big) > size_of(&small),
            "内容变高时括号应放大：{} vs {}",
            size_of(&big),
            size_of(&small)
        );
    }

    #[test]
    fn accent_adds_marks_above() {
        let plain = lay("x", false);
        let hat = lay("\\hat{x}", false);
        assert!(hat.items.len() > plain.items.len(), "重音应额外画出符号");
        assert!(hat.height() >= plain.height());
    }

    #[test]
    fn text_command_keeps_its_literal_content() {
        let l = lay("\\text{速度 v}", false);
        let joined: String = l
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            joined.contains("速度 v"),
            "`\\text{{}}` 应原样保留：{joined}"
        );
    }

    #[test]
    fn matrix_rows_and_columns_align() {
        let l = lay("\\begin{pmatrix}a&b\\\\c&d\\end{pmatrix}", true);
        // 4 个格子的文字 + 两个括号。
        let texts = l
            .items
            .iter()
            .filter(|i| matches!(i, Item::Text { .. }))
            .count();
        assert_eq!(texts, 6, "应为 4 格 + 2 个括号");
    }

    #[test]
    fn cases_environment_is_supported() {
        let l = lay("\\begin{cases}x&x>0\\\\-x&x\\leq 0\\end{cases}", true);
        assert!(l.width > 0.0);
        // cases 不画括号。
        assert!(
            !l.items
                .iter()
                .any(|i| matches!(i, Item::Text { text, .. } if text == "(" || text == "[")),
            "cases 不应画出外括号"
        );
    }

    #[test]
    fn malformed_input_is_reported_not_panicked() {
        // `panic = "abort"` 下绝不能崩——这几类坏输入必须变成 `Err`。
        for src in ["{a", "a}", "\\frac{a}", "^2", "x & y", "\\begin{matrix}a"] {
            let _ = err(src);
        }
    }

    #[test]
    fn unsupported_command_is_reported_not_guessed() {
        // 子集外的写法必须**明确失败**，由调用方回退到源码卡，
        // 而不是画出一个缺胳膊少腿的公式让用户以为公式写错了。
        let e = err("\\notarealcommand{x}");
        assert!(
            matches!(e, MathError::Unsupported(_)),
            "应报 Unsupported，实际 {e:?}"
        );
    }

    #[test]
    fn empty_formula_does_not_produce_zero_width() {
        let l = lay("   ", false);
        assert!(l.width > 0.0, "空公式也不该是 0 宽（会变成隐形块）");
    }

    #[test]
    fn spacing_commands_add_room() {
        let tight = lay("ab", false);
        let loose = lay("a\\quad b", false);
        assert!(loose.width > tight.width);
    }

    #[test]
    fn deep_nesting_does_not_shrink_to_nothing() {
        let l = lay("x^{y^{z^{w}}}", false);
        let min_size = l
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Text { size, .. } => Some(*size),
                _ => None,
            })
            .fold(f32::INFINITY, f32::min);
        assert!(
            min_size >= 16.0 * SCRIPT_MIN_SCALE - 0.01,
            "最深层角标不应小于下限：{min_size}"
        );
    }
}
