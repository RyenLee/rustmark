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

use crate::shared::fonts::{DEFAULT_EDITOR_FONT_KEY, EditorFontChoice};

/// 字体装配结果。
pub struct FontSetup {
    /// 可直接交给 `Context::set_fonts` 的字体定义。
    pub definitions: FontDefinitions,
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
    let mut definitions = FontDefinitions::default();

    // 发现并登记可选的编辑区字体（内置三款 + 项目 fonts/ 目录 + 少量常见系统等宽字体）。
    let sources = discover_font_sources();
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

/// 把用户选中的字体**置顶**进内置 `Monospace` 族（纯函数，便于单测）。
///
/// # 为什么是「置顶进内置族」而不是「自建一个族」
///
/// 自建族要求每一处排版前该族都已注册；而 egui 对未注册的
/// `FontFamily::Name` 是 **panic**（`epaint fonts.rs:808`），
/// 在 `panic = "abort"` 下即窗口消失。内置族永远有绑定，没有这个风险。
/// 完整说明见 `shared::fonts` 模块文档。
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
fn prepend_selected_to_monospace(definitions: &mut FontDefinitions, selected_family: Option<&str>) {
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

/// 一个已发现的、可供用户选择的编辑区字体源。
struct FontSource {
    /// egui 字体数据键（`user:<字体名>` 或 `builtin:<字体名>`，
    /// 与 [`EditorFontChoice::key`] 同值）。
    key: String,
    /// 下拉显示名（字体文件名去扩展名 / 内置字体名）。
    display_name: String,
    /// 字体字节的来源（磁盘路径或随仓库分发的内置资产）。
    bytes: FontBytes,
}

/// 字体字节的来源。
///
/// 区分两者是因为内置资产**没有路径**（`include_bytes!` 直接进二进制），
/// 而「只做存在性检查、不读内容」的旧约定只对磁盘候选成立。
enum FontBytes {
    /// 磁盘路径（项目 `fonts/` 目录或系统字体目录）。
    Path(PathBuf),
    /// 随仓库分发的内置资产（`assets/fonts/`，经 `include_bytes!` 编译进程序）。
    Static(&'static [u8]),
}

impl FontSource {
    /// 取出字体字节并包装成 egui 的 `FontData`；读不通返回 `None`。
    ///
    /// 内置资产走 [`FontData::from_static`]（零拷贝），磁盘候选读文件。
    fn font_data(&self) -> Option<Arc<FontData>> {
        match &self.bytes {
            FontBytes::Path(path) => std::fs::read(path)
                .ok()
                .map(|bytes| Arc::new(FontData::from_owned(bytes))),
            FontBytes::Static(bytes) => Some(Arc::new(FontData::from_static(bytes))),
        }
    }
}

/// 随仓库分发的内置等宽字体（`assets/fonts/`，许可证见同目录 `Fonts-LICENSE.txt`）。
///
/// # 为什么要内置，而不是只发现系统字体
///
/// 只发现系统字体意味着「设置页能选什么」取决于用户机器装了什么：
/// 多数 Windows 机器只有 Consolas / Cascadia，macOS 只有 Menlo，
/// 用户听说 JetBrains Mono 好用，下拉里却没有——想选也无从选起。
/// 内置三款（合计约 320KB）让三个平台都有一致的可选集，
/// 「编辑区观感」也不再随机器漂移。
///
/// # 为什么只有 Regular 一个字重
///
/// 语法高亮的加粗靠 `TextFormat` 的加粗标记表达，egui 不做合成加粗；
/// 缺 Bold 字重时加粗文本回落 Regular——这与系统 CJK 兜底是同一取舍。
/// 三款全字重会把体积推到 2MB 以上，不划算。
///
/// # 显示名即去重键
///
/// 用户机器上恰好装了同名字体时，内置那份赢（内容相同、来源确定），
/// 避免下拉里出现两个重复项——见 [`discover_font_sources`]。
const BUILTIN_EDITOR_FONTS: &[(&str, &str, &[u8])] = &[
    (
        "builtin:JetBrains Mono",
        "JetBrains Mono",
        include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    ),
    (
        "builtin:Fira Code",
        "Fira Code",
        include_bytes!("../../assets/fonts/FiraCode-Regular.ttf"),
    ),
    (
        "builtin:IBM Plex Mono",
        "IBM Plex Mono",
        include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf"),
    ),
];

/// 内置等宽字体清单（顺序即下拉顺序，按「综合口碑」排）。
fn builtin_editor_fonts() -> Vec<FontSource> {
    BUILTIN_EDITOR_FONTS
        .iter()
        .map(|(key, name, bytes)| FontSource {
            key: (*key).to_string(),
            display_name: (*name).to_string(),
            bytes: FontBytes::Static(bytes),
        })
        .collect()
}

/// 发现可选的编辑区字体：内置三款打头，其后是项目 `fonts/` 目录与常见系统等宽字体。
///
/// 只做**存在性**检查（不读内容、不解码）：真正读字节发生在
/// [`font_definitions`]（内置资产在那里经 `from_static` 零拷贝登记）。
fn discover_font_sources() -> Vec<FontSource> {
    let mut sources: Vec<FontSource> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    // 内置字体排最前：同名时内置赢——跨平台确定性优先于「用户机器上恰好装了什么」。
    for source in builtin_editor_fonts() {
        if seen.insert(source.display_name.clone()) {
            sources.push(source);
        }
    }
    for (path, name) in candidate_font_paths() {
        if !path.is_file() {
            continue;
        }
        let key = format!("user:{name}");
        // 按**显示名**去重（不是键）：用户装了与内置同名的字体时只保留内置那份，
        // 否则下拉里会出现两个「JetBrains Mono」，选哪个全凭运气。
        if seen.insert(name.clone()) {
            sources.push(FontSource {
                key,
                display_name: name,
                bytes: FontBytes::Path(path),
            });
        }
    }
    sources
}

/// 候选字体路径（项目 `fonts/` + 系统等宽），按优先级排序。
fn candidate_font_paths() -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    out.extend(scan_project_fonts_dir());
    out.extend(system_mono_candidates());
    out
}

/// 扫描项目级 `fonts/` 目录：用户自置字体的入口，优先于系统字体。
///
/// 查找两个位置（都存在时都扫，项目根优先）：
/// 1. 可执行文件旁的 `fonts/`（安装态：随程序分发的字体）；
/// 2. 当前工作目录的 `fonts/`（开发态：`cargo run` 的工作目录即项目根）。
fn scan_project_fonts_dir() -> Vec<(PathBuf, String)> {
    const EXTS: [&str; 3] = ["ttf", "otf", "ttc"];
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        dirs.push(dir.join("fonts"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("fonts"));
    }
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .is_some_and(|e| EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
            })
            .collect();
        // 排序让下拉顺序稳定（`read_dir` 顺序随文件系统而变）。
        files.sort();
        for path in files {
            let Some(stem) = path.file_stem() else {
                continue;
            };
            let name = stem.to_string_lossy().into_owned();
            if !name.trim().is_empty() {
                out.push((path, name));
            }
        }
    }
    out
}

/// 常见**等宽**字体候选（不收录彩色 emoji 字体，理由见模块文档的安全约束）。
#[cfg(target_os = "windows")]
fn system_mono_candidates() -> Vec<(PathBuf, String)> {
    let fonts = std::env::var_os("WINDIR")
        .map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from)
        .join("Fonts");
    // 新版 Cascadia 常按用户安装到 `%LOCALAPPDATA%\Microsoft\Windows\Fonts`。
    let user_fonts = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Microsoft\\Windows\\Fonts");
    [
        (fonts.join("consola.ttf"), "Consolas"),
        (fonts.join("lucon.ttf"), "Lucida Console"),
        (user_fonts.join("CascadiaMono.ttf"), "Cascadia Mono"),
        (user_fonts.join("CascadiaCode.ttf"), "Cascadia Code"),
    ]
    .into_iter()
    .map(|(path, name)| (path, name.to_string()))
    .collect()
}

/// macOS：Menlo 是终端与 Xcode 的默认等宽字体。
#[cfg(target_os = "macos")]
fn system_mono_candidates() -> Vec<(PathBuf, String)> {
    [
        ("/System/Library/Fonts/Menlo.ttc", "Menlo"),
        (
            "/Applications/Utilities/Terminal.app/Contents/Resources/Fonts/SFMono-Regular.otf",
            "SF Mono",
        ),
    ]
    .into_iter()
    .map(|(path, name)| (PathBuf::from(path), name.to_string()))
    .collect()
}

/// Linux：发行版差异大，列最常见的等宽字体安装路径。
#[cfg(all(unix, not(target_os = "macos")))]
fn system_mono_candidates() -> Vec<(PathBuf, String)> {
    [
        (
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "DejaVu Sans Mono",
        ),
        (
            "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
            "Liberation Mono",
        ),
        (
            "/usr/share/fonts/truetype/ubuntu/UbuntuMono-R.ttf",
            "Ubuntu Mono",
        ),
        (
            "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
            "Noto Sans Mono",
        ),
    ]
    .into_iter()
    .map(|(path, name)| (PathBuf::from(path), name.to_string()))
    .collect()
}

/// 把发现的字体源映射为设置页下拉项（始终以「系统等宽（默认）」打头）。
fn to_choices(sources: &[FontSource]) -> Vec<EditorFontChoice> {
    let mut choices = vec![EditorFontChoice {
        key: DEFAULT_EDITOR_FONT_KEY.to_string(),
        display_name: "系统等宽（默认）".to_string(),
    }];
    for s in sources {
        choices.push(EditorFontChoice {
            key: s.key.clone(),
            display_name: s.display_name.clone(),
        });
    }
    choices
}

/// 轻量版清单：只发现字体（存在性检查），不读字节、不装配。
///
/// 供应用启动时填充设置页下拉用；完整装配走 [`font_definitions`]。
/// 分开是为了避免「为了列个下拉就把所有字体文件读两遍」。
pub fn editor_font_choices() -> Vec<EditorFontChoice> {
    to_choices(&discover_font_sources())
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
///
/// # 完整的兜底链（`font_definitions` 的装配顺序）
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

/// 内置数学兜底字体（TeX Gyre DJV Math，DejaVu 的数学扩展版）。
///
/// # 为什么必须内置，而不能指望系统字体
///
/// egui 自带的两套字体（比例族 Ubuntu-Light、等宽族 Hack）与常见中文字体
/// （雅黑 / 苹方 / Noto CJK）**都缺数学运算符区**（U+2200–22FF 与
/// U+2A00–2AFF）。实测 67 个常用符号（`\forall` `\emptyset` `\Rightarrow`
/// `\langle` `\bigoplus` …）在西文与中文字体里**全部落空**，预览里就是一屏
/// 方框——不报错、不崩溃，肉眼极难归因（问题 3「数学符号不齐缺失」的根因）。
///
/// 系统里并非没有数学字体（Windows 有 Cambria Math），但**依赖它等于把
/// 正确性绑到操作系统上**：Linux CI / 极简容器里没有，macOS 没有公开路径，
/// 于是同一份代码在不同机器上表现不同，字形覆盖测试也无法作为回归守住。
/// 内置一份则所有平台一致。
///
/// # 体积取舍
///
/// 578KB 换「数学公式在所有平台都能排出来」。考虑到替代方案是内嵌一个
/// JS 引擎（+100MB 量级，已被 ADR 否决）或引入 resvg（带 C 依赖），
/// 这个体积是同一目标下的最小代价。**不做子集化**：符号表由排版引擎
/// （`views/math/symbols.rs`）逐符号驱动，子集必须与它一一对应，
/// 漏一个符号就是一处方框——而那正是本函数要消灭的故障形态。
/// 整字体 + 全符号覆盖测试（`tests/presentation/math_glyph_test.rs`）更稳。
///
/// # 许可证
///
/// Bitstream Vera / TeX Gyre DJV Math 许可证允许随软件再分发（须附许可证），
/// 见 `assets/fonts/DejaVuMathTeXGyre-LICENSE.txt`。
const MATH_FALLBACK_TTF: &[u8] = include_bytes!("../../assets/fonts/DejaVuMathTeXGyre.ttf");

/// 数学兜底字体在 `font_data` 中的键名。
pub const MATH_FALLBACK_KEY: &str = "math-fallback";

/// 把内置数学兜底字体挂进两个族（位置在 CJK 之前，见 [`extend_with_cjk`] 的链图）。
fn extend_with_math_fallback(definitions: &mut FontDefinitions) {
    definitions.font_data.insert(
        MATH_FALLBACK_KEY.to_owned(),
        Arc::new(FontData::from_static(MATH_FALLBACK_TTF)),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push(MATH_FALLBACK_KEY.to_owned());
    }
}

/// CJK 字体在 `font_data` 中的键名。
pub const CJK_KEY: &str = "cjk";

/// 内置 CJK 兜底字体（Noto Sans SC 的 GB2312 子集，1.5MB）。
///
/// # 为什么内置 CJK 兜底，而不是继续只依赖系统字体
///
/// 原实现只找系统 CJK 字体（雅黑 / 苹方 / Noto CJK…），意味着
/// **精简 Linux、最小化容器、部分 CI 镜像里中文全是方框**——而这正是
/// 「切主题后编辑区乱码」类问题在最难复现的环境里的形态。内置一份子集后，
/// 所有平台都至少有 GB2312 覆盖，不再看操作系统的脸色。
///
/// # 为什么是 GB2312 子集而不是整字体
///
/// 整字体 8.5MB、子集 1.5MB。差额来自 GB2312 之外的汉字（繁体、扩展区）
/// 与用不到的字重——而那些字符在**有系统中文字体的机器上**仍由系统字体
/// 渲染（内置子集排在系统 CJK 之后，见 [`extend_with_cjk`] 的链图），
/// 所以子集化不损失实际观感，只决定「裸机」上的覆盖下限。
/// GB2312 = 6763 汉字 + 1342 符号，是「日常中文不出方框」的经典底线。
///
/// # 子集化方法（可复现）
///
/// ```text
/// pyftsubset NotoSansSC-Regular.ttf --text-file=gb2312.txt \
///   --no-hinting --layout-features='' --name-IDs=0,1,2,3,4,5,6
/// ```
///
/// `gb2312.txt` 由 GB2312 双字节码位经 Python `gb2312` 编解码器枚举而成
/// （94×94 网格里能解出的字符，共 8105 个）。未修改任何字形。
///
/// # 许可证
///
/// SIL OFL-1.1，允许随软件再分发。版权与来源见
/// `assets/fonts/Fonts-LICENSE.txt` 第 4 节。
const BUILTIN_CJK_TTF: &[u8] = include_bytes!("../../assets/fonts/NotoSansSC-Subset.ttf");

/// 内置 CJK 兜底在 `font_data` 中的键名。
pub const BUILTIN_CJK_KEY: &str = "builtin-cjk";

/// 把内置 CJK 子集挂进两个族，位置在**最末**（系统 CJK 之后）。
///
/// 无条件挂载：它轻量、只服务「系统什么中文字体都没有」的场景，
/// 有系统字体时永远排在后排，不抢渲染。
fn extend_with_builtin_cjk(definitions: &mut FontDefinitions) {
    definitions.font_data.insert(
        BUILTIN_CJK_KEY.to_owned(),
        Arc::new(FontData::from_static(BUILTIN_CJK_TTF)),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push(BUILTIN_CJK_KEY.to_owned());
    }
}

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
        let setup = font_definitions(None);
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
        let setup = font_definitions(None);
        if setup.cjk_source.is_none() {
            eprintln!("跳过：本机无中文字体，无法验证字形");
            return;
        }
        let source = setup.cjk_source.clone().expect("上面已判非空");

        let ctx = egui::Context::default();
        // `set_fonts` 会引发字体解析；若字体格式不被支持（如彩色 emoji 字体），
        // 这一步就会 panic——而本项目是 `panic = "abort"`，故这条测试同时守护了那个风险。
        ctx.set_fonts(setup.definitions);
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

    /// 默认等宽族清单（编辑区族装配的基准）。
    fn default_mono_family() -> Vec<String> {
        FontDefinitions::default()
            .families
            .get(&FontFamily::Monospace)
            .cloned()
            .expect("egui 默认定义应含等宽族")
    }

    /// 哨兵（「系统等宽」）必须让等宽族与内置**逐元素一致**。
    ///
    /// 这是「不选字体时观感与改造前完全相同」的形式化保证：早期
    /// `Theme::editor_font_family` 是死设置，编辑区一直走内置等宽族；
    /// 默认值若与它有半点差别，就是一次无意的视觉回退。
    #[test]
    fn sentinel_selection_keeps_builtin_monospace_family() {
        let mut defs = FontDefinitions::default();
        let mono = default_mono_family();
        prepend_selected_to_monospace(&mut defs, Some(DEFAULT_EDITOR_FONT_KEY));
        prepend_selected_to_monospace(&mut defs, None);
        assert_eq!(
            defs.families.get(&FontFamily::Monospace),
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
        let mut defs = FontDefinitions::default();
        defs.font_data.insert(
            "user:Consolas".to_owned(),
            std::sync::Arc::new(FontData::from_owned(vec![0u8; 8])),
        );
        let mono = default_mono_family();
        for junk in ["", "   ", "sans-serif", "serif", "system-ui", "user:缺失"] {
            prepend_selected_to_monospace(&mut defs, Some(junk));
            assert_eq!(
                defs.families.get(&FontFamily::Monospace),
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
        let mut defs = FontDefinitions::default();
        defs.font_data.insert(
            "user:Consolas".to_owned(),
            std::sync::Arc::new(FontData::from_owned(vec![0u8; 8])),
        );
        let mono = default_mono_family();
        prepend_selected_to_monospace(&mut defs, Some("user:Consolas"));

        let family = defs
            .families
            .get(&FontFamily::Monospace)
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
        let mut defs = FontDefinitions::default();
        let builtin = default_mono_family()
            .first()
            .cloned()
            .expect("内置等宽族不应为空");
        defs.font_data.insert(
            builtin.clone(),
            std::sync::Arc::new(FontData::from_owned(vec![0u8; 8])),
        );
        prepend_selected_to_monospace(&mut defs, Some(&builtin));

        let family = defs
            .families
            .get(&FontFamily::Monospace)
            .expect("等宽族应仍在");
        assert_eq!(family.first(), Some(&builtin));
        assert_eq!(
            family.iter().filter(|k| *k == &builtin).count(),
            1,
            "已在族中的字体被选中时不应重复"
        );
    }

    /// 端到端：`set_fonts` 之后，内置 `FontId::monospace` 真能排出选中字体的字形。
    ///
    /// 这条钉住「方案可行」本身：置顶进内置族后，`FontId::monospace`
    /// （编辑区 / 预览代码块实际用的构造）永远有绑定——不像自建族那样
    /// 漏一处注册就 panic（`panic = "abort"` 下即窗口消失）。
    #[test]
    fn selected_font_renderable_through_builtin_monospace_font_id() {
        let Some(source) = discover_font_sources().into_iter().next() else {
            eprintln!("跳过：本机未发现可选字体");
            return;
        };
        let setup = font_definitions(Some(&source.key));
        let ctx = egui::Context::default();
        ctx.set_fonts(setup.definitions);
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
    /// 内置资产没有路径（`include_bytes!` 进二进制），按字节非空校验。
    #[test]
    fn discover_font_sources_only_returns_existing_files() {
        for source in discover_font_sources() {
            match &source.bytes {
                FontBytes::Path(path) => {
                    assert!(path.is_file(), "发现了不存在的字体文件：{:?}", path)
                }
                FontBytes::Static(bytes) => assert!(
                    !bytes.is_empty(),
                    "内置字体资产不应为空：{}",
                    source.display_name
                ),
            }
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
        let sources = discover_font_sources();
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
        let mut defs = FontDefinitions::default();
        extend_with_math_fallback(&mut defs);
        extend_with_cjk(&mut defs, vec![0u8; 8]);
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
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

    /// 内置数学字体必须能被 egui 解析，且兜底真的接上。
    ///
    /// 畸形字体在 `panic = "abort"` 下会直接终止进程，故 `set_fonts`
    /// 这一步同时守护「随仓库分发的字体文件是好的」。
    #[test]
    fn embedded_math_font_parses_and_covers_symbols() {
        let setup = font_definitions(None);
        let ctx = egui::Context::default();
        ctx.set_fonts(setup.definitions);
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

    /// 内置三款等宽字体必须已登记、且在设置页下拉里（哨兵之后排最前）。
    ///
    /// 这条钉住「跨平台一定有得选」：只发现系统字体的旧实现下，
    /// 用户机器没装 JetBrains Mono 就永远选不到它。
    #[test]
    fn builtin_editor_fonts_are_registered_and_selectable() {
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
        let mut defs = FontDefinitions::default();
        extend_with_math_fallback(&mut defs);
        extend_with_cjk(&mut defs, vec![0u8; 8]);
        extend_with_builtin_cjk(&mut defs);
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
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

    /// **决定性验证**：内置 CJK 子集**单独**就能渲染中文（不靠系统字体）。
    ///
    /// 只用默认定义 + 内置子集装配（刻意不挂系统 CJK），模拟
    /// 「精简 Linux / 最小化容器」环境：此时中文必须有字形。
    /// 同时守护「随仓库分发的子集文件是好的」——畸形字体在
    /// `panic = "abort"` 下会直接终止进程，`set_fonts` 即试金石。
    #[test]
    fn builtin_cjk_subset_alone_provides_glyphs_for_chinese() {
        let mut defs = FontDefinitions::default();
        extend_with_builtin_cjk(&mut defs);
        let ctx = egui::Context::default();
        ctx.set_fonts(defs);
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
}
