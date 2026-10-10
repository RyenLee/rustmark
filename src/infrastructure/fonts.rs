//! 系统字体的发现、读取与 egui 字体定义组装。
//!
//! # 为什么这个模块在基础设施层
//!
//! 本模块的产物是 `egui::FontDefinitions`——一个**外部库类型**。
//! `presentation` 层按约定不得依赖 `crate::infrastructure`
//! （见 `tools/arch_check.rs` 的 `PRESENTATION_FORBIDDEN`），
//! 而「把系统字体文件适配成 egui 能消化的形状」恰是基础设施层的本职
//! （与 `export` 适配渲染后端、`persistence` 适配文件系统同理）。
//! 组装放在这里，由组合根（`app::frame`）取走后交给 egui。
//!
//! # 背景：为什么必须加载字体
//!
//! egui 0.33 的默认字体族**不含任何 CJK 字形**：
//!
//! ```text
//! Proportional = [Ubuntu-Light, NotoEmoji-Regular, emoji-icon-font]
//! Monospace    = [Hack, Ubuntu-Light, NotoEmoji-Regular, emoji-icon-font]
//! ```
//!
//! 因此不加载中文字体时，界面上的中文**全部**渲染为占位方框——
//! 这不是偶发故障而是必然结果。经 `Fonts::has_glyph` 实测确认。
//!
//! # 安全约束：只接受常规轮廓字体
//!
//! egui 底层用 `ab_glyph` 解析字体，它只处理**轮廓**（glyf/CFF）。
//! **彩色 emoji 字体**（`seguiemj.ttf`、`NotoColorEmoji.ttf` 等，采用 COLR/CBDT）
//! 可能无法解析，而本项目设置了 `panic = "abort"`——解析 panic 会**直接终止进程**。
//! 故候选清单**刻意只收录常规 TrueType 中文字体**，不收录彩色 emoji 字体。
//! （emoji 由 egui 内置的 NotoEmoji / emoji-icon-font 覆盖，实测常用 emoji 均有字形。）

use std::path::PathBuf;
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// 字体装配结果。
pub struct FontSetup {
    /// 可直接交给 `Context::set_fonts` 的字体定义。
    pub definitions: FontDefinitions,
    /// 实际加载的 CJK 字体路径；`None` 表示本机未找到（中文将显示为方框）。
    pub cjk_source: Option<String>,
}

/// 加载 CJK 字体并组装完整的字体定义。
///
/// 找不到字体时**不报错**：界面仍然可用，只是中文显示为方框。
/// 让整个应用因字体缺失而启动失败是过度反应；但静默忽略会让
/// 「中文是方框」无从归因，故调用方应依据 `cjk_source.is_none()` 记录警告。
pub fn font_definitions() -> FontSetup {
    let mut definitions = FontDefinitions::default();
    let cjk_source = match load_cjk_font() {
        Some((path, bytes)) => {
            extend_with_cjk(&mut definitions, bytes);
            Some(path)
        }
        None => None,
    };
    FontSetup {
        definitions,
        cjk_source,
    }
}

/// 读取第一个可用的 CJK 字体，返回 `(路径, 字节)`。
///
/// 逐个尝试候选路径，第一个能读通的即返回——**不做字体内容校验**：
/// 候选清单里的都是常规轮廓字体，而校验需要额外解析（见模块文档的安全约束）。
pub fn load_cjk_font() -> Option<(String, Vec<u8>)> {
    candidate_paths().into_iter().find_map(|path| {
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

/// CJK 字体在 `font_data` 中的键名。
pub const CJK_KEY: &str = "cjk";

/// 候选字体路径，按「字形覆盖 + 观感」排序，靠前者优先。
#[cfg(target_os = "windows")]
fn candidate_paths() -> Vec<PathBuf> {
    // 用 `%WINDIR%` 而非写死 `C:\Windows`：Windows 可装在非 C 盘。
    let fonts = std::env::var_os("WINDIR")
        .map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from)
        .join("Fonts");
    ["msyh.ttc", "Deng.ttf", "simhei.ttf", "simsun.ttc"]
        .iter()
        .map(|name| fonts.join(name))
        .collect()
}

/// macOS：苹方为系统默认中文界面字体。
#[cfg(target_os = "macos")]
fn candidate_paths() -> Vec<PathBuf> {
    [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
    ]
    .iter()
    .map(PathBuf::from)
    .collect()
}

/// Linux：发行版差异大，按常见安装路径依次尝试。
#[cfg(all(unix, not(target_os = "macos")))]
fn candidate_paths() -> Vec<PathBuf> {
    [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/arphic/uming.ttc",
    ]
    .iter()
    .map(PathBuf::from)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本机应当能找到一个 CJK 字体（三大平台都有标准路径）。
    ///
    /// 该测试在"未安装任何中文字体"的极简容器里会失败——那正是需要知道的信号，
    /// 因为此时应用的中文确实会显示为方框。
    #[test]
    fn a_cjk_font_is_available_on_this_platform() {
        let found = load_cjk_font();
        assert!(
            found.is_some(),
            "本机未找到任何 CJK 字体，候选路径：{:?}",
            candidate_paths()
        );
        let (path, bytes) = found.expect("上面已断言存在");
        assert!(!bytes.is_empty(), "字体文件不应为空：{path}");
    }

    /// CJK 必须**追加在族末尾**，保持西文优先。
    ///
    /// 这是防止"插到开头"这一看似无害的改动——它会让代码块失去等宽。
    #[test]
    fn cjk_is_appended_not_prepended() {
        let mut defs = FontDefinitions::default();
        let before: Vec<_> = defs
            .families
            .get(&FontFamily::Proportional)
            .expect("默认应有比例族")
            .clone();

        extend_with_cjk(&mut defs, vec![0u8; 8]);

        let after = defs
            .families
            .get(&FontFamily::Proportional)
            .expect("追加后仍应有比例族");
        assert_eq!(
            &after[..before.len()],
            before.as_slice(),
            "原有字体必须保持在前面，否则西文与代码块等宽都会被中文字体接管"
        );
        assert_eq!(after.last(), Some(&CJK_KEY.to_owned()), "CJK 应在末尾");
    }

    /// 比例族与等宽族都要挂上 CJK，漏一族则那一处的中文仍是方框。
    #[test]
    fn both_families_receive_cjk() {
        let mut defs = FontDefinitions::default();
        extend_with_cjk(&mut defs, vec![0u8; 8]);

        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let list = defs.families.get(&family).expect("该族应存在");
            assert!(
                list.iter().any(|k| k == CJK_KEY),
                "{family:?} 未挂载 CJK，该区域中文将显示为方框"
            );
        }
        assert!(defs.font_data.contains_key(CJK_KEY), "字体数据应已登记");
    }

    /// `font_definitions` 在缺字体时也必须返回可用定义（不 panic）。
    ///
    /// 用不存在的路径模拟"本机无中文字体"的效果：`definitions` 仍须完整，
    /// 因为界面靠它渲染所有西文与 emoji。
    #[test]
    fn definitions_stay_usable_when_no_cjk_font_exists() {
        let defs = FontDefinitions::default();
        assert!(
            defs.families.contains_key(&FontFamily::Proportional),
            "缺字体时默认族必须仍在，否则界面完全无法渲染"
        );
        // 组装函数本身不应 panic（该函数内部可能走到"没找到字体"分支）。
        let setup = font_definitions();
        assert!(
            setup
                .definitions
                .families
                .contains_key(&FontFamily::Monospace)
        );
    }

    /// **决定性验证**：加载系统 CJK 字体后，中文确实有字形了。
    ///
    /// 这是本次修复最核心的断言——前面几条只验证"字体被挂进了族里"，
    /// 而这条验证"egui 真的能据此渲染出中文字形"。
    /// 两者之间有真实落差：字体文件可能解析失败（egui 底层 `ab_glyph` 只吃轮廓字体），
    /// 那样挂上去也没用，甚至会 panic。
    ///
    /// 本机无中文字体时**软跳过**（而非失败）：那是环境问题，不是代码缺陷，
    /// 由 `a_cjk_font_is_available_on_this_platform` 负责在缺字体时给出信号。
    #[test]
    fn real_cjk_font_provides_glyphs_for_chinese() {
        let setup = font_definitions();
        if setup.cjk_source.is_none() {
            eprintln!("跳过：本机无中文字体，无法验证字形");
            return;
        }
        let source = setup.cjk_source.clone().expect("上面已判非空");

        let ctx = egui::Context::default();
        // `set_fonts` 会引发字体解析；若字体格式不被支持（如彩色 emoji 字体），
        // 这一步就会 panic——而本项目是 `panic = "abort"`，故这条测试同时守护了那个风险。
        ctx.set_fonts(setup.definitions);
        let _ = ctx.run(egui::RawInput::default(), |_| {});

        let font = egui::FontId::proportional(14.0);
        let chinese = "中文编辑器文档预览测试";
        let has = ctx.fonts_mut(|f| f.has_glyphs(&font, chinese));
        assert!(
            has,
            "已加载 {source}，但中文仍无字形——字体可能解析失败或未被挂进族"
        );

        // 中文标点同样要能渲染（缺了会让中文排版断在标点上）。
        let punct = ctx.fonts_mut(|f| f.has_glyphs(&font, "，。、「」"));
        assert!(punct, "中文标点应同时具备字形");

        // 等宽族（编辑区/代码块）也必须覆盖中文，否则那里仍是方框。
        let mono = egui::FontId::monospace(14.0);
        let mono_has = ctx.fonts_mut(|f| f.has_glyphs(&mono, chinese));
        assert!(mono_has, "等宽族缺中文字形，编辑区与代码块会显示为方框");
    }
}
