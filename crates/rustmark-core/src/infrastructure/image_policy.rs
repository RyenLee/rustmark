//! 图片格式白名单：基础设施层的**唯一真源**（预览加载与导出消毒共用）。
//!
//! # 为什么要单独立一个模块
//!
//! 同一份「位图清单」此前由两处各自维护：预览侧的
//! `persistence::asset_loader::policy::ALLOWED_MIME`（完整 MIME，用于
//! data URI 与远程响应的 `Content-Type`）与导出消毒侧的
//! `SAFE_DATA_IMAGE_TYPES`（MIME 子类型，用于判定 `data:image/<sub>;base64,`）。
//! 两处只靠**一句注释**互相约束——改一处漏一处的后果是：同一张图预览放行、
//! 导出被剥（或反之），而两边都不会报错，现象像是「随机丢图」。
//!
//! 现在以**完整 MIME** 为唯一真源，子类型由它推导（去掉 `image/` 前缀），
//! 两处消费同一份数据：要增删允许的图片格式，改本文件一处即可。
//!
//! # 为什么排除 `svg`
//!
//! SVG 是 XML，可以内嵌 `<script>`：内联进预览或导出都等于直接放行脚本，
//! 故永不进本清单。注意与「本地 `.svg` 不被渲染」区分——后者是 `image`
//! crate 未启用 svg 解码带来的**隐式**防护，与本清单无关（见 `SECURITY.md`）。

/// 允许内联 / 加载的位图完整 MIME（小写）。
///
/// **不含 `svg+xml`**，理由见模块文档。
pub(crate) const ALLOWED_IMAGE_MIME: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/jpg",
    "image/gif",
    "image/webp",
    "image/bmp",
];

/// MIME **子类型**（`data:image/png;base64,` 里的 `png`）是否为被放行的位图。
///
/// 刻意用「补回 `image/` 前缀再查同一张表」实现，而不是再写一份子类型清单——
/// 清单一旦有两份，就必须靠注释保持同步，而注释是不会被编译器校验的。
pub(crate) fn is_allowed_image_subtype(subtype: &str) -> bool {
    ALLOWED_IMAGE_MIME
        .iter()
        .any(|mime| mime.strip_prefix("image/") == Some(subtype))
}
