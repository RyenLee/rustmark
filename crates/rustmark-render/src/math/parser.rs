//! 解析器：在字符序列上做**单遍**下降解析。
//!
//! [`Parser`] 的固有 impl 按功能簇拆至兄弟模块：
//! - [`parser::basics`] — peek / bump / skip / expect / 命令名识别
//! - [`parser::row`] — `parse_row` / `parse_until` / `apply_script`
//! - [`parser::command`] — `parse_command` / 符号查表 / 参数读取
//! - [`parser::delim`] — `\left` / `\right` / `\bigl` 定界符解析
//! - [`parser::environment`] — 矩阵 / `\begin{...}` 环境

use super::model::{Accent, MathFont, Node, default_font_for, retext};
use super::symbols::{ENVIRONMENTS, FUNCTIONS, LIMIT_OPS, SYMBOLS};
use super::*;

mod basics;
mod command;
mod delim;
mod environment;
mod row;

// ============================================================================
// Parser struct + 入口 free fn
// ============================================================================

/// 解析器：在字符序列上做**单遍**下降解析。
struct Parser {
    src: Vec<char>,
    pos: usize,
    /// 当前是否处于 display（行间）模式。
    ///
    /// 只影响一件事：大运算符的上下限是叠放还是放在右侧。
    /// 与 TeX 一致——行内 `$\sum_{i}$` 也是右侧，否则会撑高行距。
    display: bool,
    /// 剩余可下放的递归预算（F-04）。
    ///
    /// 深嵌套公式（`x^{x^{x^{…}}}`、嵌套 `\frac` / `\sqrt` / 矩阵 / 定界符）
    /// 会让下降解析器一直递归直到栈溢出，`panic = "abort"` 下直接终止进程。
    /// 所有子表达式最终都经由 [`parse_row`] 进入，故在该入口统一消耗预算，
    /// 超限即报 `MathError::Malformed`，由渲染层回退到源码卡。
    depth_remaining: u32,
}

/// 下降解析允许的最大嵌套层数（F-04）。
///
/// 远超任何合理公式的嵌套深度，但远在栈溢出之下；`x^{…}` 每深一层消耗一格。
const MAX_MATH_RECURSION: u32 = 256;

/// 解析一段 LaTeX 为语法树。
pub(super) fn parse(src: &str, display: bool) -> Result<Vec<Node>, MathError> {
    let mut p = Parser {
        src: src.chars().collect(),
        pos: 0,
        display,
        depth_remaining: MAX_MATH_RECURSION,
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
