//! 外部打开器端口与链接安全校验（rustmark-ports，R-10 项 A 从 shared 抽出，零外部依赖）。
//!
//! egui 在本项目编译时未启用 `open_url` feature（见 `Cargo.toml` 的
//! `egui = { version = "0.37.0", default-features = false }`），且项目对运行时依赖一贯
//! 严控（数学 / 图表均自研），因此不引入 `webbrowser` crate，而是用
//! 操作系统自带的默认程序打开——零额外依赖，桌面端行为与原生应用一致。
//!
//! # 为什么执行端不在这里
//!
//! `rustmark-ports` 是**零 IO** 的共享内核（R-03 起 `std::process` 亦进
//! [`SHARED_FORBIDDEN`]，见 `docs/spec/architecture-constraints.md`）。早先
//! 「`std::process` 不在禁止清单，故在此落位」是**绕规则落位**——文件头
//! 自认绕过，与「零外部依赖」的自我描述矛盾。现按 `DialogPort` / `Clock`
//! 的既有范式拆开：
//!
//! - **端口**（[`ExternalOpener`] trait）留在这里——表现层（预览区）需要
//!   这个能力，而架构约束禁止 `presentation` 依赖 `crate::infrastructure`；
//!   `rustmark-ports` 可被任意层依赖，端口本身零 IO；
//! - **实现**（三平台 `Command::new(...).spawn()`）移到
//!   `infrastructure::system::SystemExternalOpener`；
//! - **链接安全校验**（[`is_safe_to_open`]，纯逻辑）**必须留在这里**——
//!   若随执行一起搬走，`rustmark-ports` 会重新含 IO，且校验逻辑会被拆到
//!   infrastructure 侧，表现层再想复用就得反向依赖 infrastructure。

/// 用系统默认程序打开一个外部地址 / 本地文件。
///
/// 行为对齐「在浏览器中打开链接」：http(s) / mailto 交浏览器 / 邮件客户端，
/// 本地路径交文件管理器 / 关联程序。实现应 `spawn` 而非 `wait`——不阻塞 UI 帧。
///
/// 失败只返回 `Err`，不 panic——打开器不可用（或平台不支持）不应拖垮编辑器。
///
/// # 为什么这是个 trait 而不是自由函数
///
/// 预览区点击外链是**用户可见**行为，而真起进程的调用在测试里无法断言
/// （`spawn` 一个真 `cmd` 既不幂等也没法观察）。抽成端口后，测试可注入
/// 假实现，断言「点了链接、参数正确、且**只在通过安全校验后**才调」。
///
/// 与 [`crate::dialog::DialogPort`] / [`crate::time::Clock`]
/// 同一范式：端口在 `rustmark-ports`，实现在 `infrastructure`。
pub trait ExternalOpener: Send + Sync {
    /// 打开 `target`（URL 或本地路径）。
    fn open(&self, target: &str) -> std::io::Result<()>;
}

/// 该链接目标是否可安全打开（拒绝 XSS 向量）。
///
/// 与 `export::html_sanitize::is_safe_url` 的取向一致但面向「点击跳转」：
/// 绝对协议只放 `http/https/mailto/tel`（拒绝 `javascript:` / `vbscript:` /
/// `data:` 等非位图 data URI）；相对路径与 `file://` 视为本地文件，允许；
/// `#锚点`（内部跳转）在调用方单独处理，这里返回 `true` 以免误杀。
pub fn is_safe_to_open(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return false;
    }
    // 内部锚点：交给调用方做标题跳转，本身无害。
    if trimmed.starts_with('#') {
        return true;
    }
    // XSS 向量：这些 scheme 通常**不含** `://`，若只靠下方 `://` 判定会漏过，
    // 故在 `://` 检查之前显式拒绝。scheme 大小写不敏感（`DATA:` / `JavaScript:`
    // 等同），按小写前缀判定。即便将来有 `javascript://` 形态也一并挡下。
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("javascript:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("data:")
    {
        return false;
    }
    // 相对路径（不含协议分隔符）或显式 file://：本地文件，允许。
    if trimmed.starts_with("file://") || !trimmed.contains("://") {
        return true;
    }
    // 绝对协议白名单（与导出清洗同源，但导出侧还含 `tel:`，这里同样放行）。
    matches!(
        trimmed.split("://").next(),
        Some("http") | Some("https") | Some("mailto") | Some("tel")
    )
}

#[cfg(test)]
mod tests {
    use super::is_safe_to_open;

    /// 端口必须能装进 `Arc<dyn ExternalOpener>` 并跨线程传递（`Services`
    /// 持有它，对话框端口同为 `Send + Sync`）。这条是**编译期**约束：
    /// 若哪天有人给 trait 去掉 `Send + Sync`，实现侧 `Arc<dyn …>` 会编译失败。
    #[test]
    fn opener_port_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn super::ExternalOpener>();
    }

    #[test]
    fn rejects_xss_protocols() {
        // 这些 scheme 不含 `://`，必须有单独拦截，否则会被「相对路径」分支放过。
        assert!(!is_safe_to_open("javascript:alert(1)"));
        assert!(!is_safe_to_open("javascript://evil.com"));
        assert!(!is_safe_to_open("vbscript:msgbox"));
        assert!(!is_safe_to_open("data:text/html,<script>"));
        assert!(!is_safe_to_open("DATA:image/png;base64,xxxx"));
    }

    #[test]
    fn allows_safe_protocols_and_local() {
        assert!(is_safe_to_open("https://example.com"));
        assert!(is_safe_to_open("http://example.com"));
        assert!(is_safe_to_open("mailto:a@b.com"));
        assert!(is_safe_to_open("tel:+123456"));
        assert!(is_safe_to_open("relative/path.md"));
        assert!(is_safe_to_open("./foo.png"));
        assert!(is_safe_to_open("file:///C:/x.png"));
        // 内部锚点：交给调用方跳转，本身无害。
        assert!(is_safe_to_open("#section"));
    }

    #[test]
    fn rejects_empty_or_whitespace() {
        assert!(!is_safe_to_open(""));
        assert!(!is_safe_to_open("   "));
    }
}
