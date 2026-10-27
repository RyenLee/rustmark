//! 工作区文件树的**已装载条目**快照（R-03 B1 从组合根迁入）。
//!
//! # 为什么它住在 `rustmark-ui`
//!
//! 它是**纯投影**（路径字符串 + 一个布尔），不含任何领域对象，
//! 却住在 root 的 `app/frame/workspace.rs`——于是「快速打开」与「侧栏文件树」
//! 两个视图都得 `use crate::app::frame::…` 才能拿到它。
//!
//! 搬到这里后，trait 的 `workspace_entries_state` 端口可以直接返回
//! `&WorkspaceEntries`，视图不再需要认识组合根。
//!
//! # 公开路径不变
//!
//! root 侧 `src/app/frame.rs` 保留同名 re-export，
//! 故 `rustmark::app::frame::WorkspaceEntries` 路径**逐字不变**。

/// 工作区文件树的**已装载条目**。
///
/// # 为什么不每帧调 `scan_tree`
///
/// 扫描是**磁盘 IO**（`read_dir` 递归 + 最多 10000 条）。每帧做一次，
/// 哪怕窗口最小化也在后台狂读磁盘，文件树还会因为「顺序随文件系统返回序变化」
/// 而不停抖动。故只在打开文件夹 / 文件操作之后显式扫一次，视图每帧只读这份快照。
///
/// 它是**投影**不是缓存的业务真值：磁盘一变就重扫，
/// 快照里没有任何领域对象（只有路径字符串与一个布尔）。
#[derive(Clone, Debug, Default)]
pub struct WorkspaceEntries {
    /// 根目录路径（空串 = 未打开工作区）。
    pub root: Option<String>,
    /// 扁平条目 `(路径, 是否目录)`，已按「目录在前、同层按名称」排序。
    pub items: Vec<(String, bool)>,
}

impl WorkspaceEntries {
    /// 以根目录与条目构造。
    pub fn new(root: String, items: Vec<(String, bool)>) -> Self {
        Self {
            root: Some(root),
            items,
        }
    }

    /// 是否已装载（未打开工作区时为假）。
    pub fn is_loaded(&self) -> bool {
        self.root.is_some()
    }
}
