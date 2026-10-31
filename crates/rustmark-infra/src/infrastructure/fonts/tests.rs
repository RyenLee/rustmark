//! `fonts` 门面测试（从主文件内联 `mod tests` 迁出；R-07 Batch 0，零生产改动）。

use super::*;
use rustmark_core::shared::fonts::DEFAULT_EDITOR_FONT_KEY;

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

/// macOS bundle 布局：字体根必须解析到 `Contents/Resources/fonts`。
///
/// 字体若留在 `Contents/MacOS/fonts`，`codesign` 会把它当代码容器递归检查，
/// 对未签名的 `.ttf` 报 `code object is not signed at all` 并让发布失败。
#[test]
fn bundle_font_dir_is_resources_not_macos() {
    let exe = std::path::Path::new("/tmp/Rustmark.app/Contents/MacOS/rustmark");
    assert_eq!(
        discovery::bundle_resource_font_dir(exe),
        Some(std::path::PathBuf::from(
            "/tmp/Rustmark.app/Contents/Resources/fonts"
        )),
        "bundle 内字体应解析到 Contents/Resources/fonts"
    );
    // 非 bundle 布局（Linux / Windows 便携包）：不产生资源路径，
    // 继续沿用 exe 旁 `fonts/`，行为与改动前一致。
    assert_eq!(
        discovery::bundle_resource_font_dir(std::path::Path::new("/tmp/rustmark")),
        None
    );
    // 仅有 MacOS 目录名不够，必须完整的 Contents/MacOS 结构。
    assert_eq!(
        discovery::bundle_resource_font_dir(std::path::Path::new("/tmp/X/Contents/other/rustmark")),
        None
    );
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
/// `docs/archive/architecture-reviews/architecture-refactor-plan.md` 的 R-20 段。
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
