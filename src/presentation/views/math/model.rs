//! 语法树：排版节点、字体族 / 重音枚举，以及树变换辅助函数。

/// 数学排版样式族（只区分「正体 / 斜体 / 手写 / 双线」，不做真字体切换）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum MathFont {
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
pub(super) enum Accent {
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
pub(super) enum Node {
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

/// 一个字符的默认字体：字母斜体、其余正体。
pub(super) fn default_font_for(c: char) -> MathFont {
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
pub(super) fn retext(nodes: Vec<Node>, font: MathFont) -> Vec<Node> {
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
pub(super) fn mark_stacked(nodes: &mut [Node], display: bool) {
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
