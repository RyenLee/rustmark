//! 差异对比浮层的状态写入口。
//!
//! 与 [`super::export`] 同构（R-11 子批 2）：表现层不再直写 `app.diff_ui.*`，
//! 改走这里的方法；`open_overlay` 里那段 diff 特有的初始化也从路由表下沉到这里。
//!
//! # 只做「直写 → 方法调用」这一件事
//!
//! `DiffUiState` 自身已有的行为（`open` / `recompute` / `visible_rows` /
//! `is_identical`）仍在状态结构体上，视图照旧直接调。这里只补上「表现层原先
//! 直接改字段」的那几个入口。
//!
//! # 一处刻意的例外
//!
//! 「只看差异行」开关仍以 `ui.checkbox(&mut app.diff_ui.only_changes, …)`
//! 绑定：egui 的复选框必须拿到 `&mut bool`，改成方法调用就只能是「返回一个
//! `&mut bool` 的访问器」，那不过是把直写换个写法，所有权并未收敛。
//! R-11 / R-12 只针对**赋值形态**，这一处是已知边界。

use super::*;

impl EditorApp {
    /// 打开差异对比浮层：只在**还没选对照文件**时清掉上一次的错误。
    ///
    /// 为什么不是无条件清：保留上一次选的对照文件是刻意的（用户关掉再打开
    /// 大概率还是想看同一对）。既然文件还在，上一次读失败的原因也还成立，
    /// 无脑清掉会让面板显示「两侧内容完全一致」——而真相是根本没读进来。
    pub fn open_diff_overlay(&mut self) {
        if self.diff_ui.right_path.is_none() {
            self.diff_ui.error = None;
        }
    }

    /// 记下一条对比错误（读取对照文件失败等）。
    ///
    /// 必须**明说**而不是静默返回空串：空串会被当成「两侧一致」，
    /// 用户得到的结论与事实完全相反。
    pub fn set_diff_error(&mut self, message: impl Into<String>) {
        self.diff_ui.error = Some(message.into());
    }

    /// 换一个对照文件：清空选择，并清掉上一份结果的行表。
    ///
    /// 两步必须一起做：只清 `right_path` 会留下上一次的行表，面板在
    /// 「没有对照文件」的分支里虽不显示它们，但下次选文件时若读取失败，
    /// 旧行会与错误提示同时出现（结果说一致、错误说没读进来）。
    pub fn clear_diff_selection(&mut self) {
        self.diff_ui.right_path = None;
        self.diff_ui.rows.clear();
    }
}
