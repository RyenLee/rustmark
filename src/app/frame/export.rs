//! 导出浮层的状态写入口。
//!
//! # 为什么单独成文件
//!
//! R-11 子批 1 把 `export_view` 对 `app.export_ui.*` 的直写收敛为方法调用。
//! 落在这里而不是 `actions.rs`，是因为 `actions.rs` 是**路由表**——它只该
//! 回答「这个动作走哪条路」，不该顺带初始化浮层状态（R-11 配套要求）。
//!
//! # 只做「直写 → 方法调用」这一件事
//!
//! 本文件刻意**不**复刻 `ExportUiState` 已有的行为（`open` / `close` /
//! `set_format` / `begin` / `succeed` / `fail` 都在状态结构体上，视图照旧直接
//! 调它们）。这里只补上「表现层原先直接改字段」的那几个入口，不顺手重构选项
//! 的组织方式。
//!
//! # 一处刻意的例外
//!
//! 「输出目录 / 文件名」两个输入框仍以 `TextEdit::singleline(&mut …)` 绑定
//! 在 `export_view` 里：egui 的文本控件必须拿到 `&mut String`，改成方法调用
//! 就只能是「返回一个 `&mut String` 的访问器」，那不过是把直写换了个写法，
//! 所有权并没有收敛。R-11 / R-12 只针对**赋值形态**，这两处是已知边界。

use super::*;

impl EditorApp {
    /// 打开导出浮层：填好建议文件名 / 主题 / 默认输出目录。
    ///
    /// 从 `actions::open_overlay` 下沉而来——原来路由表顺手初始化了这批状态，
    /// 与「路由表只做路由」相悖。
    pub fn open_export_overlay(&mut self) {
        // 建议文件名取当前文档标题去扩展名；导出主题取当前生效主题的种类。
        let stem = self
            .active_document_title_stem()
            .unwrap_or_else(|| "document".to_string());
        // 直接调真源的 `as_str()`，不再手写一份 `match`——
        // 这份手写 `match` 是 R-09 收敛掉的**第三份**主题映射副本。
        let theme_kind = self.ui.theme.kind.as_str();
        self.export_ui.theme = theme_kind.to_string();
        self.export_ui.file_stem = stem;
        // 默认输出目录取文档所在目录；未保存的新文档没有目录，
        // 此时留空并由 `can_submit` 拦下，提示用户先保存或填目录。
        self.export_ui.output_dir = self.active_document_dir().unwrap_or_default();
        self.export_ui.open();
    }

    /// 图片格式：是否导出整篇长图（滚动截取全部内容）。
    pub fn set_export_long_image(&mut self, long: bool) {
        self.export_ui.long_image = long;
    }

    /// 是否包含目录（HTML / Markdown 等通用格式的那个开关）。
    pub fn set_export_include_toc(&mut self, on: bool) {
        self.export_ui.include_toc = on;
    }

    /// 是否把图片内联为 data URI。
    pub fn set_export_embed_images(&mut self, embed: bool) {
        self.export_ui.embed_images = embed;
    }

    /// PDF 纸张尺寸。
    pub fn set_export_page_size(&mut self, size: PageSize) {
        self.export_ui.page_size = size;
    }

    /// 是否按标题分页。
    pub fn set_export_paginate(&mut self, paginate: bool) {
        self.export_ui.paginate = paginate;
    }

    /// PDF 的「包含目录」开关，**连带它的领域规则一起**。
    ///
    /// 领域规则：PDF 的目录必须配分页（`TocRequiresPagination`）。与其让用户
    /// 提交后看到一条他无从理解的错误，不如在勾上目录时自动补上分页。
    ///
    /// 这条规则收在这里而不是留在视图里：视图原先要写**两个**字段
    /// （`include_toc`，再按 `toc` 条件写 `paginate`），规则散在渲染代码中，
    /// 改视图布局时极易只搬走前半句、把「自动补分页」落在原地。
    pub fn set_pdf_include_toc(&mut self, toc: bool) {
        self.export_ui.include_toc = toc;
        if toc {
            self.export_ui.paginate = true;
        }
    }
}
