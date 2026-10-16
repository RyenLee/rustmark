//! 拖放分类的纯函数（不 IO、不依赖 EditorApp）。

use crate::shared::text_format::TextFormats;

/// 把一批拖入的路径分成「可打开」与「应拒绝」两组（纯函数，便于单测）。
///
/// 判定走**全应用唯一**的格式白名单（[`TextFormats`]，问题 045）——
/// 此前这里自带一份 `ends_with(".md")` 之类的清单，与文件树的打开守卫
/// 各判一套：白名单在某一边加了一种格式，另一边就会「拖进来能开、
/// 双击却打不开」（`.csv` 曾经就是这种半开半闭的状态）。
///
/// 不在此处做任何 IO：是否真的能打开由 [`EditorApp::open_path`] 决定，
/// 这里只按扩展名做**意图分类**——它的唯一职责是把「拒绝」的反馈
/// 从逐文件一条错误变成一次汇总提示。
pub(crate) fn classify_dropped(
    formats: &TextFormats,
    paths: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut open = Vec::new();
    let mut reject = Vec::new();
    for p in paths {
        if formats.accepts(p) {
            open.push(p.clone());
        } else {
            reject.push(p.clone());
        }
    }
    (open, reject)
}
