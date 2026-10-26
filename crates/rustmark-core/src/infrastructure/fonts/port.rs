//! egui 无关的字体定义端口类型（V11-63 阶段 B）。
//!
//! 基础设施层不得依赖 egui，但「把系统字体文件适配成 egui 能消化的形状」
//! 恰是基础设施本职（与 `export` 适配渲染后端、`persistence` 适配文件系统
//! 同理）。解决办法：让基础设施产出这里的**纯数据**端口类型，由组合根
//! （`rustmark::app::frame`）经适配器一次性转成 `egui::FontDefinitions` 再交给
//! `Context::set_fonts`。
//!
//! # 与 egui 类型的对应关系
//!
//! | 端口类型 | egui 对应 |
//! |---|---|
//! | [`PortFontDefinitions`] | `epaint::text::FontDefinitions` |
//! | [`PortFontFamily`] | `epaint::text::FontFamily` |
//! | [`PortFontData`] | `epaint::text::FontData` |
//!
//! # 为什么只承载「字节 + scale」
//!
//! egui 0.37 的 `FontData` 是 `{ font: Blob, index: u32, tweak: FontTweak }`，
//! 而 `FontTweak` 有 9 个字段（`hinting` / `hinting_target` / `subpixel_binning`
//! / `coords` / `thin_space_width` / `tab_size` …）。逐字段镜像会把
//! `HintingTarget`、`SmoothHinting`、`VariationCoords` 一并拖进基础设施层，
//! 换来一堆**本项目从未设置**的字段。
//!
//! 本项目的实际需求只有两项（见 `fonts.rs::install_egui_base_fonts` 与
//! `discovery.rs`）：**字体文件字节**，以及 emoji 字体的**整体缩放**
//! （NotoEmoji 0.81 / emoji-icon-font 0.90）。`index` 恒为 0（首字面），
//! 其余微调项改造前也从未赋值——适配器把它们一律取 egui 的 `Default`，
//! 与改造前的行为逐字段等价。将来真要调 hinting / coords 时，在此加字段
//! 并在适配器里补一行映射即可（端口是本地类型，改动面收敛在这两个文件）。

use std::collections::BTreeMap;
use std::sync::Arc;

/// 镜像 `epaint::text::FontFamily`。
///
/// 排序与 egui 一致：`Proportional < Monospace < Name`（derive `Ord` 按变体声明
/// 顺序 + 名称字典序），使 `families` 的 `BTreeMap` 键序与 egui 完全对应。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PortFontFamily {
    /// 比例族（正文）；镜像 egui 的 `FontFamily::Proportional`。
    Proportional,
    /// 等宽族（编辑区与代码块）；镜像 egui 的 `FontFamily::Monospace`。
    Monospace,
    /// 自建族（egui 的 `FontFamily::Name(Arc<str>)`）。
    Name(String),
}

/// 镜像 `epaint::text::FontData` 中**本项目实际使用的部分**。
///
/// `bytes` 用 `Arc` 包裹（与 egui 的 `Blob` 同构），使适配器能零拷贝地把
/// 字节交给 egui——不必为 1.5 MB 的 CJK 子集多复制一份。
#[derive(Clone, Debug, PartialEq)]
pub struct PortFontData {
    /// 字体文件原始字节（TTF / OTF 等轮廓字体）。
    pub bytes: Arc<Vec<u8>>,
    /// 整体缩放；`1.0` 为原始大小（对应 `egui::FontTweak::scale`）。
    ///
    /// 只有 emoji 字体用它（epaint 内嵌时代的原值）。其余微调项（hinting、
    /// coords 等）由适配器取 egui 默认值，见模块文档。
    pub scale: f32,
}

impl PortFontData {
    /// 包裹字体字节，`scale` 取 `1.0`（不缩放）。
    pub fn from_owned(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Arc::new(bytes),
            scale: 1.0,
        }
    }

    /// 设置整体缩放后返回（对应 `egui::FontData::tweak`）。
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }
}

/// 镜像 `epaint::text::FontDefinitions`：`font_data` 是键到字体字节的映射，
/// `families` 是族到「字体键序列」的映射（序列顺序即排版回退优先级）。
///
/// `font_data` 的值按值存放（egui 存 `Arc<FontData>`）：本端口的
/// [`PortFontData::bytes`] 已是 `Arc`，克隆整张表仍只复制指针 + 一个 `f32`，
/// 适配器再加一层 `Arc` 的开销发生在启动时那 4–10 个字体上，可忽略。
#[derive(Clone, Debug, PartialEq)]
pub struct PortFontDefinitions {
    /// 字体键 → 字体数据；键即 `families` 里引用的名字。
    pub font_data: BTreeMap<String, PortFontData>,
    /// 族 → 字体键序列（顺序即回退优先级）。
    pub families: BTreeMap<PortFontFamily, Vec<String>>,
}

impl Default for PortFontDefinitions {
    /// 与 `epaint::text::FontDefinitions::empty()` **逐字段一致**：
    /// `font_data` 空，但 `Proportional` / `Monospace` 两个族**键存在、列表为空**。
    ///
    /// 这个细节不是形式主义：`install_egui_base_fonts` 用 `families.get_mut(family)`
    /// 往既有族里 push（缺键即静默跳过，避免留悬空键），`extend_with_cjk` 则用
    /// `entry(...).or_default()`。若默认是**空 map**，前者会把四个基础字体全部丢掉，
    /// 界面直接失去首选字形——`definitions_stay_usable_when_no_cjk_font_exists`
    /// 正是把这条钉死的测试（它断言的 `contains_key(Proportional)` 就是判据）。
    ///
    /// 本项目 `default_fonts` / `monochrome_emoji_fonts` 两个 feature 已关闭，
    /// 故 egui 的 `FontDefinitions::default()` 即 `empty()`，由此取齐。
    fn default() -> Self {
        let mut families = BTreeMap::new();
        families.insert(PortFontFamily::Monospace, Vec::new());
        families.insert(PortFontFamily::Proportional, Vec::new());

        Self {
            font_data: BTreeMap::new(),
            families,
        }
    }
}
