// 架构边界校验器 —— 由 `build.rs` 与 `src/bin/check-arch.rs` 通过 `include!` 共用。
//
// 只扫描 `use` 语句（不解析 AST）：牺牲对宏内引用的识别，换取零额外依赖与极快的构建期校验。
// 逃生口：行尾注释 `// arch-allow: <crate>` 可豁免该行，豁免须在 `.agent/architecture.md` 登记。

use std::fs;
use std::path::{Path, PathBuf};

/// 一处架构违规。
pub struct Violation {
    /// 相对路径（便于 CI 日志直接阅读）。
    pub file: String,
    /// 行号（1-based）。
    pub line: usize,
    /// 违规说明。
    pub detail: String,
}

const DOMAIN_FORBIDDEN: &[&str] = &[
    "egui",
    "eframe",
    "rfd",
    "syntect",
    "pulldown_cmark",
    "std::fs",
    "std::io",
    "anyhow",
    "crate::application",
    "crate::infrastructure",
    "crate::presentation",
];

const APPLICATION_FORBIDDEN: &[&str] = &[
    "egui",
    "eframe",
    "rfd",
    "syntect",
    "pulldown_cmark",
    "std::fs",
    "std::io",
    "crate::infrastructure",
    "crate::presentation",
];

const INFRASTRUCTURE_FORBIDDEN: &[&str] = &["crate::application", "crate::presentation"];

const PRESENTATION_FORBIDDEN: &[&str] = &["crate::domain", "crate::infrastructure"];

/// `presentation/state/` 的额外禁令（S6-12）。
///
/// # 为什么要单独一条
///
/// [`PRESENTATION_FORBIDDEN`] 已经禁掉了整个 `crate::domain`，
/// 因此本条**在当前代码里不会新增任何拦截**。它的价值是**防未来松动**：
/// 若哪天有人为了图方便把 `PRESENTATION_FORBIDDEN` 放宽
/// （例如「状态层要读个文档摘要应该没事」），`state/` 这条独立规则
/// 仍在原地，把破坏挡在视图层之外。
///
/// 代价是**一条当前不生效的规则**。这个代价是值得付的：
/// 它的失效场景是「有人同时改了规则**和**代码」，
/// 而那正是最需要守卫的时刻。
const PRESENTATION_STATE_FORBIDDEN: &[&str] = &["crate::domain::document"];

const SHARED_FORBIDDEN: &[&str] = &[    "egui",
    "eframe",
    "rfd",
    "syntect",
    "pulldown_cmark",
    "crate::domain",
    "crate::application",
    "crate::infrastructure",
    "crate::presentation",
];

/// 定时任务「到期帧」预约的唯一出口（T11 / ADR-009）。
///
/// # 为什么这是一条架构规则，而不是编码风格
///
/// egui 是**事件驱动重绘**：没有输入、没有动画时不产生新帧。任何「等 X 毫秒
/// 后再做一件事」的逻辑，都必须有人在那个时刻预约一帧，否则功能静默失效
/// （问题 068：自动保存完全不生效）。历史上这个预约是**逐处手写**的，
/// 于是统计防抖、高亮补算、消息 TTL 三处都漏了，只靠编辑区光标闪烁兜底。
///
/// 现在只在 `FrameScheduler::flush` 里预约，调用点只 `arm` 到期点。
/// 这里用文本扫描守住它：若哪天有人在别处直接写 `request_repaint_after`，
/// 「漏预约」的坑就会以完全相同的形式回来，而它不产生编译错误、单测也未必覆盖。
const REPAINT_RESERVATION: &str = "request_repaint_after";
const REPAINT_RESERVATION_HOME: &str = "app/frame/scheduler.rs";

/// 取某层的禁用前缀。
///
/// **子层是叠加而非替换**：`presentation/state` 先取父层
/// [`PRESENTATION_FORBIDDEN`]，再把 [`PRESENTATION_STATE_FORBIDDEN`]
/// 接在后面。若写成只返回子层名单，`state/` 就不再受
/// `crate::infrastructure` 等父层禁令约束——那是**放宽**而非收紧，
/// 与「子层更严」的意图正好相反。
fn forbidden_for(layer: &str) -> Vec<&'static str> {
    match layer {
        "domain" => DOMAIN_FORBIDDEN.to_vec(),
        "application" => APPLICATION_FORBIDDEN.to_vec(),
        "infrastructure" => INFRASTRUCTURE_FORBIDDEN.to_vec(),
        "presentation" => PRESENTATION_FORBIDDEN.to_vec(),
        "presentation/state" => {
            let mut v = PRESENTATION_FORBIDDEN.to_vec();
            v.extend_from_slice(PRESENTATION_STATE_FORBIDDEN);
            v
        }
        "shared" => SHARED_FORBIDDEN.to_vec(),
        _ => Vec::new(),
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}

/// 由源文件相对 `src/` 的路径判定所属层。
///
/// `main.rs` / `lib.rs` / `bin/` 属于组合根或工具，不做限制（返回 `""`）。
/// `presentation/state/` 单独成层（子层）——见 [`PRESENTATION_STATE_FORBIDDEN`]。
fn layer_of(rel: &str) -> String {
    // `rel` 已统一为 `/` 分隔（Windows 下亦如此），故按 `/` 切分，不能用 MAIN_SEPARATOR。
    let mut parts = rel.split('/');
    match parts.next() {
        Some("domain") => "domain".to_string(),
        Some("application") => "application".to_string(),
        Some("infrastructure") => "infrastructure".to_string(),
        Some("presentation") => match parts.next() {
            Some("state") => "presentation/state".to_string(),
            _ => "presentation".to_string(),
        },
        Some("shared") => "shared".to_string(),
        _ => String::new(),
    }
}

/// 文件指纹：mtime（纳秒）+ 字节长度。
///
/// 只用秒级 mtime 会让「同一秒内的两次写入」产生相同指纹（在快速重写文件的场景下会漏检）。
fn file_stamp(path: &Path) -> String {
    let modified = fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    format!("{modified}:{len}")
}

/// 扫描 `src_root` 下的全部 `.rs` 文件，返回违规列表（空表示通过）。
pub fn scan(src_root: &Path) -> Vec<Violation> {
    let mut files = Vec::new();
    collect_rs_files(src_root, &mut files);
    files.sort();

    let mut violations = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(src_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let layer = layer_of(&rel);
        if layer.is_empty() {
            continue;
        }
        let forbidden = forbidden_for(&layer);
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        for (idx, raw_line) in content.lines().enumerate() {
            let line_no = idx + 1;
            let line = raw_line.trim();

            // 定时任务的帧预约只允许一个出口（T11 / ADR-009）。
            // 先剥掉行尾注释：文档里大段解释「为什么不能自己预约」时会出现这个词。
            let code = line.split("//").next().unwrap_or(line);
            if code.contains(REPAINT_RESERVATION)
                && !rel.ends_with(REPAINT_RESERVATION_HOME)
            {
                violations.push(Violation {
                    file: rel.clone(),
                    line: line_no,
                    detail: format!(
                        "定时任务的帧预约必须走 FrameScheduler（{REPAINT_RESERVATION} 只允许出现在 \
                         {REPAINT_RESERVATION_HOME}）：调用点只 `arm` 到期点，否则「新增定时功能\
                         就漏预约」的老问题会原样复发"
                    ),
                });
            }

            // 领域层禁止向 stdout 直接输出
            if layer == "domain" && line.contains("println!") && !line.starts_with("//") {
                violations.push(Violation {
                    file: rel.clone(),
                    line: line_no,
                    detail: "领域层禁止 println!（应通过错误或事件表达）".to_string(),
                });
            }

            let Some(rest) = line.strip_prefix("use ") else {
                continue;
            };
            if line.contains("arch-allow:") {
                continue;
            }
            // 必须先剥离行尾注释再去分号：`use egui; // 说明` 若先 trim_end_matches(';')，
            // 得到的是 `egui; // 说明`，永远匹配不到 `egui`（该缺陷由注入实验发现）。
            let without_comment = rest.split("//").next().unwrap_or(rest);
            let use_path = without_comment.trim().trim_end_matches(';').trim();
            for &banned in &forbidden {
                if use_path == banned || use_path.starts_with(&format!("{banned}::")) {
                    // 只给**子层自己那条**禁令附「为什么」。父层禁令也走这个
                    // 循环，若无条件附加，`use crate::infrastructure` 在
                    // `state/` 里也会显示「业务数据来自 DTO」——
                    // 那句话跟基础设施依赖无关，是误导。
                    let reason = if layer == "presentation/state"
                        && PRESENTATION_STATE_FORBIDDEN.contains(&banned)
                    {
                        "（UI State 只放视图状态：开关 / 滚动 / 焦点 / 临时输入；\
                         业务数据一律来自 DTO，见 docs/06 表现层与交互实现）"
                    } else {
                        ""
                    };
                    violations.push(Violation {
                        file: rel.clone(),
                        line: line_no,
                        detail: format!("{layer} 层禁止依赖 {banned}（use {use_path}）{reason}"),
                    });
                }
            }
        }
    }
    violations
}

/// 源文件指纹（路径 + mtime），用于 `build.rs` 缓存判定是否需要重扫。
#[allow(dead_code)] // 仅 build.rs 使用；check-arch bin 走全量扫描
pub fn signature(src_root: &Path) -> String {
    let mut files = Vec::new();
    collect_rs_files(src_root, &mut files);
    files.sort();
    let mut sig = String::new();
    for path in files {
        let rel = path
            .strip_prefix(src_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        sig.push_str(&format!("{rel}:{}\n", file_stamp(&path)));
    }
    sig
}
