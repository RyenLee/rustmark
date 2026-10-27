//! 原生对话框的后台执行（S6-11f）。
//!
//! # 为什么必须后台线程
//!
//! `RfdDialogAdapter` 用的是 **rfd 的同步 API**（S5-08 的模块文档已写明
//! 「UI 线程不得直接调本适配器」）。同步调用会**阻塞整个窗口**：
//! 用户在系统文件对话框里挑文件的那几十秒里，编辑区不重绘、
//! 菜单点不动、快捷键无反应。在 Windows 上还可能表现为「程序未响应」。
//!
//! # 为什么不用 `rfd` 的异步 API
//!
//! `AsyncFileDialog` 需要一个 async 运行时，而本项目没有任何
//! 运行时依赖（ADR-001 只选了 egui/wgpu）。为一个对话框引入
//! `async-std` / `tokio` 不划算——`std::thread` + `mpsc` 已经够了。
//!
//! # 取消与失败的区分
//!
//! 回填结果用 [`DialogOutcome`] 显式区分 [`AppError::Cancelled`]（用户主动取消，
//! **静默**）与其他错误（对话框不可用、路径非法，**必须提示**）。
//! 两者混成「没选到文件」的后果是：用户取消一次之后看到一条
//! 「打开失败」的错误框，莫名其妙。
//!
//! # 一次只允许一个对话框在飞
//!
//! 真实对话框是系统级模态窗口，两个同时弹出会互相挡在后面。
//! 再次请求时**直接忽略**并记状态，而不是排队——排队会让用户
//! 连点三次打开就得到三个文件对话框。

use std::sync::mpsc::{Receiver, Sender, channel};

use rustmark_core::shared::dialog::{DialogPort, default_filters};
use rustmark_core::shared::error::AppError;
use rustmark_core::shared::path::FilePath;

/// 一次对话框请求（UI 线程登记意图时使用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogRequest {
    /// 打开文件。
    OpenFile,
    /// 另存为（携带建议文件名）。
    SaveFile {
        /// 建议文件名（空串 = 由对话框决定）。
        suggested: String,
    },
    /// 选择工作区目录。
    PickFolder,
    /// 选择图片文件（R-04b：插入图片）。
    PickImage,
    /// 确认框（是 / 否），用于不可逆操作前的二次确认（V11-06）。
    Confirm {
        /// 提示文案（应说明**要删什么**与「不可恢复」）。
        message: String,
    },
}

/// 一次对话框的回填结果。
#[derive(Debug)]
pub enum DialogOutcome {
    /// 打开文件的结果。
    OpenFile(Result<FilePath, AppError>),
    /// 另存为的结果。
    SaveFile(Result<FilePath, AppError>),
    /// 选择目录的结果。
    PickFolder(Result<FilePath, AppError>),
    /// 选择图片的结果（R-04b）。
    PickImage(Result<FilePath, AppError>),
    /// 确认框的结果：`Ok(true)` = 用户点了「是」。
    Confirm(Result<bool, AppError>),
}

impl DialogOutcome {
    /// 取**路径类**结果本体（确认框没有路径，返回 `None`）。
    ///
    /// 为什么不继续用「对所有变体都成立」的签名：确认框的结果是
    /// `Result<bool, _>`，硬塞进 `Result<FilePath, _>` 就得编一个假路径，
    /// 而假路径一旦被当成真路径拿去打开，后果是打开错文件。
    /// 宁可让调用方显式处理 `None`。
    pub fn path_result(&self) -> Option<&Result<FilePath, AppError>> {
        match self {
            Self::OpenFile(r) | Self::SaveFile(r) | Self::PickFolder(r) | Self::PickImage(r) => {
                Some(r)
            }
            Self::Confirm(_) => None,
        }
    }

    /// 是否为「用户主动取消」。
    ///
    /// 只看错误**种类**而不看文案：文案会随版本变，
    /// 按文案判断会在一次措辞调整后静默失效——
    /// 症状是「取消保存也会弹错误框」。
    pub fn is_cancelled(&self) -> bool {
        match self {
            Self::OpenFile(r) | Self::SaveFile(r) | Self::PickFolder(r) | Self::PickImage(r) => {
                matches!(r, Err(AppError::Cancelled))
            }
            Self::Confirm(r) => matches!(r, Err(AppError::Cancelled)),
        }
    }
}

/// 后台对话框执行器。
///
/// 由 [`crate::app::frame::EditorApp`] 持有：每帧调 [`Self::poll`]，
/// 有结果就应用，无结果立即返回（**不做任何等待**）。
pub struct DialogRunner {
    /// 结果接收端。
    rx: Receiver<DialogOutcome>,
    /// 结果发送端（克隆给每个线程）。
    tx: Sender<DialogOutcome>,
    /// 端口句柄（克隆进线程）。
    port: std::sync::Arc<dyn DialogPort>,
    /// 是否有对话框正在等待用户。
    in_flight: bool,
}

impl DialogRunner {
    /// 以给定端口构造。
    pub fn new(port: std::sync::Arc<dyn DialogPort>) -> Self {
        let (tx, rx) = channel();
        Self {
            rx,
            tx,
            port,
            in_flight: false,
        }
    }

    /// 是否有对话框正在等待用户。
    pub fn is_busy(&self) -> bool {
        self.in_flight
    }

    /// 在后台线程执行一次请求；已有对话框在飞时**忽略**本次请求。
    ///
    /// 忽略而非排队：系统对话框互相遮挡，排队的结果是
    /// 用户连点三次「打开」后连弹三个窗口，必须一个个关掉。
    pub fn spawn(&mut self, request: DialogRequest) {
        if self.in_flight {
            return;
        }
        let tx = self.tx.clone();
        let port = self.port.clone();
        self.in_flight = true;
        let _ = std::thread::Builder::new()
            .name("rustmark-dialog".to_string())
            .spawn(move || {
                let outcome = match request {
                    DialogRequest::OpenFile => {
                        DialogOutcome::OpenFile(port.open_file(&default_filters()))
                    }
                    DialogRequest::SaveFile { suggested } => {
                        DialogOutcome::SaveFile(port.save_file(&suggested, &default_filters()))
                    }
                    DialogRequest::PickFolder => DialogOutcome::PickFolder(port.pick_folder()),
                    DialogRequest::PickImage => DialogOutcome::PickImage(port.pick_image()),
                    DialogRequest::Confirm { message } => {
                        DialogOutcome::Confirm(port.confirm(&message))
                    }
                };
                // 接收端已被丢弃（应用退出）时 `send` 只是失败，不 panic。
                let _ = tx.send(outcome);
            });
    }

    /// 取出一个已完成的结果（非阻塞）。
    ///
    /// 返回 `None` 表示「还没选完」，调用方**不得**因此阻塞或提示——
    /// 用户在系统对话框里慢慢挑是正常的。
    pub fn poll(&mut self) -> Option<DialogOutcome> {
        match self.rx.try_recv() {
            Ok(outcome) => {
                self.in_flight = false;
                Some(outcome)
            }
            // 通道断开 = 执行器被丢弃，加载中对话框无法回收。
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.in_flight = false;
                None
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
        }
    }
}
