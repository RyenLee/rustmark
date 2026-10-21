//! 侧栏（文件树 / 大纲 / 搜索）的状态写入口。
//!
//! # 为什么单独成文件
//!
//! R-11 子批 3 把 `sidebar_view` 对 `app.sidebar.*` 的直写收敛为方法调用，
//! 与 [`super::export`] / [`super::diff`] 同构：表现层只读状态、只提意图，
//! 写操作一律走这里。
//!
//! # 只做「直写 → 方法调用」这一件事
//!
//! `SidebarUiState` 自身已有的行为（`start_rename` / `start_create` /
//! `toggle_expanded` / `open_context_menu` …）继续由视图直接调用——那些是
//! **状态结构体自己的方法**，不是「视图直写字段」，不在 R-11 范围内。
//! 本文件只收「视图直接给字段赋值」的形态。

use super::*;

use crate::presentation::state::sidebar_ui_state::SearchToggle;

impl EditorApp {
    /// 恢复**新建**草稿（提交失败时把用户输入放回编辑态，见 `restore_draft`）。
    ///
    /// 与 `SidebarUiState::start_create` **不等价**：后者是「开始新建」入口，
    /// 会预填默认名并清掉菜单 / 重命名 / 错误；这里只回放用户原本敲的三个
    /// 值，错误提示与其它状态原样保留——否则失败原因会被顺手抹掉，用户
    /// 不知道自己错在哪。
    pub fn restore_create_draft(&mut self, parent: String, name: String, is_directory: bool) {
        self.sidebar.creating = Some((parent, name, is_directory));
    }

    /// 恢复**重命名**草稿（同上，只回放两个字段，不动菜单与错误）。
    pub fn restore_rename_draft(&mut self, path: String, name: String) {
        self.sidebar.renaming_path = Some(path);
        self.sidebar.rename_draft = name;
    }

    /// 单选文件树某行（只改高亮，不开文档——打开是双击的动作）。
    pub fn select_sidebar_path(&mut self, path: String) {
        self.sidebar.selected_path = Some(path);
    }

    /// 选中大纲某条（与 `select_sidebar_path` 同理：纯高亮，跳转由调用方
    /// 另走 `goto_outline_line`，两个动作不在这里耦合）。
    pub fn select_outline_item(&mut self, text: String) {
        self.sidebar.outline_selected = Some(text);
    }

    /// 写入搜索框当前文本（响应 `TextEdit::changed` 的回写）。
    pub fn set_search_query(&mut self, query: String) {
        self.sidebar.search_query = query;
    }

    /// 切换搜索范围（当前文档 / 打开的文档）。
    pub fn set_search_scope(&mut self, scope: SearchScope) {
        self.sidebar.search_scope = scope;
    }

    /// 翻转一个搜索开关（区分大小写 / 全字匹配 / 正则）。
    ///
    /// 开关是**双态**的：再点一次即取消。`toggled` 的计算放在这里而不是
    /// 视图里——「基于旧值算新值」属于状态演化，不该散进渲染代码
    /// （与 `toggle_editor_spell_check` 等同族的既定模式）。
    pub fn toggle_search_option(&mut self, which: SearchToggle) {
        self.sidebar.search_options = self.sidebar.search_options.toggled(which);
    }
}
