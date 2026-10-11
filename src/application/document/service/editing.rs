//! 编辑用例：插入 / 区间替换 / 格式指令 / 选区设置。

use crate::domain::document::services::EditingService;
use crate::domain::document::value_objects::EditPlan;
use crate::shared::error::AppError;
use crate::shared::geometry::{ByteOffset, TextRange};
use crate::shared::result::AppResult;

use crate::application::dto::{FormatResultView, RangeView};

use super::super::commands::*;
use super::super::queries::*;
use super::DocumentAppService;
use super::mapping::{apply_edit, shift_selection};
use super::stats::leading_list_number;

/// 把 `Document` 聚合投影为 [`DocumentView`](crate::application::dto::DocumentView)。
/// 「插入表格」按钮生成的表格尺寸。
///
/// 固定 3×3：它只是骨架，用户按需增删行列。做成命令参数反而要在
/// 工具栏上再加两个输入框，与 S6-07「一个按钮一个动作」的语义不符。
const TABLE_ROWS: usize = 3;
const TABLE_COLS: usize = 3;

impl DocumentAppService {
    /// 在指定偏移插入文本（替换选区请用 [`ReplaceRangeCommand`]）。
    pub fn insert_text(&mut self, cmd: InsertTextCommand) -> AppResult<()> {
        let now = self.clock.now();
        let events = {
            let mut reg = self.lock_write()?;
            let doc = reg
                .get_mut(cmd.id)
                .ok_or_else(|| AppError::domain("文档未打开"))?;
            doc.insert_at(ByteOffset::new(cmd.offset), &cmd.text, now)?;
            Self::drain_doc_events(doc)
        };
        self.publish_all(events);
        Ok(())
    }

    /// 把区间替换为文本。
    pub fn replace_range(&mut self, cmd: ReplaceRangeCommand) -> AppResult<()> {
        let now = self.clock.now();
        let events = {
            let mut reg = self.lock_write()?;
            let doc = reg
                .get_mut(cmd.id)
                .ok_or_else(|| AppError::domain("文档未打开"))?;
            doc.replace(cmd.range, &cmd.text, now)?;
            Self::drain_doc_events(doc)
        };
        self.publish_all(events);
        Ok(())
    }

    /// 应用一种格式指令（S6-07）。
    ///
    /// # 流程：取内容 → 领域算 `EditPlan` → 写回 → 发布事件 → 折算 DTO
    ///
    /// 规则全部来自 [`EditingService`]，本方法**不含任何 Markdown 拼接逻辑**——
    /// 那是「三入口一致」的前提：UI、快捷键、命令面板都只是本方法的不同调用方。
    ///
    /// 写回复用 [`Self::replace_range`]，因此格式操作与普通编辑**共享同一条撤销链**，
    /// Ctrl+Z 能把加粗撤掉（格式动作必须可撤销）。
    ///
    /// 返回 [`FormatResultView`] 而非 `()`：表现层需要新文本与新选区，
    /// 而它拿不到领域类型（`check-arch` 禁止 presentation 依赖 domain）。
    pub fn format(&mut self, cmd: FormatCommand) -> AppResult<FormatResultView> {
        // 1) 取当前内容。格式规则是纯函数，需要整篇文本作为上下文
        //    （例如判断是否「已包裹」要看包裹符是否紧邻在选区外）。
        let text = self.get_content(GetContentQuery {
            id: cmd.id,
            range: None,
        })?;

        // 2) 夹紧区间：调用方给的光标可能落在文本之外
        //    （例如文档刚被外部改短），不夹紧会在切片时 panic，
        //    而本项目 `panic = "abort"` 下等于终止进程。
        let range = TextRange::new(
            ByteOffset::new(cmd.range.start().value().min(text.len())),
            ByteOffset::new(cmd.range.end().value().min(text.len())),
        );

        let plan = match cmd.kind {
            FormatKind::Bold => EditingService::wrap_selection(&text, range, "**", "**"),
            FormatKind::Italic => EditingService::wrap_selection(&text, range, "*", "*"),
            FormatKind::Strikethrough => EditingService::wrap_selection(&text, range, "~~", "~~"),
            FormatKind::InlineCode => EditingService::wrap_selection(&text, range, "`", "`"),
            FormatKind::Quote => EditingService::toggle_line_prefix(&text, range, "> "),
            FormatKind::BulletList => EditingService::toggle_line_prefix(&text, range, "- "),
            FormatKind::PromoteHeading => EditingService::promote_heading(&text, range),
            FormatKind::DemoteHeading => EditingService::demote_heading(&text, range),
            FormatKind::OrderedList => Self::toggle_ordered_list(&text, range),
            FormatKind::ToggleTask => EditingService::toggle_task(&text, range.start()),
            FormatKind::InsertLink => EditingService::insert_link(&text, range),
            FormatKind::InsertCodeBlock => EditingService::insert_code_block(&text, range),
            FormatKind::InsertTable => {
                EditingService::insert_table(range.end(), TABLE_ROWS, TABLE_COLS)
            }
            FormatKind::Indent => {
                EditingService::indent_lines(&text, range, cmd.indent_unit.as_str())
            }
            FormatKind::Outdent => {
                EditingService::outdent_lines(&text, range, cmd.indent_unit.as_str())
            }
        };

        let edit = plan.edit();
        if edit.is_noop() {
            // 无效果编辑：不写、不发事件、不入撤销栈
            // （否则按一次 Ctrl+Z 只会「撤掉一个什么也没发生的操作」，
            //  用户会以为撤销坏了）。
            return Ok(FormatResultView {
                text,
                selection: RangeView::new(range.start().value(), range.end().value()),
                changed: false,
            });
        }

        // 3) 构造新文本并折算选区——必须在写回**之前**完成：
        //    领域给的选区是「编辑完成后在新内容中的位置」，
        //    写回后旧文本就没了。
        let new_text = apply_edit(&text, edit.range(), edit.replacement());
        let selection = shift_selection(&text, &new_text, plan.selection(), edit.range());

        // 4) 写回。走 `replace_range` 以复用事件发布与撤销链。
        self.replace_range(ReplaceRangeCommand {
            id: cmd.id,
            range: edit.range(),
            text: edit.replacement().to_string(),
        })?;

        Ok(FormatResultView {
            text: new_text,
            selection,
            changed: true,
        })
    }

    /// 切换有序列表：先探测范围内已用的序号，没有则从 1 起。
    ///
    /// 有序列表不能用固定前缀（`toggle_line_prefix` 只会插同一个字符串），
    /// 否则从第 5 行开始插入会得到「1. 1. 1.」。
    fn toggle_ordered_list(text: &str, range: TextRange) -> EditPlan {
        // 必须先把区间**扩展到整行**再探测序号：`toggle_line_prefix`
        // 内部会这么做，但探测发生在它之前。若直接用原始区间
        // （光标是空区间时为空串），`first` 永远回退到 1，
        // 于是 `3. third` 会被拼成 `1. 3. third`（序号被叠加）。
        let lines = EditingService::expand_to_full_lines(text, range);
        let first = text
            .get(lines.as_usize_range())
            .unwrap_or("")
            .lines()
            .filter_map(leading_list_number)
            .next()
            .unwrap_or(1);
        let prefix = format!("{first}. ");
        EditingService::toggle_line_prefix(text, range, prefix.as_str())
    }

    /// 设置光标 / 选区（空区间表示仅移动光标、清除选区）。
    pub fn set_selection(&mut self, cmd: SetSelectionCommand) -> AppResult<()> {
        let events = {
            let mut reg = self.lock_write()?;
            let doc = reg
                .get_mut(cmd.id)
                .ok_or_else(|| AppError::domain("文档未打开"))?;
            doc.select(cmd.selection);
            Self::drain_doc_events(doc)
        };
        self.publish_all(events);
        Ok(())
    }
}
