//! [`MenuGroup`] 的固有 impl 块：分组常量 + 显示名。

use super::*;

impl MenuGroup {
    /// 全部分组（按显示顺序）。
    ///
    /// 用于**需要覆盖全部动作**的分组视图：命令面板、快捷键一览。
    /// 顶部菜单栏不用这一份——它只放应用级分组，见 [`MenuGroup::APP_LEVEL`]。
    pub const ALL: &'static [MenuGroup] = &[
        MenuGroup::File,
        MenuGroup::Edit,
        MenuGroup::Format,
        MenuGroup::View,
        MenuGroup::Help,
    ];

    /// **应用级**分组（顶部菜单栏第一层，按显示顺序）。
    ///
    /// # 为什么把 [`MenuGroup::Format`] 排除在外
    ///
    /// 这一层是「应用级菜单」：打开 / 保存 / 撤销 / 视图开关 / 设置，
    /// 作用域是**整个应用**。而格式编排（加粗、列表、缩进……）作用域是
    /// **当前编辑区的内容**，它属于编辑区工具栏，不属于应用级菜单。
    ///
    /// 两者混在同一行时，「文件」下拉与「加粗」按钮看起来同级，
    /// 职责边界就消失了——用户分不清哪些命令会改变全局状态、
    /// 哪些只动光标处的文本。因此这里显式按作用域切开：
    /// - 本常量：顶部**第一层**，全部是下拉菜单；
    /// - [`MenuGroup::Format`]：顶部**第二层**（编辑区工具栏），全部是按钮。
    ///
    /// [`MenuGroup::ALL`] 保持不变：命令面板与快捷键一览仍需要「格式」分组。
    pub const APP_LEVEL: &'static [MenuGroup] = &[
        MenuGroup::File,
        MenuGroup::Edit,
        MenuGroup::View,
        MenuGroup::Help,
    ];

    /// 分组显示名。
    pub const fn label(self) -> &'static str {
        match self {
            MenuGroup::File => "文件",
            MenuGroup::Edit => "编辑",
            MenuGroup::Format => "格式",
            MenuGroup::View => "视图",
            MenuGroup::Help => "设置",
        }
    }
}
