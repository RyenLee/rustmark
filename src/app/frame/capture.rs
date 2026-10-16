//! 图片导出：视口截图、整篇长图、GPU 回传兑现、take_screenshot_event。
use super::*;

/// 一次等待回传的截图任务。
///
/// 只存**裁剪所需的矩形**与落盘目标，不存像素：像素由 egui 在下一帧
/// 通过 `Event::Screenshot` 交回，那是唯一拿到它的时机。
pub(crate) struct PendingCapture {
    /// 导出任务 ID（用于把回传结果关联回领域任务）。
    job_id: JobId,
    /// 预览区矩形（**逻辑点**，兑现时乘 `pixels_per_point` 才是像素）。
    ///
    /// 存逻辑点而非像素：`pixels_per_point` 要到回传时刻才能从
    /// `ctx` 读到，而矩形是在登记那一帧的布局里确定的。
    clip: egui::Rect,
    /// 输出路径。
    path: String,
    /// 命令是否已经发出去。
    ///
    /// 必须显式区分，否则每帧都会重发一次 `Screenshot`——那不是「多截几帧」，
    /// 而是让后端每帧都做一次全帧回读（GPU 同步等待），
    /// 代价高且用户只要第一张。
    requested: bool,
    /// 长图状态：`None` = 视口截图（一帧完成）；`Some` = 整篇长图（多段滚动拼接）。
    long: Option<LongImageState>,
}

/// 整篇长图的分段捕获状态。
///
/// 每段占用两帧：「设滚动偏移并等待绘制」→「发截图命令」→ 下一帧收像素。
/// 全部段落收齐后在 [`EditorApp::deliver_capture`] 里拼接落盘。
pub(crate) struct LongImageState {
    /// 每段的滚动偏移（逻辑点，内容坐标）。首段恒为 0，末段为 `content_h - 视口高`。
    offsets: Vec<f32>,
    /// 下一要捕获的段下标。
    next: usize,
    /// 已收到的各段像素（未拼接）。
    parts: Vec<image::RgbaImage>,
    /// 预览视口高度（逻辑点）。计算末段与上一段的重叠行数用。
    viewport_h: f32,
    /// 本段的滚动偏移是否已设、并等到了一次绘制。
    ///
    /// 截图命令捕获的是「发命令那一帧」的画面，所以必须先让预览
    /// 以目标偏移绘制一帧，**下一帧**才能发命令——两拍缺一不可。
    armed: bool,
}

/// 整篇长图的**段数上限**。
///
/// 每段至少跨两帧（预备拍 + 截图拍），256 段意味着滚动捕获约需十几秒——
/// 这已经是用户愿意等的上限。超限说明文档大到该走 HTML / PDF 导出，
/// 而不是让应用卡在「逐屏滚动截图」里几十秒甚至几分钟。
const MAX_LONG_IMAGE_SEGMENTS: usize = 256;

/// 计算长图拼接时每段需要丢弃的**顶部像素行数**。
///
/// 段偏移是「首段 0、其余每段推进一个视口高、末段滚到底」，因此只有
/// 末段与上一段重叠：上一段的底边（`offsets[i-1] + viewport_h`）超出
/// 本段顶边（`offsets[i]`）的部分就是要丢弃的重叠行。
///
/// 逻辑点 → 像素要乘 `pixels_per_point`（截图回传的是物理像素）；
/// 结果夹到 `part.height() - 1` 以内，防御舍入与几何量微小漂移
/// 导致 skip ≥ 段高（那会让 [`stitch`](crate::infrastructure::export::viewport_capture::stitch)
/// 直接拒绝整个任务，用户白等全部段落捕获完）。
fn compute_long_image_skip(long: &LongImageState, pixels_per_point: f32) -> Vec<u32> {
    long.parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                return 0;
            }
            let prev_end = long.offsets[i - 1] + long.viewport_h;
            let overlap = ((prev_end - long.offsets[i]).max(0.0) * pixels_per_point).round();
            overlap.min(part.height().saturating_sub(1) as f32) as u32
        })
        .collect()
}

impl EditorApp {
    /// 保存当前文档到指定路径（另存为对话框回填）。
    pub fn save_document_to(&mut self, path: &str) -> AppResult<()> {
        let Some(id) = self.ensure_document() else {
            return Ok(());
        };
        let target = FilePath::new(path).map_err(|e| AppError::Config {
            message: format!("路径非法：{e}"),
        })?;
        self.services
            .documents
            .save_to(SaveToPathCommand { id, path: target })?;
        // 内容没变但「已保存」状态变了，仍要回读让脏标记与标签标题一致。
        self.refresh_text();
        Ok(())
    }

    /// 导出当前文档到 `path`（返回产出路径与字节数）。
    ///
    /// 编排放这里而不是留在视图里，有两个理由：
    ///
    /// 1. **可测**：视图的 `run_export` 是私有函数，集成测试够不到；
    ///    而「四类格式各产出一次真实文件」正是验收标准，必须能被测试断言。
    /// 2. **单一入口**：将来菜单、快捷键、命令面板要触发导出时，
    ///    走的是同一段代码——与 S6-07 「三入口一致」同源。
    ///
    /// 走应用层两段式（`create_job` → `run`）：选项校验在领域，
    /// 这里只负责拼 DTO 与把错误转成提示。
    pub fn export_current_document(
        &mut self,
        request: ExportRequestView,
        path: &str,
    ) -> AppResult<ExportResultView> {
        let destination = FilePath::new(path).map_err(|e| AppError::io(e.to_string()))?;
        // 输出目录可能不存在（用户手填的路径，或上次导出后目录被删）。
        // 导出前确保它存在——否则底层写盘会以「找不到路径」失败，
        // 而那条错误对用户毫无指导意义。`create_dir_all` 对已存在目录幂等，
        // 故重复导出不会报错。
        if let Some(parent) = std::path::Path::new(path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::io(format!("无法创建输出目录 {}：{e}", parent.display())))?;
        }
        let doc_id = self
            .active_doc
            .ok_or_else(|| AppError::domain("没有可导出的文档"))?;
        let job = self.services.export.create_job(CreateExportJobCommand {
            doc_id,
            destination,
            request,
        })?;
        self.services.export.run(RunExportCommand { job_id: job })
    }

    /// 记录本帧预览区在窗口中的矩形（逻辑点）。
    ///
    /// 由 [`crate::presentation::views::main_view`] 在布局出预览区时调用。
    pub fn set_preview_rect(&mut self, rect: egui::Rect) {
        self.preview_rect = Some(rect);
    }

    /// 本帧记录的预览区矩形（尚未布局过时为 `None`）。
    pub fn preview_rect(&self) -> Option<egui::Rect> {
        self.preview_rect
    }

    /// 发起一次图片导出（视口截图），返回受理结果。
    ///
    /// # 为什么图片不走 `export_current_document`
    ///
    /// 那条路是「拿到源文本 → 同步写盘 → 返回字节数」。图片没有源文本可写——
    /// 像素来自GPU 对**当前帧**的回读，而 egui 的
    /// [`egui::ViewportCommand::Screenshot`] 以回调方式交付像素，后端在
    /// 下一帧开始时把渲染结果经 `ctx.request_screenshot` 的回调写入共享收件箱。
    /// 于是这里只做「登记 + 发命令」，真正的裁剪与落盘由
    /// [`Self::deliver_capture`] 在下一帧完成。
    ///
    /// # 为什么必须要求预览区可见
    ///
    /// 截图裁的是预览区那一块。预览区被折叠（专注模式 / 关闭预览）时
    /// 根本没有这块像素，与其导出一张空白图，不如当场说清原因。
    pub fn begin_viewport_capture(&mut self, path: &str) -> AppResult<()> {
        let doc_id = self
            .active_doc
            .ok_or_else(|| AppError::domain("没有可导出的文档"))?;
        let clip = self
            .preview_rect
            .filter(|r| r.width() > 0.0 && r.height() > 0.0)
            .ok_or_else(|| {
                AppError::io("预览区当前不可见，无法截图（请关闭专注模式或打开预览面板）")
            })?;

        // 任务仍走应用层创建：选项校验（扩展名匹配等）与状态机是领域的事，
        // 不该因为「像素晚一帧到」就在视图层另起一套。
        let destination = FilePath::new(path).map_err(|e| AppError::io(e.to_string()))?;
        let request = ExportRequestView {
            format: "image".to_string(),
            include_toc: false,
            embed_images: false,
            theme: self.export_ui.theme.clone(),
            page_size: "a4".to_string(),
            paginate: false,
        };
        let job_id = self.services.export.create_job(CreateExportJobCommand {
            doc_id,
            destination,
            request,
        })?;
        self.pending_capture = Some(PendingCapture {
            job_id,
            clip,
            path: path.to_string(),
            requested: false,
            long: None,
        });
        self.export_ui.awaiting_capture = true;
        Ok(())
    }

    /// 发起一次**整篇长图**导出（自动滚动预览区、分段截图、纵向拼接）。
    ///
    /// # 与视口截图的区别
    ///
    /// 视口截图只取「此刻所见」；长图要把整篇文档拼进一张 PNG。
    /// 实现方式不是离屏重建（egui 没有这条公开路径），而是**复用既有
    /// 截图回路**：把预览区滚到每个分段位置、各截一帧、在基础设施层
    /// 纵向拼接（[`crate::infrastructure::export::viewport_capture::stitch`]）。
    ///
    /// # 参数
    ///
    /// `pixels_per_point` 用于在**登记时刻**估算成品高度并提前拦截
    /// 超限请求——否则用户要等整个滚动流程跑完才被告知失败。
    pub fn begin_long_capture(&mut self, path: &str, pixels_per_point: f32) -> AppResult<()> {
        let doc_id = self
            .active_doc
            .ok_or_else(|| AppError::domain("没有可导出的文档"))?;
        let clip = self
            .preview_rect
            .filter(|r| r.width() > 0.0 && r.height() > 0.0)
            .ok_or_else(|| {
                AppError::io("预览区当前不可见，无法截图（请关闭专注模式或打开预览面板）")
            })?;

        // 内容几何量来自上一帧的预览布局（同步状态里存的就是它）。
        let geometry = self.ui.sync.geometry(Side::Preview);
        let viewport_h = geometry.viewport_h;
        let content_h = geometry.content_h;
        if viewport_h <= 0.0 || content_h <= viewport_h {
            // 内容不足一屏：长图与视口截图等价，直接走单帧路径。
            return self.begin_viewport_capture(path);
        }

        // 分段：每段恰好一个视口高（无缝平铺）；末段滚到底，与上一段的重叠
        // 在拼接时丢弃。段数上限防止异常大的文档让应用滚动几十秒。
        let segments =
            usize::try_from((content_h / viewport_h).ceil() as u64).unwrap_or(usize::MAX);
        if segments > MAX_LONG_IMAGE_SEGMENTS {
            return Err(AppError::io(format!(
                "文档过长（约 {segments} 屏，上限 {MAX_LONG_IMAGE_SEGMENTS} 屏），请改用 HTML / PDF 导出"
            )));
        }
        let mut offsets = Vec::with_capacity(segments);
        for i in 0..segments {
            let offset = if i + 1 == segments {
                content_h - viewport_h
            } else {
                i as f32 * viewport_h
            };
            offsets.push(offset.min(content_h - viewport_h).max(0.0));
        }
        // 成品高度提前拦截：估算 = 内容高 × 像素密度。
        let estimated_height = (content_h * pixels_per_point).ceil() as u64;
        if estimated_height
            > u64::from(crate::infrastructure::export::viewport_capture::MAX_LONG_IMAGE_HEIGHT)
        {
            return Err(AppError::io(format!(
                "长图高度约 {estimated_height}px，超过上限 {}px，请改用 HTML / PDF 导出",
                crate::infrastructure::export::viewport_capture::MAX_LONG_IMAGE_HEIGHT
            )));
        }

        let destination = FilePath::new(path).map_err(|e| AppError::io(e.to_string()))?;
        let request = ExportRequestView {
            format: "image".to_string(),
            include_toc: false,
            embed_images: false,
            theme: self.export_ui.theme.clone(),
            page_size: "a4".to_string(),
            paginate: false,
        };
        let job_id = self.services.export.create_job(CreateExportJobCommand {
            doc_id,
            destination,
            request,
        })?;
        self.pending_capture = Some(PendingCapture {
            job_id,
            clip,
            path: path.to_string(),
            requested: false,
            long: Some(LongImageState {
                offsets,
                next: 0,
                parts: Vec::new(),
                viewport_h,
                armed: false,
            }),
        });
        self.export_ui.awaiting_capture = true;
        Ok(())
    }

    /// 兑现一次截图：裁剪 →（长图：继续下一段或拼接落盘）→ 推进领域任务状态。
    ///
    /// 由 [`Self::pump_capture`] 在收件箱 `screenshot_sink` 拿到回传像素时调用。
    pub(super) fn deliver_capture(
        &mut self,
        mut pending: PendingCapture,
        frame: &std::sync::Arc<egui::epaint::ColorImage>,
        ctx: &egui::Context,
    ) {
        // 任务已兑现，无论成败都不再占用「等待中」——
        // 放在函数开头，保证下面任何 early return 都不会把它漏掉。
        self.export_ui.awaiting_capture = false;
        // 逻辑点 → 物理像素。必须在**回传时刻**换算：`pixels_per_point`
        // 此刻才能从 ctx 读到，而矩形是上一帧布局定的。
        let ppp = ctx.input(|i| i.pixels_per_point);
        let clip = [
            (pending.clip.min.x * ppp).round() as i64,
            (pending.clip.min.y * ppp).round() as i64,
            (pending.clip.max.x * ppp).round() as i64,
            (pending.clip.max.y * ppp).round() as i64,
        ];

        let Some(mut long) = pending.long else {
            // 视口截图：一帧闭环。
            let result = crate::infrastructure::export::viewport_capture::capture_region_to_png(
                frame,
                clip,
                std::path::Path::new(&pending.path),
            )
            .map_err(|e| ExportError::io_unavailable(e.to_string()));

            match self
                .services
                .export
                .complete_deferred(pending.job_id, result)
            {
                Ok(view) => self.export_ui.succeed(format!(
                    "已导出 {}（{} 字节）",
                    view.output_path.as_str(),
                    view.bytes
                )),
                Err(e) => self.export_ui.fail(e.to_string()),
            }
            return;
        };

        // ---- 整篇长图：本帧像素是第 `long.next` 段 ----
        let cropped = crate::infrastructure::export::viewport_capture::crop(frame, clip);
        match cropped {
            Ok(part) => long.parts.push(part),
            Err(e) => {
                self.export_ui
                    .fail(ExportError::io_unavailable(e.to_string()).to_string());
                return;
            }
        }
        long.next += 1;

        let has_more = long.next < long.offsets.len();
        if has_more {
            // 还有段：把任务放回 pending（清两拍标志），awaiting 保持占用，
            // 防止用户在滚动过程中再次提交覆盖本次任务。
            self.export_ui.awaiting_capture = true;
            // `armed` 必须清零：本帧像素属于刚交付的段，**下一段**的滚动
            // 偏移尚未设置（步骤 2 要到下一拍才会设）。若沿用 armed=true，
            // 步骤 2 会在同一帧立即重发截图命令，捕获到的仍是上一段
            // 的画面——长图里出现重复内容。
            long.armed = false;
            // `long` 是从 `pending` 里移出来的字段，放回去之后 `pending`
            // 重新完整，可以整体塞回 `pending_capture`。
            pending.long = Some(long);
            pending.requested = false;
            self.pending_capture = Some(pending);
            return;
        }

        // 全部段到齐：拼接 → 落盘 → 推进领域任务。
        let skip_top = compute_long_image_skip(&long, ppp);
        let result =
            crate::infrastructure::export::viewport_capture::stitch(&long.parts, &skip_top)
                .and_then(|img| {
                    crate::infrastructure::export::viewport_capture::write_png(
                        &img,
                        std::path::Path::new(&pending.path),
                    )
                })
                .map_err(|e| ExportError::io_unavailable(e.to_string()));

        match self
            .services
            .export
            .complete_deferred(pending.job_id, result)
        {
            Ok(view) => self.export_ui.succeed(format!(
                "已导出整篇长图 {}（{} 字节）",
                view.output_path.as_str(),
                view.bytes
            )),
            Err(e) => self.export_ui.fail(e.to_string()),
        }
    }

    /// 处理截图回路：发出待处理的请求，并接收本帧回传的像素。
    ///
    /// 由 [`eframe::App::update`] 每帧在**绘制之后**调用。
    ///
    /// # 为什么必须跨两帧
    ///
    /// egui 在**帧末**才把 `ViewportCommand::Screenshot` 交给后端，
    /// 后端捕获的是**那一帧**的渲染结果，并在下一帧开始时经
    /// `request_screenshot` 的回调把像素写入共享收件箱。所以「发」与「收」
    /// 天然分处两帧，同一帧内无法闭环——这不是实现取巧，是 API 的时序。
    ///
    /// `requested` 标志保证**只发一次**：否则本帧登记的任务会在之后
    /// 每帧都触发一次全帧 GPU 回读。
    pub fn pump_capture(&mut self, ctx: &egui::Context) {
        // 1) 先收：上一帧发出的截图命令，其像素已由后端经回调写入 `screenshot_sink`。
        //
        // 只认 `requested == true` 的任务。否则用户在命令发出前就关掉了对话框、
        // 又恰好收到别的截图回传时，会把像素写到一个已作废的路径上。
        //
        // egui 0.37 起 `request_screenshot` 以回调交付像素（不再是 `Event::Screenshot`），
        // 回调在 `self` 之外触发，故落到共享的 `screenshot_sink` 里，这里每帧取回。
        // 锁中毒（poison）只在持有锁的线程 panic 时发生；恢复时直接取内层值，
        // 用 `unwrap_or_else(|e| e.into_inner())` 而非 `unwrap`，以满足 clippy
        // `unwrap_used` 门禁。
        let frame = {
            let mut guard = self
                .screenshot_sink
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        let deliverable =
            frame.is_some() && self.pending_capture.as_ref().is_some_and(|p| p.requested);
        if deliverable && let (Some(frame), Some(pending)) = (frame, self.pending_capture.take()) {
            self.deliver_capture(pending, &frame, ctx);
        }

        // 2) 再发：把尚未发过命令的任务登记下去（结果通过回调落入下一帧的 `screenshot_sink`）。
        //
        //    长图多一拍「预备」：命令捕获的是**发命令那一帧**的画面，
        //    所以要先设好滚动偏移、等预览以该偏移绘制一帧，下一帧才发命令。
        if let Some(pending) = self.pending_capture.as_mut()
            && !pending.requested
        {
            let mut scroll_to: Option<f32> = None;
            // `request_screenshot` 的回调在 `self` 之外触发，故用共享收件箱传递像素。
            let make_sink = || self.screenshot_sink.clone();
            match pending.long.as_mut() {
                None => {
                    let sink = make_sink();
                    ctx.request_screenshot(
                        move |image: std::sync::Arc<egui::epaint::ColorImage>| {
                            let mut guard = sink.lock().unwrap_or_else(|e| e.into_inner());
                            *guard = Some(image);
                        },
                    );
                    pending.requested = true;
                }
                Some(long) => {
                    if long.armed && long.next < long.offsets.len() {
                        let sink = make_sink();
                        ctx.request_screenshot(
                            move |image: std::sync::Arc<egui::epaint::ColorImage>| {
                                let mut guard = sink.lock().unwrap_or_else(|e| e.into_inner());
                                *guard = Some(image);
                            },
                        );
                        pending.requested = true;
                    } else if long.next < long.offsets.len() {
                        // 预备拍：把预览滚到本段位置。`preview_view` 每帧以
                        // `preview.scroll_y` 为初始偏移，下一帧即渲染本段。
                        scroll_to = Some(long.offsets[long.next]);
                        long.armed = true;
                    }
                }
            }
            if let Some(y) = scroll_to {
                self.preview.scroll_y = y;
            }
        }
    }
}
