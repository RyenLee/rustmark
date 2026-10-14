//! 最小外部打开器与链接安全校验（共享内核，零外部依赖）。
//!
//! egui 在本项目编译时未启用 `open_url` feature（见 `Cargo.toml` 的
//! `egui = { ... features = ["default_fonts"] }`），且项目对运行时依赖一贯
//! 严控（数学 / 图表均自研），因此不引入 `webbrowser` crate，而是用
//! 操作系统自带的默认程序打开——零额外依赖，桌面端行为与原生应用一致。
//!
//! 放在 `shared` 而非 `infrastructure`：打开链接是表现层（预览区）需要的能力，
//! 而架构约束禁止 `presentation` 依赖 `crate::infrastructure`
//! （见 `.agent/architecture.md` 依赖矩阵）。`shared` 可被任意层依赖，且
//! `std::process` 不在其禁止清单，故在此落位。

use std::process::Command;

/// 用系统默认程序打开一个外部地址 / 本地文件。
///
/// 行为对齐「在浏览器中打开链接」：http(s) / mailto 交浏览器 / 邮件客户端，
/// 本地路径交文件管理器 / 关联程序。`spawn` 而非 `wait`：不阻塞 UI 帧。
///
/// 失败只返回 `Err`，不 panic——打开器不可用（或平台不支持）不应拖垮编辑器。
pub fn open_external(target: &str) -> std::io::Result<()> {
    // `cmd /C start "" "url"` 的空 `""` 是 `start` 的「窗口标题」占位，
    // 必须保留，否则当 url 自身带引号 / 空格时会被 `start` 误判为标题。
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", target])
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(target).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(target).spawn()?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = target;
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "unsupported platform for open_external",
        ));
    }
    Ok(())
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
