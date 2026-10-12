//! 尺寸 token：对齐原型 `tokens.css` 的布局栅格与间距刻度。

/// 尺寸token（对齐原型 `tokens.css` 的布局栅格）。
///
/// 这些值是**硬契约**：验收标准要求「1280×800 与 1440×900 两档窗口下
/// 布局不破版」，而破版与否直接取决于侧边栏是不是 240px。
/// 圆角同样取自原型（2026-10 精修后为 6 / 8 / 14），改动即等于偏离原型。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Metrics {
    /// 顶部工具栏高度（`--topbar-h`）。
    ///
    /// 这是**应用级菜单栏**那一行（文件 / 编辑 / 视图 / 设置），
    /// 不含编辑区工具栏——后者见 [`Metrics::editor_toolbar_height`]。
    pub topbar_height: f32,
    /// **编辑区工具栏**行高。
    ///
    /// # 为什么与应用级菜单栏分成两行
    ///
    /// 两者是**不同的层**：应用级菜单作用于整个应用（打开 / 保存 /
    /// 视图开关 / 设置），编辑区工具栏作用于当前编辑区的内容格式
    /// （加粗 / 列表 / 缩进……）。挤在一行里时，「文件」下拉与「加粗」
    /// 按钮看起来同级，职责边界随之消失。
    ///
    /// 该高度**不在原型 `tokens.css` 的栅格里**——原型把两者放在同一行
    /// （`--topbar-h: 40`），是本项目按「应用级 / 编辑区」分层后新增的一段。
    /// 取 34 而非 32：按钮本身高约 26，上下各留 4 的呼吸位，
    /// 视觉密度与 `--tabstrip-h: 32` 接近而略宽松。
    pub editor_toolbar_height: f32,
    /// 状态栏高度（`--statusbar-h`）。
    pub statusbar_height: f32,
    /// 标签条高度（`--tabbar-h`）。
    pub tabbar_height: f32,
    /// 多文件标签条高度（`--tabstrip-h`）。
    pub tabstrip_height: f32,
    /// 侧边栏宽度（`--sidebar-w`）。
    pub sidebar_width: f32,
    /// 分割条宽度（`--divider-w`）。
    pub divider_width: f32,
    /// 按钮圆角（`--radius-btn`）。
    pub radius_button: f32,
    /// 卡片圆角（`--radius-card`）。
    pub radius_card: f32,
    /// 对话框圆角（`--radius-dialog`）。
    pub radius_dialog: f32,
    /// 界面字号（`--ui-font-size`）。
    pub ui_font_size: f32,
    /// 预览区字号（`--preview-font-size`）。
    pub preview_font_size: f32,
    /// 预览区行高（`--preview-line-height`）。
    pub preview_line_height: f32,
    // —— 间距刻度（spacing scale，全应用唯一来源）——
    //
    // 原型 `tokens.css` 只定义了「尺寸」没有定义「间距刻度」，各视图于是
    // 各写各的 4 / 6 / 8 / 10 / 12 / 16 / 20 / 24：同类间距在不同区域忽大忽小，
    // 「分区留白」「元素内边距」「组件间距」没有可对照的规范。
    // 这里把它收敛成一条以 4pt 为基准、2pt 为咬合位的刻度
    // （与原型 `--sp-1: 4px` 同源）。任何 `add_space` / 内边距 / 组件间隙
    // 都应取这里的刻度值，而不是在现场发明新数字。
    /// 咬合间距：分隔线与内容、控件与贴身边缘的最小距离。
    pub space_xxs: f32,
    /// 紧凑间距：预览块与块之间、行内小控件之间。
    pub space_xs: f32,
    /// 小间距：图标与文字、卡片内边距的纵向分量。
    pub space_sm: f32,
    /// 中间距：组件默认间距（与全局 `Spacing::item_spacing.x` 同源）。
    pub space_md: f32,
    /// 大间距：字段行之间、下拉分组之间、对话框分区之间。
    pub space_lg: f32,
    /// 特大间距：功能区块之间、列表缩进。
    pub space_xl: f32,
    /// 顶级留白：对话框 / 欢迎页的顶级分区之间。
    pub space_xxl: f32,
    /// 面板 / 卡片 / 表格的四周内边距。
    pub pane_padding: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            topbar_height: 40.0,
            editor_toolbar_height: 34.0,
            statusbar_height: 24.0,
            tabbar_height: 28.0,
            tabstrip_height: 32.0,
            sidebar_width: 240.0,
            divider_width: 4.0,
            radius_button: 6.0,
            radius_card: 8.0,
            radius_dialog: 14.0,
            ui_font_size: 13.0,
            preview_font_size: 15.0,
            preview_line_height: 1.7,
            // 间距刻度：4pt 基网格 + 2pt 咬合位（见结构体字段文档）。
            space_xxs: 2.0,
            space_xs: 4.0,
            space_sm: 6.0,
            space_md: 8.0,
            space_lg: 12.0,
            space_xl: 16.0,
            space_xxl: 24.0,
            pane_padding: 8.0,
        }
    }
}
