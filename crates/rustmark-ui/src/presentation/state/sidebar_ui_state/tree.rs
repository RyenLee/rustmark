//! 文件树的投影模型：扁平扫描条目折成带缩进的可见行，以及名字编辑提交。

use rustmark_core::shared::path::file_name_of;

/// 文件树的一行（**投影**，非缓存）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TreeRow {
    /// 完整路径。
    pub path: String,
    /// 显示名（末段）。
    pub name: String,
    /// 缩进层级（根为 0）。
    pub depth: usize,
    /// 是否为目录。
    pub is_directory: bool,
    /// 是否为该层级的最后一项（决定树线的「└」还是「├」）。
    pub is_last: bool,
    /// 是否有后续兄弟（即不是该层最后一项）。
    pub(crate) has_next_sibling: bool,
}

impl TreeRow {
    /// 该行是否应显示展开箭头。
    pub const fn is_expandable(&self) -> bool {
        self.is_directory
    }
}

/// 一次「名字编辑」的提交内容（新建或重命名）。
///
/// 用枚举而非 `(String, String, bool, bool)`：四种布尔组合里有两个字段
/// 永远不可能同时为真（新建与重命名互斥），用位置不同的布尔表达互斥关系，
/// 调用点只能靠注释记住「第四个布尔是 is_new」——迟早写反。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NameEdit {
    /// 新建：`(父目录, 名字, 是否目录)`；父目录为空串表示工作区根。
    Create {
        /// 父目录路径（空串 = 根目录）。
        parent: String,
        /// 新名字。
        name: String,
        /// 是否为目录。
        is_directory: bool,
    },
    /// 重命名：`(原路径, 新名字)`。
    Rename {
        /// 原路径。
        path: String,
        /// 新名字。
        name: String,
    },
}

impl NameEdit {
    /// 提交的名字（两种情况共有）。
    pub fn name(&self) -> &str {
        match self {
            Self::Create { name, .. } | Self::Rename { name, .. } => name,
        }
    }

    /// 名字是否可用（非空且非纯空白）。
    pub fn has_usable_name(&self) -> bool {
        !self.name().trim().is_empty()
    }
}

/// 把扁平的扫描条目折成带缩进的可见行。
///
/// # 为什么要「折」而不是直接画
///
/// `scan` 返回的是**深度优先的前序扁平列表**（父目录紧跟其子项），
/// 而界面需要的是「当前展开状态下可见的那些行」——未展开的目录
/// 其子树整体不画。折行必须知道每个条目的**深度**与**是否最后一项**，
/// 这两个信息在扁平列表里不存在，只能靠路径逐级推导。
///
/// # 三个必须处理的边界
///
/// 1. **顺序**：假定输入已按「目录在前、同层按名称」排好（`FsWorkspaceRepository`
///    保证）。若自行重排，「重命名后位置不动」这个预期就没了。
/// 2. **深度**：按**路径分隔符个数**相对根计算，而不是「数前面有多少个目录条目」——
///    后者在条目列表不完整时（如跳过隐藏目录）会算错。
/// 3. **折叠的目录自身仍要显示**：收起 `a/` 时 `a` 这行还在，只是它的子项不画。
pub fn tree_rows(
    entries: &[(String, bool)],
    root: &str,
    expanded: &dyn Fn(&str) -> bool,
) -> Vec<TreeRow> {
    let root_depth = depth_of(root);
    let mut out: Vec<TreeRow> = Vec::with_capacity(entries.len());
    for (path, is_directory) in entries {
        if !is_under_root(path, root) {
            continue;
        }
        let depth = depth_of(path).saturating_sub(root_depth + 1);
        // 父目录没展开 → 整棵子树不画。但这一行本身要画（收起时仍可见）。
        if depth > 0 && !visible_ancestors(path, root, expanded) {
            continue;
        }
        let name = file_name_of(path).to_string();
        out.push(TreeRow {
            path: path.clone(),
            name,
            depth,
            is_directory: *is_directory,
            is_last: false,
            has_next_sibling: false,
        });
    }
    // 标注「是否最后一项」：同深度的相邻两行，前者不是最后一项。
    // 逐层扫一遍即可，条目数上限 10000，代价可忽略。
    for i in 0..out.len() {
        let (depth, path) = (out[i].depth, out[i].path.clone());
        let has_next = out
            .iter()
            .skip(i + 1)
            .find(|row| row.depth <= depth)
            .is_some_and(|row| row.depth == depth && row.path != path);
        out[i].has_next_sibling = has_next;
        out[i].is_last = !has_next;
    }
    out
}

/// 某路径是否在根目录之下（含根本身）。
fn is_under_root(path: &str, root: &str) -> bool {
    if root.is_empty() {
        return true;
    }
    path == root || path.starts_with(&format!("{root}/"))
}

/// 路径的深度（`/` 分隔的段数）。
fn depth_of(path: &str) -> usize {
    if path.is_empty() {
        return 0;
    }
    path.split('/').filter(|s| !s.is_empty()).count()
}

/// `path` 的所有祖先目录（不含自身）是否都已展开。
fn visible_ancestors(path: &str, root: &str, expanded: &dyn Fn(&str) -> bool) -> bool {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let root_depth = depth_of(root);
    let mut current = String::from(root);
    // 逐级向下拼，每到一级目录就问一次「展开了吗」。
    for segment in segments.iter().skip(root_depth) {
        current = if current.is_empty() {
            segment.to_string()
        } else {
            format!("{current}/{segment}")
        };
        if current == path {
            return true;
        }
        if !expanded(&current) {
            return false;
        }
    }
    true
}
