//! 导出对话框 UI 状态（格式、选项、输出路径、进度、错误）。
//!
//! # 硬边界：这里只放**视图状态**
//!
//! 允许：选了哪个格式、勾了哪些选项、正在导出吗、错误文案。
//! 禁止：`ExportJob` / `ExportOptions` 等领域对象或应用层 DTO 的副本。
//!
//! 尤其**不缓存**「上一次导出用了什么选项」：那是从 `ExportJob` 派生的数据，
//! 缓存它就等于与领域里的任务记录双写，不一致时用户看到的选项与实际导出的
//! 文件对不上，且**没有任何报错**。

/// 导出格式（UI 视角，取值与领域 `ExportFormat` 的 `parse_format` 一致）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ExportFormatChoice {
    /// 单文件 HTML。
    #[default]
    Html,
    /// Markdown 源码。
    Markdown,
    /// PDF。
    Pdf,
    /// 长图（PNG）。
    Image,
}

impl ExportFormatChoice {
    /// 全部格式（单选组的顺序即显示顺序）。
    pub const ALL: &'static [ExportFormatChoice] = &[
        ExportFormatChoice::Html,
        ExportFormatChoice::Markdown,
        ExportFormatChoice::Pdf,
        ExportFormatChoice::Image,
    ];

    /// 中文显示名。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Html => "HTML",
            Self::Markdown => "Markdown",
            Self::Pdf => "PDF",
            Self::Image => "图片",
        }
    }

    /// 提交给应用层的格式标识（`ExportRequestView::format` 的取值）。
    pub const fn id(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Markdown => "markdown",
            Self::Pdf => "pdf",
            Self::Image => "image",
        }
    }

    /// 文件扩展名（不含点）。
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Markdown => "md",
            Self::Pdf => "pdf",
            Self::Image => "png",
        }
    }

    /// 是否支持目录（TOC）。
    pub const fn supports_toc(self) -> bool {
        matches!(self, Self::Html | Self::Pdf)
    }

    /// 是否支持分页。
    pub const fn supports_pagination(self) -> bool {
        matches!(self, Self::Pdf)
    }

    /// 是否需要指定页面尺寸。
    pub const fn needs_page_size(self) -> bool {
        matches!(self, Self::Pdf)
    }

    /// 该格式在本实现中是否**真的可用**。
    ///
    /// **PDF 已开放（V11-04）**：选择「开放 + 语义诚实」而非继续禁用——
    /// 基础设施（S5-10 打印管线）本就就绪，禁用只是把已能用的能力藏起来。
    /// 产出方式取决于本机有什么：装有 Edge / Chrome 时**直接落盘** `.pdf`，
    /// 否则退回系统打印对话框。这一点由 [`Self::pdf_caveat`] 在选择 PDF 时
    /// 就讲清楚，而不是让用户点了导出才发现。
    pub const fn is_available(self) -> bool {
        true
    }

    /// 不可用时的原因（`None` = 可用）。
    pub const fn unavailable_reason(self) -> Option<&'static str> {
        None
    }

    /// PDF 的**前置声明**：点了会发生什么（V11-04，V11-04a 修订）。
    ///
    /// 之所以必须写在界面上：PDF 与另外三种格式的行为**可能**不同——
    /// 其余格式一定是「选好路径 → 点导出 → 文件就在那」，
    /// PDF 在缺少浏览器内核的机器上会变成
    /// 「生成打印用 HTML → 调起系统对话框 → 用户在对话框里另存」。
    /// 不提前说明，用户会以为导出失败（目标 `.pdf` 没出现）。
    ///
    /// 这里**不**去探测本机装了什么再改文案：那要求表现层查文件系统，
    /// 既跨层又每帧一次。把两种情形各说一句更省事，也不会说错。
    pub const fn pdf_caveat(self) -> Option<&'static str> {
        match self {
            Self::Pdf => Some(
                "PDF 由系统内置排版内核直接生成到目标路径（需本机装有 Edge 或 Chrome）；缺少时改为调起系统打印，请在对话框中选择「另存为 PDF」。",
            ),
            Self::Html | Self::Markdown | Self::Image => None,
        }
    }
}

/// 页面尺寸。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PageSizeChoice {
    /// A4（210×297mm）。
    #[default]
    A4,
    /// Letter（216×279mm）。
    Letter,
}

impl PageSizeChoice {
    /// 全部取值。
    pub const ALL: &'static [PageSizeChoice] = &[PageSizeChoice::A4, PageSizeChoice::Letter];

    /// 中文显示名。
    pub const fn label(self) -> &'static str {
        match self {
            Self::A4 => "A4 210×297mm",
            Self::Letter => "Letter 216×279mm",
        }
    }

    /// 提交给应用层的标识（`ExportRequestView::page_size` 的取值）。
    pub const fn id(self) -> &'static str {
        match self {
            Self::A4 => "a4",
            Self::Letter => "letter",
        }
    }
}

/// 导出对话框状态。
#[derive(Clone, Debug)]
pub struct ExportUiState {
    /// 当前格式。
    pub format: ExportFormatChoice,
    /// 是否生成目录。
    pub include_toc: bool,
    /// 是否内联图片为 data URI。
    pub embed_images: bool,
    /// 页面尺寸。
    pub page_size: PageSizeChoice,
    /// 是否分页。
    pub paginate: bool,
    /// 输出目录（含分隔符，或直接是完整目标路径）。
    pub output_dir: String,
    /// 建议文件名（不含扩展名）。
    pub file_stem: String,
    /// 导出主题名（写入导出件）。
    pub theme: String,
    /// 本次导出的目标路径（由格式与文件名推导，供用户确认）。
    pub resolved_path: String,
    /// 是否正在导出。
    pub running: bool,
    /// 是否有一张截图在等待 GPU 回传（P2 图片导出）。
    ///
    /// 独立于 `running`：截图**必然跨一帧**，`running` 会在用户关闭对话框时
    /// 被复位（那只是 UI 的事），而这一项必须活到回传真正兑现为止。
    /// 少了它，用户关掉再打开对话框就能再点一次导出，
    /// 于是同一个 `pending_capture` 被静默覆盖，上一张截图白等。
    pub awaiting_capture: bool,
    /// 图片格式专用：导出**整篇长图**（多段滚动拼接）而不是当前视口所见。
    ///
    /// 只在 `format == Image` 时有意义；切到其他格式时清零
    /// （见 [`Self::set_format`] 的夹紧策略，与目录 / 分页开关同理）。
    pub long_image: bool,
    /// 错误提示（如扩展名不匹配、格式不可用）。
    pub error: Option<String>,
    /// 成功提示（含产出路径与字节数）。
    pub notice: Option<String>,
}

impl Default for ExportUiState {
    fn default() -> Self {
        Self::new("document", "dark")
    }
}

impl ExportUiState {
    /// 以建议文件名与主题构造。
    pub fn new(file_stem: impl Into<String>, theme: impl Into<String>) -> Self {
        let mut state = Self {
            format: ExportFormatChoice::Html,
            include_toc: false,
            embed_images: false,
            page_size: PageSizeChoice::A4,
            paginate: false,
            output_dir: String::new(),
            file_stem: file_stem.into(),
            theme: theme.into(),
            resolved_path: String::new(),
            running: false,
            awaiting_capture: false,
            long_image: false,
            error: None,
            notice: None,
        };
        state.refresh_resolved_path();
        state
    }

    /// 打开导出对话框：清掉上一次的提示，但**保留**格式与选项。
    ///
    /// 保留是刻意的：用户连续导出多个格式时不该每次重选。
    /// 若认为该重置，注意这与「切换文档重置查找查询」（S6-08）的理由不同——
    /// 那里的查询属于**旧文档**，带过去会跳到错误位置；这里的选项与文档无关。
    pub fn open(&mut self) {
        self.running = false;
        self.error = None;
        self.notice = None;
        self.refresh_resolved_path();
    }

    /// 切换格式，并**夹紧**该格式不支持的选项。
    ///
    /// 夹紧而非保留：选了 Markdown 还勾着「目录」，提交时领域会拒
    /// （`TocNotSupported`）。那时用户看到的错误是「Markdown 不支持目录」，
    /// 而他从来没想过这件事——夹紧掉这个开关，界面上就根本没这个矛盾。
    pub fn set_format(&mut self, format: ExportFormatChoice) {
        self.format = format;
        if !format.supports_toc() {
            self.include_toc = false;
        }
        if !format.supports_pagination() {
            self.paginate = false;
        }
        // 长图开关同理夹紧：它是 Image 专属选项，切走再切回来时
        // 用户多半已不记得勾过——留着只会导出意料之外的产物。
        if format != ExportFormatChoice::Image {
            self.long_image = false;
        }
        self.error = None;
        self.refresh_resolved_path();
    }

    /// 目标文件完整路径（目录 + 文件名 + 扩展名）。
    pub fn target_path(&self) -> String {
        let stem = if self.file_stem.trim().is_empty() {
            "document"
        } else {
            self.file_stem.trim()
        };
        let dir = if self.output_dir.trim().is_empty() {
            String::new()
        } else {
            let mut dir = self.output_dir.trim().to_string();
            if !dir.ends_with('/') && !dir.ends_with('\\') {
                dir.push('/');
            }
            dir
        };
        format!("{dir}{}.{}", stem, self.format.extension())
    }

    /// 刷新 `resolved_path`（渲染层显示用，避免每帧重复拼字符串）。
    pub fn refresh_resolved_path(&mut self) {
        self.resolved_path = self.target_path();
    }

    /// 当前提交是否**可执行**。
    ///
    /// 三个条件缺一不可：格式可用、没在导出中、目标路径带正确扩展名。
    /// 「扩展名正确」放在这里而不是交给领域报错，是为了让「导出」按钮
    /// 直接置灰——但**仍要**在提交时让领域再校验一次（UI 的判断可能过期，
    /// 例如用户在别的窗口改了文件名）。
    pub fn can_submit(&self) -> bool {
        self.blocked_reason().is_none()
    }

    /// 不可提交时的原因（`None` = 可提交）。
    pub fn blocked_reason(&self) -> Option<String> {
        if self.running {
            return Some("正在导出，请稍候".to_string());
        }
        // 截图在等GPU 回传：与 `running` 分开判，因为关掉对话框会复位
        // `running`，但那一帧的命令已经发出去了，只能等回传兑现。
        if self.awaiting_capture {
            return Some("正在等待截图返回，请稍候".to_string());
        }
        if let Some(reason) = self.format.unavailable_reason() {
            return Some(reason.to_string());
        }
        // 必须显式给出输出目录。留空时 `target_path` 产出的是裸文件名，
        // 会被写到**进程工作目录**——GUI 程序的 CWD 取决于启动方式，
        // 用户既看不到也找不到。把文件藏在看不见的地方比拦住更糟。
        if self.output_dir.trim().is_empty() {
            return Some("请先填写输出目录（或先把文档保存到磁盘）".to_string());
        }
        let extension_ok = self
            .resolved_path
            .rsplit('.')
            .next()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(self.format.extension()));
        if !extension_ok {
            return Some(format!(
                "输出路径必须以 `.{}` 结尾（当前：{}）",
                self.format.extension(),
                self.resolved_path
            ));
        }
        None
    }

    /// 标记导出开始。
    pub fn begin(&mut self) {
        self.running = true;
        self.error = None;
        self.notice = None;
    }

    /// 标记导出成功。
    pub fn succeed(&mut self, message: impl Into<String>) {
        self.running = false;
        self.error = None;
        self.notice = Some(message.into());
    }

    /// 标记导出失败。
    pub fn fail(&mut self, message: impl Into<String>) {
        self.running = false;
        self.notice = None;
        self.error = Some(message.into());
    }

    /// 关闭对话框：清掉瞬态状态。
    pub fn close(&mut self) {
        self.running = false;
        self.error = None;
        self.notice = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_to_markdown_clears_toc_and_pagination() {
        let mut state = ExportUiState::new("doc", "dark");
        state.include_toc = true;
        state.paginate = true;

        state.set_format(ExportFormatChoice::Markdown);

        assert!(!state.include_toc, "Markdown 不支持目录，开关必须夹紧");
        assert!(!state.paginate, "Markdown 不支持分页，开关必须夹紧");
    }

    #[test]
    fn pdf_keeps_toc_and_pagination() {
        let mut state = ExportUiState::new("doc", "dark");
        state.include_toc = true;
        state.paginate = true;

        state.set_format(ExportFormatChoice::Pdf);

        assert!(
            state.include_toc && state.paginate,
            "PDF 两者都支持，不该夹紧"
        );
    }

    #[test]
    fn switching_away_from_image_clears_long_image_flag() {
        // 长图开关是 Image 专属：切走必须清零，防止用户切回 HTML 后
        // 残留的开关在某个未来分支里悄悄改变行为（防御性夹紧，与 TOC 同理）。
        let mut state = ExportUiState::new("doc", "dark");
        state.set_format(ExportFormatChoice::Image);
        state.long_image = true;

        state.set_format(ExportFormatChoice::Html);
        assert!(!state.long_image, "切走 Image 时长图开关应被夹紧");

        state.set_format(ExportFormatChoice::Image);
        assert!(!state.long_image, "切回 Image 不应恢复旧开关状态");
    }

    #[test]
    fn target_path_follows_the_selected_extension() {
        let mut state = ExportUiState::new("notes", "dark");
        assert_eq!(state.target_path(), "notes.html");

        state.set_format(ExportFormatChoice::Markdown);
        assert_eq!(state.target_path(), "notes.md");

        state.set_format(ExportFormatChoice::Image);
        assert_eq!(state.target_path(), "notes.png");
    }

    #[test]
    fn output_dir_gains_a_separator_when_missing() {
        let mut state = ExportUiState::new("notes", "dark");
        state.output_dir = "/tmp/out".to_string();
        assert_eq!(state.target_path(), "/tmp/out/notes.html");

        state.output_dir = "/tmp/out/".to_string();
        assert_eq!(
            state.target_path(),
            "/tmp/out/notes.html",
            "已带分隔符时不应重复添加"
        );
    }

    #[test]
    fn blank_file_stem_falls_back_to_document() {
        let state = ExportUiState::new("   ", "dark");
        assert!(
            state.target_path().starts_with("document."),
            "空文件名应回退而不是产出 `.html` 这种无名文件"
        );
    }

    #[test]
    fn unavailable_formats_cannot_be_submitted_with_a_reason() {
        // 遍历**全部**格式而非只列未开放的那几个：
        // 这样「哪些格式不可用」由实现决定，测试不会在有人新增格式时静默失效。
        // 自 P2 起图片已可用（视口截图 + 长图），故只有 PDF 属界面未开放。
        for format in ExportFormatChoice::ALL {
            let mut state = ExportUiState::new("doc", "dark");
            state.output_dir = "/tmp/out".to_string();
            state.set_format(*format);
            match format.unavailable_reason() {
                Some(reason) => {
                    assert!(
                        !state.can_submit(),
                        "{format:?} 既声明了不可用原因，就不该可提交"
                    );
                    assert_eq!(
                        state.blocked_reason().as_deref(),
                        Some(reason),
                        "{format:?} 的拦截原因应与声明一致"
                    );
                    assert!(
                        reason.contains("开放") || reason.contains("实现"),
                        "原因应说清是未开放/未实现而不是笼统的失败：{reason}"
                    );
                }
                None => assert!(
                    state.can_submit(),
                    "{format:?} 未声明不可用原因，就该可提交：{:?}",
                    state.blocked_reason()
                ),
            }
        }
        // 明确钉住当前的可用性边界，防止悄悄变化。
        //
        // V11-04 之后 PDF 经由系统打印对话框产出，不再是「未实现」的占位——
        // 因此四种格式**全部可用**，可用性的边界从「PDF 不可用」变成了「全可用」。
        let unavailable: Vec<_> = ExportFormatChoice::ALL
            .iter()
            .filter(|f| !f.is_available())
            .copied()
            .collect();
        assert!(
            unavailable.is_empty(),
            "V11-04 后 PDF 经系统打印可用，不应有不可用格式：{:?}",
            unavailable
        );
        // 反向钉死：PDF 现在必须可用（这是 V11-04 验收的核心，不能静默回退）。
        assert!(
            ExportFormatChoice::Pdf.is_available(),
            "PDF 自 V11-04 起应可用（系统打印路径）"
        );
    }

    #[test]
    fn image_format_is_available_since_p2() {
        // 图片走视口截图：可用、可提交、不给「未实现」提示。
        let mut state = ExportUiState::new("doc", "dark");
        state.output_dir = "/tmp/out".to_string();
        state.set_format(ExportFormatChoice::Image);
        assert!(
            ExportFormatChoice::Image.is_available(),
            "图片自 P2 起应可用"
        );
        assert!(
            ExportFormatChoice::Image.unavailable_reason().is_none(),
            "可用格式不应给出未实现原因"
        );
        assert!(state.can_submit());
        assert!(state.target_path().ends_with(".png"));
    }

    #[test]
    fn awaiting_capture_blocks_resubmission() {
        // 截图跨一帧：用户关掉对话框会复位 `running`，
        // 若不单独拦awaiting_capture，就能再点一次导出把上一次覆盖掉。
        let mut state = ExportUiState::new("doc", "dark");
        state.output_dir = "/tmp/out".to_string();
        state.set_format(ExportFormatChoice::Image);
        state.begin();
        state.running = false; // 模拟「关掉对话框」把 running 复位
        state.awaiting_capture = true;
        assert!(!state.can_submit(), "等截图时不应能再次提交");
        assert_eq!(
            state.blocked_reason().as_deref(),
            Some("正在等待截图返回，请稍候")
        );
    }

    #[test]
    fn available_formats_are_submittable() {
        for format in [
            ExportFormatChoice::Html,
            ExportFormatChoice::Markdown,
            ExportFormatChoice::Image,
        ] {
            let mut state = ExportUiState::new("doc", "dark");
            state.output_dir = "/tmp/out".to_string();
            state.set_format(format);
            assert!(state.can_submit(), "{format:?} 应可提交");
            assert!(state.blocked_reason().is_none());
        }
    }

    #[test]
    fn running_blocks_submission() {
        let mut state = ExportUiState::new("doc", "dark");
        state.output_dir = "/tmp/out".to_string();
        state.begin();
        assert!(!state.can_submit());
        assert_eq!(state.blocked_reason().as_deref(), Some("正在导出，请稍候"));
    }

    #[test]
    fn empty_output_dir_blocks_submission() {
        let mut state = ExportUiState::new("doc", "dark");
        assert!(
            !state.can_submit(),
            "留空输出目录时不得提交：文件会落到用户看不见的进程工作目录"
        );
        assert!(
            state
                .blocked_reason()
                .is_some_and(|reason| reason.contains("输出目录")),
            "应说清是缺输出目录"
        );
        state.output_dir = "/tmp/out".to_string();
        assert!(state.can_submit(), "填了目录后应可提交");
    }

    #[test]
    fn every_format_reports_a_label_and_id() {
        for format in ExportFormatChoice::ALL {
            assert!(!format.label().is_empty(), "{format:?} 缺显示名");
            assert!(!format.id().is_empty(), "{format:?} 缺提交标识");
            assert!(!format.extension().is_empty(), "{format:?} 缺扩展名");
        }
    }
}
