//! 字体发现：定位「机器上有哪些可选编辑区字体 / CJK 候选 / 数学兜底文件」。
//!
//! 本模块只做**路径与存在性**层面的发现，不读字节、不解码、不装配
//! （装配见 `assembly.rs`，门面组装见 `fonts.rs`）。发现逻辑接受**注入的根目录
//! 列表**（`discover_font_sources(roots)`），真实文件系统只留一层薄适配
//! `default_roots()`，从而可在无真实资产的环境下用临时目录单测优先级。
//!
//! 背景与体积红线约定见 `fonts.rs` 模块文档。

use std::path::PathBuf;
use std::sync::Arc;

use egui::FontData;

use crate::shared::fonts::{DEFAULT_EDITOR_FONT_KEY, EditorFontChoice};

/// 一个已发现的、可供用户选择的编辑区字体源。
pub struct FontSource {
    /// egui 字体数据键（`user:<字体名>` 或 `builtin:<字体名>`，
    /// 与 [`EditorFontChoice::key`] 同值）。
    pub key: String,
    /// 下拉显示名（字体文件名去扩展名 / 内置字体名）。
    pub display_name: String,
    /// 字体文件路径（内置 `fonts/`、项目 `fonts/` 目录或系统字体目录）。
    ///
    /// 内置字体不再 `include_bytes!` 进二进制（体积红线 NFR-PERF-01），
    /// 改为随发行包以文件形式分发（见 [`builtin_font_path`]），
    /// 故此处统一为磁盘路径、只做「存在性检查」。
    pub path: PathBuf,
}

impl FontSource {
    /// 取出字体字节并包装成 egui 的 `FontData`；读不通返回 `None`。
    pub fn font_data(&self) -> Option<Arc<FontData>> {
        std::fs::read(&self.path)
            .ok()
            .map(|bytes| Arc::new(FontData::from_owned(bytes)))
    }
}

/// 定位一个内置字体文件，返回可读的路径；找不到返回 `None`。
///
/// 按顺序查找（先到先得）：
/// 1. 可执行文件旁的 `fonts/`（安装态：随发行包分发的字体）；
/// 2. 当前工作目录的 `fonts/`（开发态：`cargo run` 的工作目录即项目根）；
/// 3. 编译期清单目录下的 `assets/fonts/`（开发 / 测试态：源码树）。
///
/// 内置字体随发行包分发，缺失时降级为「下拉里少一项 / 数学符号或裸机
/// 中文回落到系统字体」，不阻断启动（`font_definitions` 的调用方只依据
/// `cjk_source.is_none()` 记一条警告）。
pub fn builtin_font_path(file: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        dirs.push(dir.join("fonts"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("fonts"));
    }
    // `env!` 在编译期展开为清单目录；发行包在用户机器上不存在该路径，
    // 但它是最后兜底、只在文件真存在时命中，因此无副作用。
    // D-3 阶段 4a 拆分后清单目录是 `crates/rustmark-core`，仓库资产在根目录，
    // 故上两级再进入（命中条件与拆分前完全一致：仅仓库内开发构建存在）。
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts"));
    dirs.into_iter().map(|d| d.join(file)).find(|p| p.is_file())
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
///
/// # 为什么是文件名而非字节
///
/// 体积红线（`NFR-PERF-01`）下，内置字体**不再 `include_bytes!` 进二进制**，
/// 而是随发行包以文件分发（`metrics.md §2.4` 方案 1）。第三列是文件名，
/// 运行期经 [`builtin_font_path`] 定位、按需读取。
pub const BUILTIN_EDITOR_FONTS: &[(&str, &str, &str)] = &[
    (
        "builtin:JetBrains Mono",
        "JetBrains Mono",
        "JetBrainsMono-Regular.ttf",
    ),
    ("builtin:Fira Code", "Fira Code", "FiraCode-Regular.ttf"),
    (
        "builtin:IBM Plex Mono",
        "IBM Plex Mono",
        "IBMPlexMono-Regular.ttf",
    ),
];

/// 内置等宽字体清单（顺序即下拉顺序，按「综合口碑」排）。
///
/// 只收录**文件可解析**的项：发行包里缺了某个字体文件时，该字体
/// 既不登记进 `font_data`，也不出现在下拉里（避免「选了却没生效」）。
pub fn builtin_editor_fonts() -> Vec<FontSource> {
    BUILTIN_EDITOR_FONTS
        .iter()
        .filter_map(|(key, name, file)| {
            builtin_font_path(file).map(|path| FontSource {
                key: (*key).to_string(),
                display_name: (*name).to_string(),
                path,
            })
        })
        .collect()
}

/// 默认的项目字体根目录（安装态 exe 旁 `fonts/` + 开发态 cwd `fonts/`）。
///
/// 真实文件系统的薄适配：[`discover_font_sources`] 接受注入的根目录列表，
/// 此处给出生产环境的默认根，使发现逻辑可在无真实资产时单测。
pub fn default_roots() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        dirs.push(dir.join("fonts"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("fonts"));
    }
    dirs
}

/// 发现可选的编辑区字体：内置三款打头，其后是注入根目录的项目 `fonts/` 与常见系统等宽字体。
///
/// 只做**存在性**检查（不读内容、不解码）：真正读字节发生在
/// [`assembly::font_definitions`]（那里经 [`FontSource::font_data`] 按需读文件）。
///
/// `roots` 为注入的项目字体根目录列表（生产环境传 [`default_roots`]）；
/// 项目字体排在系统等宽候选之前——用户自置字体优先于「机器刚好装了什么」。
pub fn discover_font_sources(roots: &[PathBuf]) -> Vec<FontSource> {
    let mut sources: Vec<FontSource> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    // 内置字体排最前：同名时内置赢——跨平台确定性优先于「用户机器上恰好装了什么」。
    for source in builtin_editor_fonts() {
        if seen.insert(source.display_name.clone()) {
            sources.push(source);
        }
    }
    for (path, name) in candidate_font_paths(roots) {
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
                path,
            });
        }
    }
    sources
}

/// 候选字体路径（注入根目录的项目 `fonts/` + 系统等宽），按优先级排序：项目优先于系统。
pub fn candidate_font_paths(roots: &[PathBuf]) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    out.extend(scan_project_fonts_dir(roots));
    out.extend(system_mono_candidates());
    out
}

/// 扫描项目级 `fonts/` 目录：用户自置字体的入口，优先于系统字体。
///
/// 查找两个位置（都存在时都扫，项目根优先）：
/// 1. 可执行文件旁的 `fonts/`（安装态：随程序分发的字体）；
/// 2. 当前工作目录的 `fonts/`（开发态：`cargo run` 的工作目录即项目根）。
///
/// `roots` 由调用方注入（生产环境 [`default_roots`]），使发现可在临时目录单测。
pub fn scan_project_fonts_dir(roots: &[PathBuf]) -> Vec<(PathBuf, String)> {
    const EXTS: [&str; 3] = ["ttf", "otf", "ttc"];
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    for dir in roots {
        let Ok(entries) = std::fs::read_dir(dir) else {
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

/// 常见**等宽**字体候选（不收录彩色 emoji 字体，理由见 `fonts.rs` 模块文档的安全约束）。
#[cfg(target_os = "windows")]
pub fn system_mono_candidates() -> Vec<(PathBuf, String)> {
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
pub fn system_mono_candidates() -> Vec<(PathBuf, String)> {
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
pub fn system_mono_candidates() -> Vec<(PathBuf, String)> {
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

/// 候选字体路径，按「字形覆盖 + 观感」排序，靠前者优先（CJK 发现用，与装配解耦）。
#[cfg(target_os = "windows")]
pub fn candidate_paths() -> Vec<PathBuf> {
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
pub fn candidate_paths() -> Vec<PathBuf> {
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
pub fn candidate_paths() -> Vec<PathBuf> {
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

/// 把发现的字体源映射为设置页下拉项（始终以「系统等宽（默认）」打头）。
pub fn to_choices(sources: &[FontSource]) -> Vec<EditorFontChoice> {
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
