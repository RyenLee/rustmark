//! 系统字体的发现、读取与字体定义组装（egui 无关）。
//!
//! # 为什么这个模块在基础设施层
//!
//! 本模块的产物是 [`PortFontDefinitions`]——一个**纯数据端口类型**，不是 egui
//! 类型（V11-63 阶段 B 端口化的结果，见 `port` 模块文档）。它之所以留在基础设施层，
//! 是因为「把系统字体文件适配成 egui 能消化的形状」正是基础设施层的本职
//! （与 `export` 适配渲染后端、`persistence` 适配文件系统同理）。
//! 由组合根（`app::frame`）取走后经适配器转成 `egui::FontDefinitions` 再交给 egui。
//!
//! 这层间接也解掉了原先的结构性矛盾：`presentation` 层按约定不得依赖
//! `crate::infrastructure`（见 `tools/arch_check.rs` 的 `PRESENTATION_FORBIDDEN`），
//! 而基础设施此前直接产出 `egui::FontDefinitions`——一边禁 egui、一边把 egui
//! 类型交给表现层。端口化后基础设施零 egui 依赖，该矛盾消失。
//!
//! # 背景：基础族从哪来、为什么还要加载中文字体
//!
//! REL-03 体积缓解后（egui 的 `default_fonts` / `monochrome_emoji_fonts`
//! 已关闭，见 `Cargo.toml` 注释），`PortFontDefinitions::default()` 返回的是
//! **空族**——界面基础字形改由 [`install_egui_base_fonts`] 从随包分发的
//! 字体文件（`assets/fonts/` → 发行包 `fonts/`）装回，族构成与 egui
//! 内嵌时代逐项一致（唯一差异是不再挂 `egui-icons`，本项目零引用）：
//!
//! ```text
//! Proportional = [Ubuntu-Light, NotoEmoji-Regular, emoji-icon-font]
//! Monospace    = [Hack, Ubuntu-Light, NotoEmoji-Regular, emoji-icon-font]
//! ```
//!
//! 这条基础族**不含任何 CJK 字形**（Ubuntu-Light / Hack 是西文字体，
//! emoji 字体只有图形符号），因此不加载中文字体时，界面上的中文
//! **全部**渲染为占位方框——这不是偶发故障而是必然结果。
//! 经 `Fonts::has_glyph` 实测确认。
//!
//! # 安全约束：只接受常规轮廓字体
//!
//! egui 0.37 底层的文字管线（read-fonts / skrifa）面向**轮廓**
//! （glyf/CFF）字体设计；彩色 emoji 字体（`seguiemj.ttf`、
//! `NotoColorEmoji.ttf` 等，采用 COLR/CBDT）的解析行为不在本项目
//! 测试覆盖内，而本项目设置了 `panic = "abort"`——解析 panic 会
//! **直接终止进程**。故候选清单**刻意只收录常规 TrueType 中文字体**，
//! 不收录彩色 emoji 字体。
//! （emoji 由随包分发的 NotoEmoji / emoji-icon-font 覆盖，实测常用
//! emoji 均有字形。）
//!
//! # 分层
//!
//! - `discovery`：路径与存在性发现（可注入根目录，可单测）；
//! - `assembly`：字节 → [`PortFontDefinitions`] 的纯装配逻辑；
//! - `port`：egui 无关的端口类型（[`PortFontDefinitions`] / [`PortFontFamily`] /
//!   [`PortFontData`]）；
//! - 本门面：组装各层并产出 [`FontSetup`]。

pub(crate) mod assembly;
pub(crate) mod discovery;
/// egui 无关的字体定义端口类型（V11-63 阶段 B）。由组合根经适配器转成
/// `egui::FontDefinitions`。`pub` 以便根 crate 测试与适配器引用。
pub mod port;

pub use port::*;

use rustmark_core::shared::fonts::EditorFontChoice;

// 以下三项 `pub`：V11-63 阶段 B 把 4 个「字形真的能渲染」类测试迁到根 crate 的
// `tests/presentation/font_glyph_test.rs`（那里才有 egui 与适配器），迁移后它们
// 仍需要「只装内置 CJK 子集」「按真实根目录发现字体」这两个能力——
// `font_definitions` 做不到（它必然混入系统 CJK），故开放为受支持接口。
pub use assembly::extend_with_builtin_cjk;
pub(crate) use assembly::{
    extend_with_cjk, extend_with_math_fallback, load_cjk_font, prepend_selected_to_monospace,
};
pub use discovery::{FontSource, default_roots, discover_font_sources};
pub(crate) use discovery::{builtin_font_path, to_choices};

// 以下名称仅 `#[cfg(test)] mod tests` 使用：非测试编译里保持不可见，
// 避免 dead_code 警告；测试编译经 `use super::*` 照常拿到。
#[cfg(test)]
use assembly::{BUILTIN_CJK_KEY, CJK_KEY, MATH_FALLBACK_KEY};
#[cfg(test)]
use discovery::{BUILTIN_EDITOR_FONTS, candidate_paths, system_mono_candidates};

/// 字体装配结果。
pub struct FontSetup {
    /// egui 无关的端口字体定义；组合根经适配器转成 `egui::FontDefinitions`
    /// 后交给 `Context::set_fonts`（V11-63 阶段 B：基础设施不得依赖 egui）。
    pub definitions: PortFontDefinitions,
    /// 实际加载的 CJK 字体路径；`None` 表示本机未找到（中文将显示为方框）。
    pub cjk_source: Option<String>,
    /// 设置页「字体」下拉可用的选项（含默认的「系统等宽」）。
    pub editor_fonts: Vec<EditorFontChoice>,
}

/// 加载 CJK 字体并组装完整的字体定义。
///
/// 找不到字体时**不报错**：界面仍然可用，只是中文显示为方框。
/// 让整个应用因字体缺失而启动失败是过度反应；但静默忽略会让
/// 「中文是方框」无从归因，故调用方应依据 `cjk_source.is_none()` 记录警告。
///
/// # `selected_family`：等宽字体选择
///
/// 传入用户在设置页选中的字体键（[`DEFAULT_EDITOR_FONT_KEY`] 哨兵，
/// 或 `user:<名>`）。函数据此把选中字体**置顶进 egui 内置的 `Monospace` 族**
/// （见 [`prepend_selected_to_monospace`]，以及 `shared::fonts` 里
/// 「为什么不用自建族」的说明）。效果：
///
/// - 选中的西文字体优先命中（编辑区 / 预览代码块观感随选择变化）；
/// - 中文始终能回落到 CJK 字体（不会变方框）；
/// - 选中字体缺字时再回落到内置等宽（不会变豆腐块）。
///
/// 哨兵或「字体数据里找不到的键」会被忽略，等宽族保持内置原样——
/// 这样旧配置里残留的非法字体名（如 `sans-serif`）也能安全回退，
/// 而不是让设置页「改了没生效」。
pub fn font_definitions(selected_family: Option<&str>) -> FontSetup {
    let mut definitions = PortFontDefinitions::default();
    // REL-03：两个字体 feature 关闭后 `default()` 是空族——先装回基础族
    // （首选字形 + emoji 兜底），后续数学 / CJK 追加才有「西文优先」的链头。
    install_egui_base_fonts(&mut definitions);

    // 发现并登记可选的编辑区字体（内置三款 + 项目 fonts/ 目录 + 少量常见系统等宽字体）。
    let sources = discover_font_sources(&default_roots());
    for source in &sources {
        // 读不通的候选不登记，下拉里也不会出现（内置资产不存在读不通的问题）。
        if let Some(data) = source.font_data() {
            definitions.font_data.insert(source.key.clone(), data);
        }
    }

    let cjk_source = match load_cjk_font() {
        Some((path, bytes)) => {
            // 数学兜底先挂、CJK 后挂：两者都追加在族末尾，相对顺序即兜底链
            // 「西文 → 数学 → 中文」（见 `extend_with_cjk` 的链图）。
            extend_with_math_fallback(&mut definitions);
            extend_with_cjk(&mut definitions, bytes);
            Some(path)
        }
        None => {
            // 没有中文字体时数学兜底照样挂：缺中文只影响中文，缺数学符号
            // 影响的是所有公式——两者不该互相牵连。
            extend_with_math_fallback(&mut definitions);
            None
        }
    };

    // 内置 CJK 子集**无条件**挂上，且必在最末：系统没中文字体时
    // （精简 Linux / 最小化容器）它是中文不至于全变方框的最后一道防线；
    // 系统有时它只是沉默的备份（排在系统 CJK 之后，不会抢着渲染）。
    extend_with_builtin_cjk(&mut definitions);

    prepend_selected_to_monospace(&mut definitions, selected_family);

    FontSetup {
        definitions,
        cjk_source,
        editor_fonts: to_choices(&sources),
    }
}

/// 轻量版清单：只发现字体（存在性检查），不读字节、不装配。
///
/// 供应用启动时填充设置页下拉用；完整装配走 [`font_definitions`]。
/// 分开是为了避免「为了列个下拉就把所有字体文件读两遍」。
pub fn editor_font_choices() -> Vec<EditorFontChoice> {
    to_choices(&discover_font_sources(&default_roots()))
}

/// REL-03：把 egui 的默认字体（如今随发行包以文件分发）装回基础族。
///
/// `default_fonts` / `monochrome_emoji_fonts` 两个 feature 关闭后，
/// `PortFontDefinitions::default()` 等价于 egui 的 `FontDefinitions::empty()`——
/// Proportional / Monospace 两个族都是空列表，界面没有任何首选字形。
/// 本函数按 egui 内嵌时代的族顺序，把 `epaint_default_fonts` 0.37.0 的
/// 同名同源字体文件（`assets/fonts/`，许可证见同目录 Fonts-LICENSE.txt）
/// 装回 `font_data` 与两个基础族；唯一差异是不再挂 `egui-icons`
/// （仅 `egui::special_emojis` 使用，本项目与 egui 自身绘制均零引用）。
///
/// scale 微调与 epaint 原实现逐项一致（NotoEmoji 0.81、emoji-icon-font
/// 0.90，其余不缩放），保证与内嵌时代字形等价、界面观感不变。
///
/// # 文件缺失时的行为
///
/// 对应键**不进任何族**（宁可首选字形落到数学 / CJK 兜底、观感退化），
/// 绝不留悬空键——`FontProvider` 见到族里引用了 `font_data` 没有的键
/// 会 panic（`panic = "abort"` 下即进程终止）。`assets/fonts/` 被删的
/// 干净克隆与字体目录缺失的安装态都依赖这条退化路径。
fn install_egui_base_fonts(definitions: &mut PortFontDefinitions) {
    /// （文件名，egui 键，挂入的族，缩放微调）——顺序即族内追加顺序。
    const BASE: &[(&str, &str, &[PortFontFamily], f32)] = &[
        (
            "Hack-Regular.ttf",
            "Hack",
            &[PortFontFamily::Monospace],
            1.0,
        ),
        (
            "Ubuntu-Light.ttf",
            "Ubuntu-Light",
            &[PortFontFamily::Monospace, PortFontFamily::Proportional],
            1.0,
        ),
        (
            "NotoEmoji-Regular.ttf",
            "NotoEmoji-Regular",
            &[PortFontFamily::Monospace, PortFontFamily::Proportional],
            0.81,
        ),
        (
            "emoji-icon-font.ttf",
            "emoji-icon-font",
            &[PortFontFamily::Monospace, PortFontFamily::Proportional],
            0.90,
        ),
    ];

    for (file, key, families, scale) in BASE {
        let Some(path) = builtin_font_path(file) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let mut data = PortFontData::from_owned(bytes);
        if *scale != 1.0 {
            data = data.with_scale(*scale);
        }
        definitions.font_data.insert((*key).to_owned(), data);
        for family in *families {
            if let Some(list) = definitions.families.get_mut(family) {
                list.push((*key).to_owned());
            }
        }
    }
}

#[cfg(test)]
mod tests;
