//! 编辑区 UI 状态（光标、选区、滚动、IME 组字）。
//!
//! # 为什么光标要存在 UI 层而不在领域层
//!
//! 领域的 `Document` 只管**内容**与撤销栈；光标与选区是**视图状态**——
//! 它不参与撤销语义（撤销一次输入不该把光标弹回三行之前），
//! 却必须随编辑更新。放错层会导致「撤销时光标乱跳」（原型缺陷 B-07）。
//!
//! 边界：本模块只存**字节偏移**，不存文本、不存 `Rope`。
//! 文本永远来自 `DocumentService::get_content`（DTO）。

/// 编辑区 UI 状态。
#[derive(Clone, Debug, Default)]
pub struct EditorUiState {
    /// 光标字节偏移。
    pub cursor: usize,
    /// 选区锚点（`None` 表示无选区）。
    pub anchor: Option<usize>,
    /// 垂直滚动量（egui 的 `ScrollArea` 偏移）。
    pub scroll_y: f32,
    /// 水平滚动量。
    pub scroll_x: f32,
    /// 是否拥有键盘焦点（决定快捷键是否走编辑区上下文）。
    pub focused: bool,
    /// IME 组字中的**预编辑文本**（拼音候选未上屏的那部分）。
    ///
    /// 存在理由（FR-05 最高风险项）：组字期间**不能**触发撤销快照——
    /// 否则用户输入「nihao」上屏时会得到 5 个撤销步骤，且中途
    /// 拼音串会短暂出现在文档里被高亮/预览。
    pub composing: Option<String>,
    /// 自动换行。
    pub word_wrap: bool,
    /// 显示行号栏。
    ///
    /// 默认 `false`（隐藏）：真实环境里中文经系统字体回退后行高与等宽
    /// 缓存的行顶表有帧级偏差，行号与正文可能错位；持久化开关在领域
    /// `EditorSettings::show_line_numbers`，启动时由 [`crate::app::frame::EditorApp::new`]
    /// 播种到本字段。
    pub line_numbers: bool,
    /// 拼写检查开关（P2）。
    pub spell_check: bool,
}

impl EditorUiState {
    /// 构造默认状态。
    pub fn new() -> Self {
        Self {
            word_wrap: true,
            line_numbers: false,
            ..Default::default()
        }
    }

    /// 当前选区（无选区为 `None`）。
    ///
    /// 始终返回**归一化**区间（起点 ≤ 终点）：用户拖选方向可能是
    /// 从后往前，若不归一，「选中部分」会算出负长度。
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor.map(|anchor| {
            if anchor <= self.cursor {
                (anchor, self.cursor)
            } else {
                (self.cursor, anchor)
            }
        })
    }

    /// 选区是否为空（光标与锚点重合）。
    pub fn has_selection(&self) -> bool {
        matches!(self.selection(), Some((start, end)) if start != end)
    }

    /// 选中部分的长度（字节）；无选区为 0。
    pub fn selected_len(&self) -> usize {
        self.selection().map_or(0, |(start, end)| end - start)
    }

    /// 移动光标并清除选区。
    pub fn move_cursor(&mut self, offset: usize) {
        self.cursor = offset;
        self.anchor = None;
    }

    /// 移动光标但**保留**选区锚点（Shift+方向键）。
    pub fn extend_to(&mut self, offset: usize) {
        self.anchor.get_or_insert(self.cursor);
        self.cursor = offset;
    }

    /// 设定选区（两端任意顺序）。
    pub fn set_selection(&mut self, start: usize, end: usize) {
        self.anchor = Some(start);
        self.cursor = end;
    }

    /// 清除选区。
    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// 文档内容变化后**夹紧**光标与选区。
    ///
    /// 必需：删光标之前的所有内容会让 `cursor` 超出新长度，
    /// 越界偏移会让 `TextBuffer` 的 `slice` panic
    /// （`panic = "abort"` 下直接终止进程）。
    pub fn clamp_to(&mut self, len: usize) {
        self.cursor = self.cursor.min(len);
        // 锚点不能落在被删除的范围内，否则选区会「反向」跨过光标。
        if let Some(anchor) = self.anchor {
            self.anchor = Some(anchor.min(len));
        }
    }

    /// 文档切换后重置（光标回到开头）。
    pub fn reset_for_new_document(&mut self) {
        self.cursor = 0;
        self.anchor = None;
        self.scroll_y = 0.0;
        self.scroll_x = 0.0;
        self.composing = None;
    }

    // —— IME ——

    /// 开始组字。
    pub fn begin_composing(&mut self, initial: &str) {
        self.composing = Some(initial.to_string());
    }

    /// 更新组字文本（拼音候选变化）。
    pub fn update_composing(&mut self, text: &str) {
        self.composing = Some(text.to_string());
    }

    /// 结束组字并返回**已上屏**的文本。
    ///
    /// 调用方**只**在拿到 `Some` 时才提交编辑命令。
    pub fn end_composing(&mut self) -> Option<String> {
        self.composing.take().filter(|t| !t.is_empty())
    }

    /// **取消**组字（用户按 Esc 或点击别处），丢弃全部未上屏的拼音。
    ///
    /// 与 `end_composing()` 的区别是**语义**上的：组字过程中用户可以中途
    /// 反悔，此时必须区分「上屏提交」与「丢弃」——若两者都走 `end_composing()`
    /// 并把返回值当成新文本插入，就会在文档里留下半截拼音，且因为
    /// FR-05 抑制了组字期间的撤销快照，这段垃圾**无法用撤销键清掉**。
    pub fn cancel_composing(&mut self) {
        self.composing = None;
    }

    /// 是否正在组字。
    pub fn is_composing(&self) -> bool {
        self.composing.is_some()
    }

    /// 组字期间是否应**跳过**撤销快照。
    ///
    /// 这是 FR-05「输入法组合期间不触发撤销快照」的实现点。
    pub fn should_skip_snapshot(&self) -> bool {
        self.is_composing()
    }
}
