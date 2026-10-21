//! 字体装配：把发现的字体字节组装成 egui `FontDefinitions`。
//!
//! 本模块负责「读字节 → 挂进族」的纯逻辑，不关心字体从哪发现
//! （发现见 `discovery.rs`，门面组装见 `mod.rs`）。核心函数
//! [`extend_with_cjk`] / [`prepend_selected_to_monospace`] 接受已加载的字节或
//! 字体数据，可在无真实字体文件的单测中直接用占位字节验证族顺序。

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

use crate::shared::fonts::DEFAULT_EDITOR_FONT_KEY;

use super::discovery::builtin_font_path;

/// CJK 字体在 `font_data` 中的键名。
pub const CJK_KEY: &str = "cjk";

/// 内置数学兜底字体（TeX Gyre DJV Math，DejaVu 的数学扩展版）。
pub const MATH_FALLBACK_FILE: &str = "DejaVuMathTeXGyre.ttf";

/// 数学兜底字体在 `font_data` 中的键名。
pub const MATH_FALLBACK_KEY: &str = "math-fallback";

/// 内置 CJK 兜底字体（Noto Sans SC 的 GB2312 子集，1.5MB）。
pub const BUILTIN_CJK_FILE: &str = "NotoSansSC-Subset.ttf";

/// 内置 CJK 兜底在 `font_data` 中的键名。
pub const BUILTIN_CJK_KEY: &str = "builtin-cjk";

/// 把用户选中的字体**置顶**进内置 `Monospace` 族（纯函数，便于单测）。
///
/// # 为什么是「置顶进内置族」而不是「自建一个族」
///
/// 自建族要求每一处排版前该族都已注册；而 egui 对未注册的
/// `FontFamily::Name` 是 **panic**（`epaint fonts.rs:808`），
/// 在 `panic = "abort"` 下即窗口消失。内置族永远有绑定，没有这个风险。
/// 完整说明见 `crate::shared::fonts` 模块文档。
///
/// # 三条规则
///
/// 1. **哨兵 / 空 / 未登记的键 → 原样返回**：观感与改造前逐像素一致，
///    旧配置里残留的非法字体名（如 `sans-serif`）也能安全退化。
/// 2. **置顶（不是追加）**：追加的话西文仍由 Hack / Ubuntu-Light 渲染，
///    用户的选择看不见——那就是这个设置此前的「死设置」病因。
/// 3. **先去重再插入**：选中键可能本就在族里（如某字体恰好被 egui 内置收录），
///    不去重会得到重复成员，egui 会为它排版两遍。
///
/// 注意这与 [`extend_with_cjk`] 的「必须追加到末尾」不矛盾：那条管的是
/// **默认**装配（中文兜底不能抢西文）；本条是用户**显式**选择后的覆写，
/// 选中字体的拉丁字形优先正是用户意图。
pub fn prepend_selected_to_monospace(
    definitions: &mut FontDefinitions,
    selected_family: Option<&str>,
) {
    let Some(sel) = selected_family.filter(|s| {
        !s.trim().is_empty()
            && *s != DEFAULT_EDITOR_FONT_KEY
            && definitions.font_data.contains_key(*s)
    }) else {
        return;
    };
    let mono = definitions.families.get_mut(&FontFamily::Monospace);
    let Some(mono) = mono else {
        // egui 默认定义必然含 Monospace 族；真丢了也不慌——自建一个只含
        // 选中字体的族，比让用户的选择静默失效好。
        definitions
            .families
            .insert(FontFamily::Monospace, vec![sel.to_string()]);
        return;
    };
    mono.retain(|k| k != sel);
    mono.insert(0, sel.to_string());
}

/// 读取第一个可用的 CJK 字体，返回 `(路径, 字节)`。
///
/// 逐个尝试候选路径，第一个能读通的即返回——**不做字体内容校验**：
/// 候选清单里的都是常规轮廓字体，而校验需要额外解析（见 `mod.rs` 模块文档的安全约束）。
pub fn load_cjk_font() -> Option<(String, Vec<u8>)> {
    super::discovery::candidate_paths()
        .into_iter()
        .find_map(|path| {
            std::fs::read(&path)
                .ok()
                .map(|bytes| (path.to_string_lossy().into_owned(), bytes))
        })
}

/// 把 CJK 字体追加进默认字体定义。
///
/// # 必须追加到族**末尾**，不能插到开头
///
/// 这是本模块最容易被"优化"错的地方，代价却很大：
///
/// - 插到开头 → 西文也改用中文字体的拉丁字形（雅黑的拉丁部分不如 Ubuntu-Light 精细，
///   观感明显变差），**且代码块会失去等宽**——中文等宽字体的西文字形通常不等宽，
///   代码缩进与对齐会全乱。
/// - 追加到末尾 → 保持正确语义：**西文优先命中，缺失才回落到 CJK**。
///
/// 表现层无法察觉这一差别（都只是"字体变了"），故有专门的单元测试锁定族内顺序。
///
/// # 完整的兜底链（`mod.rs::font_definitions` 的装配顺序）
///
/// ```text
/// 西文（Ubuntu-Light / Hack）──► 数学兜底（MATH_FALLBACK_KEY）──► 系统 CJK ──► 内置 CJK 子集
/// ```
///
/// 数学兜底必须**插在 CJK 之前**：雅黑等中文字体自己也带一部分数学字形
/// （≤ ≥ ≠ 之类），若让 CJK 先命中，同一页公式里会出现"一部分符号是雅黑
/// 形状、另一部分是数学字体形状"的混搭；置前则数学符号形状一致，
/// 中文字符继续落到 CJK（数学字体没有汉字）。
///
/// 内置 CJK 子集必须**排在系统 CJK 之后**：系统字体（雅黑 / 苹方）观感更好
/// 且字重齐全，内置子集只是「一台机器上什么中文字体都没有」时的保险。
/// 顺序反了会让有中文字体的用户也被迫用子集（生僻字反而变方框）。
pub fn extend_with_cjk(definitions: &mut FontDefinitions, bytes: Vec<u8>) {
    definitions
        .font_data
        .insert(CJK_KEY.to_owned(), Arc::new(FontData::from_owned(bytes)));

    // 比例族与等宽族都要挂：正文用比例族，编辑区与代码块用等宽族，
    // 漏掉任一族都会让那一处的中文变成方框。
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push(CJK_KEY.to_owned());
    }
}

/// 把内置数学兜底字体挂进两个族（位置在 CJK 之前，见 [`extend_with_cjk`] 的链图）。
///
/// 字体随发行包以文件分发（体积红线）：找不到或读不通时**跳过**——数学符号
/// 缺字形只影响公式观感，不阻断启动（`font_definitions` 的调用方不因此报错）。
pub fn extend_with_math_fallback(definitions: &mut FontDefinitions) {
    let Some(path) = builtin_font_path(MATH_FALLBACK_FILE) else {
        return;
    };
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    definitions.font_data.insert(
        MATH_FALLBACK_KEY.to_owned(),
        Arc::new(FontData::from_owned(bytes)),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push(MATH_FALLBACK_KEY.to_owned());
    }
}

/// 把内置 CJK 子集挂进两个族，位置在**最末**（系统 CJK 之后）。
///
/// 无条件挂载（文件可解析时）：它轻量、只服务「系统什么中文字体都没有」
/// 的场景，有系统字体时永远排在后排，不抢渲染。字体随发行包以文件分发
/// （体积红线），缺失时跳过——此时裸机中文回落到系统 CJK。
pub fn extend_with_builtin_cjk(definitions: &mut FontDefinitions) {
    let Some(path) = builtin_font_path(BUILTIN_CJK_FILE) else {
        return;
    };
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    definitions.font_data.insert(
        BUILTIN_CJK_KEY.to_owned(),
        Arc::new(FontData::from_owned(bytes)),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push(BUILTIN_CJK_KEY.to_owned());
    }
}
