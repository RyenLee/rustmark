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

use crate::shared::fonts::EditorFontChoice;

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
mod tests {
    use super::*;

    use crate::shared::fonts::DEFAULT_EDITOR_FONT_KEY;

    /// 内置字体资产（`assets/fonts/`）是否随仓库存在。
    ///
    /// 干净克隆删除该目录后，依赖内置字体文件的测试应**跳过**而非失败
    /// （见 R-20 验收判据）；本机有系统 CJK 时其余逻辑仍由系统字体兜底。
    fn builtin_font_assets_present() -> bool {
        // D-3 阶段 4a 拆分后本 crate 的清单目录是 `crates/rustmark-core`，
        // 仓库资产仍在根目录的 `assets/fonts/`，故上两级再进入。
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/fonts")
            .is_dir()
    }

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
        let mut defs = base_defs();
        let before: Vec<_> = defs
            .families
            .get(&PortFontFamily::Proportional)
            .expect("默认应有比例族")
            .clone();

        extend_with_cjk(&mut defs, vec![0u8; 8]);

        let after = defs
            .families
            .get(&PortFontFamily::Proportional)
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
        let mut defs = PortFontDefinitions::default();
        extend_with_cjk(&mut defs, vec![0u8; 8]);

        for family in [PortFontFamily::Proportional, PortFontFamily::Monospace] {
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
        let defs = PortFontDefinitions::default();
        assert!(
            defs.families.contains_key(&PortFontFamily::Proportional),
            "缺字体时默认族必须仍在，否则界面完全无法渲染"
        );
        // 组装函数本身不应 panic（该函数内部可能走到"没找到字体"分支）。
        let setup = font_definitions(None);
        assert!(
            setup
                .definitions
                .families
                .contains_key(&PortFontFamily::Monospace)
        );
    }

    /// 装好基础族的起点。
    ///
    /// REL-03 后裸 `PortFontDefinitions::default()` 是空族——前缀 / 顺序 /
    /// 「族不为空」类断言都要以基础族为基准才有意义（空族下要么空转、
    /// 要么把「西文在前」这类不变式断言成假）。
    fn base_defs() -> PortFontDefinitions {
        let mut defs = PortFontDefinitions::default();
        install_egui_base_fonts(&mut defs);
        defs
    }

    /// 默认等宽族清单（编辑区族装配的基准）——与 `font_definitions`
    /// 的起点一致：先装回 REL-03 外置的基础族。
    fn default_mono_family() -> Vec<String> {
        base_defs()
            .families
            .get(&PortFontFamily::Monospace)
            .cloned()
            .expect("默认定义应含等宽族")
    }

    /// 哨兵（「系统等宽」）必须让等宽族与内置**逐元素一致**。
    ///
    /// 这是「不选字体时观感与改造前完全相同」的形式化保证：早期
    /// `Theme::editor_font_family` 是死设置，编辑区一直走内置等宽族；
    /// 默认值若与它有半点差别，就是一次无意的视觉回退。
    #[test]
    fn sentinel_selection_keeps_builtin_monospace_family() {
        let mut defs = base_defs();
        let mono = default_mono_family();
        prepend_selected_to_monospace(&mut defs, Some(DEFAULT_EDITOR_FONT_KEY));
        prepend_selected_to_monospace(&mut defs, None);
        assert_eq!(
            defs.families.get(&PortFontFamily::Monospace),
            Some(&mono),
            "选默认哨兵 / 不选时，等宽族必须与内置逐元素一致"
        );
    }

    /// 旧配置里的非法字体名（CSS 通用名 / 空串 / 未登记的键）必须安全退化。
    ///
    /// 表现是「改了没生效」的那类 bug 的防守：用户手里可能有一份
    /// 写着 `sans-serif` 的历史配置，此时应静默用回内置等宽族，
    /// 而不是把 `sans-serif` 塞进族里（egui 找不到该字体数据会怎样？
    /// ——自建族场景下会整体 panic，见 `shared::fonts` 的说明）。
    #[test]
    fn legacy_or_unknown_key_falls_back_to_monospace() {
        let mut defs = base_defs();
        defs.font_data.insert(
            "user:Consolas".to_owned(),
            PortFontData::from_owned(vec![0u8; 8]),
        );
        let mono = default_mono_family();
        for junk in ["", "   ", "sans-serif", "serif", "system-ui", "user:缺失"] {
            prepend_selected_to_monospace(&mut defs, Some(junk));
            assert_eq!(
                defs.families.get(&PortFontFamily::Monospace),
                Some(&mono),
                "非法键 {junk:?} 应原样保留内置等宽族，而不是把无效键塞进去"
            );
        }
    }

    /// 选中真实字体时：它**置顶**，其余成员顺序不变，且不重复。
    ///
    /// 置顶（而非追加）是这次修复的全部意义——追加的话西文仍由
    /// Hack / Ubuntu-Light 渲染，用户在设置里选了也看不见。
    #[test]
    fn selected_font_is_prepended_without_duplicates() {
        let mut defs = base_defs();
        defs.font_data.insert(
            "user:Consolas".to_owned(),
            PortFontData::from_owned(vec![0u8; 8]),
        );
        let mono = default_mono_family();
        prepend_selected_to_monospace(&mut defs, Some("user:Consolas"));

        let family = defs
            .families
            .get(&PortFontFamily::Monospace)
            .expect("等宽族应仍在");
        assert_eq!(
            family.first().map(String::as_str),
            Some("user:Consolas"),
            "选中字体必须置顶，否则用户的选择看不见"
        );
        // 其余成员保持内置顺序（CJK 兜底仍在末尾）。
        assert_eq!(&family[1..], mono.as_slice(), "原有成员顺序不应改变");
        let mut sorted = family.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), family.len(), "族内不应有重复字体");
    }

    /// 选中键本就内置在等宽族里时，去重后仍只出现一次且在首位。
    #[test]
    fn selecting_an_already_builtin_font_does_not_duplicate() {
        if !builtin_font_assets_present() {
            eprintln!("跳过：内置字体资产 assets/fonts 缺失");
            return;
        }
        let mut defs = base_defs();
        let builtin = default_mono_family()
            .first()
            .cloned()
            .expect("内置等宽族不应为空");
        defs.font_data
            .insert(builtin.clone(), PortFontData::from_owned(vec![0u8; 8]));
        prepend_selected_to_monospace(&mut defs, Some(&builtin));

        let family = defs
            .families
            .get(&PortFontFamily::Monospace)
            .expect("等宽族应仍在");
        assert_eq!(family.first(), Some(&builtin));
        assert_eq!(
            family.iter().filter(|k| *k == &builtin).count(),
            1,
            "已在族中的字体被选中时不应重复"
        );
    }

    /// 下拉清单必须以「系统等宽（默认）」哨兵打头。
    ///
    /// 没有默认项时，用户配了一个本机不存在的字体键后将**无法**通过下拉
    /// 回到默认状态（文本框能改，但下拉里看不到那个选项）。
    #[test]
    fn choices_always_lead_with_default_sentinel() {
        let setup = font_definitions(None);
        let first = setup
            .editor_fonts
            .first()
            .expect("下拉清单不应为空（至少含默认项）");
        assert_eq!(first.key, DEFAULT_EDITOR_FONT_KEY);
        let keys: Vec<&str> = setup.editor_fonts.iter().map(|c| c.key.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len(), "下拉里不应出现重复字体");
    }

    /// 字体发现只收录**真实存在**的字体，且键唯一（项目 fonts/ 与系统字体
    /// 可能同名，先发现者赢）。
    ///
    /// 内置字体已改为文件分发，同样按「路径存在」校验。
    #[test]
    fn discover_font_sources_only_returns_existing_files() {
        if !builtin_font_assets_present() {
            eprintln!("跳过：内置字体资产 assets/fonts 缺失");
            return;
        }
        for source in discover_font_sources(&default_roots()) {
            assert!(
                source.path.is_file(),
                "发现了不存在的字体文件：{:?}",
                source.path
            );
            assert!(
                source.key.starts_with("user:") || source.key.starts_with("builtin:"),
                "字体键应带 user:/builtin: 前缀以便与哨兵区分：{}",
                source.key
            );
            assert!(
                !source.display_name.trim().is_empty(),
                "显示名不应为空：{}",
                source.display_name
            );
        }
        let sources = discover_font_sources(&default_roots());
        let keys: Vec<&str> = sources.iter().map(|s| s.key.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len(), "发现的字体键必须唯一");
        // 内置三款必须进入发现清单——它们是「设置页一定有得选」的保证。
        for (key, _, _) in BUILTIN_EDITOR_FONTS {
            assert!(
                sources.iter().any(|s| s.key == *key),
                "内置字体 {key} 未进入发现清单，跨平台可选性被破坏"
            );
        }
    }

    /// 兜底链顺序：西文 → 数学兜底 → CJK。
    ///
    /// 数学兜底若排到 CJK 之后，中文字体自带的数学字形（雅黑有 ≤ ≥ ≠ 之类）
    /// 会先命中，同一页公式里出现两种形状的符号；排到最前则会抢西文。
    #[test]
    fn math_fallback_sits_between_western_and_cjk() {
        if !builtin_font_assets_present() {
            eprintln!("跳过：内置字体资产 assets/fonts 缺失");
            return;
        }
        let mut defs = base_defs();
        extend_with_math_fallback(&mut defs);
        extend_with_cjk(&mut defs, vec![0u8; 8]);
        for family in [PortFontFamily::Proportional, PortFontFamily::Monospace] {
            let list = defs.families.get(&family).expect("该族应存在");
            let math_at = list
                .iter()
                .position(|k| k == MATH_FALLBACK_KEY)
                .expect("数学兜底应已挂载");
            let cjk_at = list
                .iter()
                .position(|k| k == CJK_KEY)
                .expect("CJK 应已挂载");
            assert!(
                math_at < cjk_at,
                "{family:?}：数学兜底必须排在 CJK 之前，否则符号形状会混搭"
            );
            assert!(math_at > 0, "{family:?}：数学兜底不能抢在西文字体前面");
        }
    }

    /// 内置三款等宽字体必须已登记、且在设置页下拉里（哨兵之后排最前）。
    ///
    /// 这条钉住「跨平台一定有得选」：只发现系统字体的旧实现下，
    /// 用户机器没装 JetBrains Mono 就永远选不到它。
    #[test]
    fn builtin_editor_fonts_are_registered_and_selectable() {
        if !builtin_font_assets_present() {
            eprintln!("跳过：内置字体资产 assets/fonts 缺失");
            return;
        }
        let setup = font_definitions(None);
        for (key, name, _) in BUILTIN_EDITOR_FONTS {
            assert!(
                setup.definitions.font_data.contains_key(*key),
                "内置字体 {name} 未登记进 font_data，选了会是空键"
            );
            assert!(
                setup
                    .editor_fonts
                    .iter()
                    .any(|c| c.key == *key && c.display_name == *name),
                "内置字体 {name} 未出现在设置页下拉"
            );
        }
        let after_sentinel: Vec<&str> = setup
            .editor_fonts
            .iter()
            .skip(1)
            .take(3)
            .map(|c| c.display_name.as_str())
            .collect();
        assert_eq!(
            after_sentinel,
            ["JetBrains Mono", "Fira Code", "IBM Plex Mono"],
            "内置三款应排在下拉最前（哨兵之后），与 BUILTIN_EDITOR_FONTS 顺序一致"
        );
    }

    /// 内置 CJK 子集必须排在系统 CJK **之后**（两个族都是）。
    ///
    /// 顺序反了会让装了雅黑的用户也被迫用子集——生僻字反而变方框，
    /// 且观感回退。这条与 `math_fallback_sits_between_western_and_cjk`
    /// 共同钉住整条兜底链：西文 → 数学 → 系统 CJK → 内置 CJK。
    #[test]
    fn builtin_cjk_sits_after_system_cjk_in_both_families() {
        if !builtin_font_assets_present() {
            eprintln!("跳过：内置字体资产 assets/fonts 缺失");
            return;
        }
        let mut defs = PortFontDefinitions::default();
        extend_with_math_fallback(&mut defs);
        extend_with_cjk(&mut defs, vec![0u8; 8]);
        extend_with_builtin_cjk(&mut defs);
        for family in [PortFontFamily::Proportional, PortFontFamily::Monospace] {
            let list = defs.families.get(&family).expect("该族应存在");
            assert_eq!(
                list.last().map(String::as_str),
                Some(BUILTIN_CJK_KEY),
                "{family:?}：内置 CJK 子集必须排在兜底链最末"
            );
            let cjk_at = list
                .iter()
                .position(|k| k == CJK_KEY)
                .expect("系统 CJK 应已挂载");
            let builtin_at = list
                .iter()
                .position(|k| k == BUILTIN_CJK_KEY)
                .expect("内置 CJK 应已挂载");
            assert!(
                builtin_at > cjk_at,
                "{family:?}：内置子集不能抢在系统 CJK 之前"
            );
        }
    }

    /// **R-20 注入式单测**：发现逻辑必须接受注入的根目录，
    /// 且项目 fonts 目录优先于系统等宽候选。
    ///
    /// 这条把「发现」从硬编码路径里解放出来——删掉 `assets/fonts/` 的干净克隆
    /// 也能验证优先级，而无须依赖真实字体文件。变异与回退场景见
    /// `docs/architecture-refactor-plan.md` 的 R-20 段。
    #[test]
    fn discovery_prefers_project_fonts_dir_over_system_candidates() {
        let dir = tempfile::tempdir().expect("应可创建临时目录");
        let path = dir.path().join("ProjectPriorityMono.ttf");
        std::fs::write(&path, b"fake-font-bytes").expect("应可写入假字体");
        let sources = discover_font_sources(&[dir.path().to_path_buf()]);
        let project = sources
            .iter()
            .find(|s| s.display_name == "ProjectPriorityMono")
            .expect("注入的项目 fonts 目录里的字体应被发现");
        assert!(
            project.key.starts_with("user:"),
            "项目字体键应以 user: 前缀：{}",
            project.key
        );
        // 项目字体须排在系统等宽候选之前（若存在）。
        let sys_names: Vec<String> = system_mono_candidates()
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        if let Some(sys_idx) = sources
            .iter()
            .position(|s| sys_names.iter().any(|n| n == &s.display_name))
        {
            let proj_idx = sources
                .iter()
                .position(|s| s.display_name == "ProjectPriorityMono")
                .expect("上面已确认存在");
            assert!(
                proj_idx < sys_idx,
                "项目字体须优先于系统等宽候选：项目@{proj_idx} 系统@{sys_idx}"
            );
        }
    }
}
