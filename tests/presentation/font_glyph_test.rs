//! V11-63 阶段 B：字体端到端字形验证（从 core 的 `infrastructure/fonts.rs` 迁出）。
//!
//! # 为什么这几个用例搬到了这里
//!
//! 它们与 core 里其余字体测试的区别在于**断言的对象**：其余测试只验证
//! 「字体被挂进了族里」（`PortFontDefinitions` 的纯数据结构，egui 无关），
//! 而这四条验证「**egui 真的能据此渲染出字形**」——必须构造 `egui::Context`、
//! `set_fonts`、再问 `has_glyphs`。
//!
//! 基础设施端口化（V11-63）之后 core 不再依赖 egui，这四条在 core 里无处安放；
//! 而它们又只有**经过组合根适配器**（`to_egui_font_definitions`，端口 →
//! egui 类型的唯一转换点）才有意义。放到根 crate 的 `tests/presentation/` 正好：
//! 既能用 egui，也能用适配器，且与 `math_glyph_test.rs` / `s6_11f_test.rs`
//! 这些「真实字体配置下的字形核对」同类。
//!
//! # 迁出时保持的语义（不要"顺手优化"）
//!
//! - `set_fonts` 这一步本身就是断言：畸形字体在 `panic = "abort"` 下会直接
//!   终止进程，故它同时守护「随仓库分发的字体文件是好的」；
//! - 缺资产 / 缺系统 CJK 时**软跳过**并 `eprintln!`，不失败——那是环境问题，
//!   不是代码缺陷（`R-20` 验收判据：干净克隆删掉 `assets/fonts/` 后仍应通过）。

use rustmark::app::frame::to_egui_font_definitions;
use rustmark::infrastructure::fonts::{
    PortFontDefinitions, default_roots, discover_font_sources, extend_with_builtin_cjk,
    font_definitions,
};

/// 内置字体资产（`assets/fonts/`）是否随仓库存在。
///
/// 根 crate 的清单目录即工作区根，故直接 `assets/fonts`（core 侧的同一
/// 判据要上溯两级到 `../../assets/fonts`）。
fn builtin_font_assets_present() -> bool {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/fonts")
        .is_dir()
}

/// **决定性验证**：加载系统 CJK 字体后，中文确实有字形了。
///
/// 这是字体修复最核心的断言——core 里那些测试只验证"字体被挂进了族里"，
/// 而这条验证"egui 真的能据此渲染出中文字形"。
/// 两者之间有真实落差：字体文件可能解析失败（egui 底层 `skrifa` 只吃轮廓字体），
/// 那样挂上去也没用，甚至会 panic。
///
/// 本机无中文字体时**软跳过**（而非失败）：那是环境问题，不是代码缺陷，
/// 由 core 的 `a_cjk_font_is_available_on_this_platform` 负责在缺字体时给出信号。
#[test]
fn real_cjk_font_provides_glyphs_for_chinese() {
    let setup = font_definitions(None);
    if setup.cjk_source.is_none() {
        eprintln!("跳过：本机无中文字体，无法验证字形");
        return;
    }
    let source = setup.cjk_source.clone().expect("上面已判非空");

    let ctx = egui::Context::default();
    // `set_fonts` 会引发字体解析；若字体格式不被支持（如彩色 emoji 字体），
    // 这一步就会 panic——而本项目是 `panic = "abort"`，故这条测试同时守护了那个风险。
    ctx.set_fonts(to_egui_font_definitions(setup.definitions));
    ctx.run_ui(egui::RawInput::default(), |_| {})
        .drop_without_applying_deltas();

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

/// 端到端：`set_fonts` 之后，内置 `FontId::monospace` 真能排出选中字体的字形。
///
/// 这条钉住「方案可行」本身：置顶进内置族后，`FontId::monospace`
/// （编辑区 / 预览代码块实际用的构造）永远有绑定——不像自建族那样
/// 漏一处注册就 panic（`panic = "abort"` 下即窗口消失）。
#[test]
fn selected_font_renderable_through_builtin_monospace_font_id() {
    let Some(source) = discover_font_sources(&default_roots()).into_iter().next() else {
        eprintln!("跳过：本机未发现可选字体");
        return;
    };
    let setup = font_definitions(Some(&source.key));
    let ctx = egui::Context::default();
    ctx.set_fonts(to_egui_font_definitions(setup.definitions));
    ctx.run_ui(egui::RawInput::default(), |_| {})
        .drop_without_applying_deltas();
    // 中文仍须有字形（选中字体多半没有中文字形，靠 CJK 兜底）。
    let mono = egui::FontId::monospace(14.0);
    let has = ctx.fonts_mut(|f| f.has_glyphs(&mono, "中文编辑器"));
    assert!(
        has,
        "置顶 {} 后等宽族缺中文字形——CJK 兜底链被破坏",
        source.display_name
    );
}

/// 内置数学字体必须能被 egui 解析，且兜底真的接上。
///
/// 畸形字体在 `panic = "abort"` 下会直接终止进程，故 `set_fonts`
/// 这一步同时守护「随仓库分发的字体文件是好的」。
#[test]
fn embedded_math_font_parses_and_covers_symbols() {
    if !builtin_font_assets_present() {
        eprintln!("跳过：内置字体资产 assets/fonts 缺失");
        return;
    }
    let setup = font_definitions(None);
    let ctx = egui::Context::default();
    ctx.set_fonts(to_egui_font_definitions(setup.definitions));
    ctx.run_ui(egui::RawInput::default(), |_| {})
        .drop_without_applying_deltas();

    let font = egui::FontId::proportional(16.0);
    // 挑几个西文与中文字体都没有、只有数学字体有的符号。
    let has = ctx.fonts_mut(|f| f.has_glyphs(&font, "∀∅⇒⟨⨁ℜ"));
    assert!(
        has,
        "数学兜底未接上：常见数学符号仍缺字形（问题 3 的根因仍在）"
    );
    // 等宽族同样要接上（源码卡回退走等宽）。
    let mono = egui::FontId::monospace(16.0);
    let mono_has = ctx.fonts_mut(|f| f.has_glyphs(&mono, "∀∅⇒⟨⨁ℜ"));
    assert!(mono_has, "等宽族未接上数学兜底");
}

/// **决定性验证**：内置 CJK 子集**单独**就能渲染中文（不靠系统字体）。
///
/// 只用端口默认定义 + 内置子集装配（刻意不挂系统 CJK），模拟
/// 「精简 Linux / 最小化容器」环境：此时中文必须有字形。
/// 同时守护「随仓库分发的子集文件是好的」——畸形字体在
/// `panic = "abort"` 下会直接终止进程，`set_fonts` 即试金石。
///
/// 这也是 `extend_with_builtin_cjk`（core 侧 `pub`）存在的理由：`font_definitions`
/// 必然混入系统 CJK，构造不出"裸机"这个前提。
#[test]
fn builtin_cjk_subset_alone_provides_glyphs_for_chinese() {
    if !builtin_font_assets_present() {
        eprintln!("跳过：内置字体资产 assets/fonts 缺失");
        return;
    }
    let mut defs = PortFontDefinitions::default();
    extend_with_builtin_cjk(&mut defs);
    let ctx = egui::Context::default();
    ctx.set_fonts(to_egui_font_definitions(defs));
    ctx.run_ui(egui::RawInput::default(), |_| {})
        .drop_without_applying_deltas();

    // GB2312 常用汉字 + 中文标点（缺标点会让中文排版断在标点上）。
    let text = "中文编辑器文档预览测试，。、「」";
    for font in [
        egui::FontId::proportional(14.0),
        egui::FontId::monospace(14.0),
    ] {
        let has = ctx.fonts_mut(|f| f.has_glyphs(&font, text));
        assert!(
            has,
            "内置 CJK 子集应独立覆盖 GB2312 常用汉字与标点（{font:?}）"
        );
    }
}
