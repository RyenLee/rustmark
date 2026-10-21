//! 测试用原生对话框替身（S6-11f）。
//!
//! # 为什么必须有它
//!
//! S6-11f 起「打开文件 / 另存为 / 选择文件夹」会**真的**起后台线程调
//! [`DialogPort`]。生产端口是 `RfdDialogAdapter`——同步阻塞、弹出真实的
//! 系统窗口。测试若不换掉端口，就会在无人值守的 CI 上卡在
//! 「等用户点确定」直到超时。
//!
//! # 语义必须与真实实现一致
//!
//! 与 S6-11d 的 `InMemoryWorkspaceRepository` 同一原则：
//! **替身比真实实现宽松，测出来的绿是假的**。这里照搬真实端口的约定：
//!
//! 1. 用户取消返回 `Err(AppError::Cancelled)`，**不是**空路径；
//! 2. 未预置结果时也返回取消（等价于「用户在空对话框里点了取消」）；
//! 3. 记录收到的过滤器与建议文件名，供用例核对请求内容。
//!
//! # 为什么自己定义 `Reply` 而不是存 `Result<FilePath, AppError>`
//!
//! `AppError` **不是 `Clone`**（它带 `io::Error` 这类不可克隆的源）。
//! 而后台线程与 UI 线程之间的交接**必须**克隆或重建结果，
//! 所以这里把「成功 / 取消 / 失败」编码成自己的可克隆枚举，
//! 在 `DialogPort` 实现里再组装回 `AppError`。
//!
//! # 为什么整个文件标 `allow(dead_code)`
//!
//! `tests/support/` 被**每个**测试目标通过 `#[path]` 引入，
//! 而只有 S6-11f 那几个用例真的用到本替身。其余目标（domain /
//! infrastructure …）会为「定义了但没用」报 dead code，
//! 而 `cargo clippy --all-targets -- -D warnings` 把它当错误。
//! 逐个目标加 `#[cfg]` 反而会让「哪个目标能用这个替身」变成
//! 一条需要维护的清单。

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use rustmark::shared::dialog::{DialogPort, FileFilter};
use rustmark::shared::error::AppError;
use rustmark::shared::path::FilePath;

/// 预置的对话框回应（可克隆）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// 返回该路径。
    Path(String),
    /// 用户点了取消。
    Cancelled,
    /// 对话框系统不可用（`AppError::Dialog`）。
    Unavailable(String),
    /// 确认框里点了「是」（`Ok(true)`）。
    Yes,
    /// 确认框里点了「否」（`Ok(false)`）。
    No,
}

impl Reply {
    /// 「返回该路径」。
    pub fn path(p: &str) -> Self {
        Self::Path(p.to_string())
    }

    /// 组装成 [`DialogPort`] **路径类**方法的返回类型。
    ///
    /// `Yes` / `No` 只对确认框有意义（见 [`FakeDialog::set_confirm`]）；
    /// 误用到路径类对话框时按「不可用」报错，而不是静默当成取消——
    /// 否则「测试用例把回应配错了位置」会以「用户取消了」的面貌出现，查不出来。
    fn into_result(self) -> Result<FilePath, AppError> {
        match self {
            Self::Path(p) => FilePath::new(p).map_err(|e| AppError::Dialog {
                message: format!("路径非法：{e}"),
            }),
            Self::Cancelled => Err(AppError::Cancelled),
            Self::Unavailable(m) => Err(AppError::Dialog { message: m }),
            Self::Yes | Self::No => Err(AppError::Dialog {
                message: "是 / 否 只能用于确认框，不能用于路径类对话框".to_string(),
            }),
        }
    }
}

/// 原生对话框替身（克隆共享同一份状态）。
#[derive(Clone, Debug, Default)]
pub struct FakeDialog {
    /// 内部状态。
    inner: Arc<Mutex<State>>,
}

/// 替身内部状态。
#[derive(Clone, Debug, Default)]
struct State {
    open_file: Option<Reply>,
    save_file: Option<Reply>,
    pick_folder: Option<Reply>,
    pick_image: Option<Reply>,
    confirm: Option<Reply>,
    last_filters: Vec<FileFilter>,
    last_suggested: String,
    last_message: String,
    open_calls: usize,
    save_calls: usize,
    folder_calls: usize,
    image_calls: usize,
    confirm_calls: usize,
}

impl FakeDialog {
    /// 构造一个「全部取消」的替身。
    pub fn new() -> Self {
        Self::default()
    }

    /// 构造一个「打开文件返回该路径」的替身。
    pub fn opening(path: &str) -> Self {
        let fake = Self::new();
        fake.lock().open_file = Some(Reply::path(path));
        fake
    }

    /// 构造一个「另存为返回该路径」的替身。
    pub fn saving(path: &str) -> Self {
        let fake = Self::new();
        fake.lock().save_file = Some(Reply::path(path));
        fake
    }

    /// 构造一个「选择文件夹返回该路径」的替身。
    pub fn picking(path: &str) -> Self {
        let fake = Self::new();
        fake.lock().pick_folder = Some(Reply::path(path));
        fake
    }

    /// 构造一个「选择图片返回该路径」的替身（R-04b）。
    pub fn picking_image(path: &str) -> Self {
        let fake = Self::new();
        fake.lock().pick_image = Some(Reply::path(path));
        fake
    }

    /// 设置「打开文件」的回应。
    pub fn set_open_file(&self, reply: Reply) {
        self.lock().open_file = Some(reply);
    }

    /// 设置「另存为」的回应。
    pub fn set_save_file(&self, reply: Reply) {
        self.lock().save_file = Some(reply);
    }

    /// 设置「选择文件夹」的回应。
    pub fn set_pick_folder(&self, reply: Reply) {
        self.lock().pick_folder = Some(reply);
    }

    /// 设置「选择图片」的回应（R-04b）。
    pub fn set_pick_image(&self, reply: Reply) {
        self.lock().pick_image = Some(reply);
    }

    /// 「选择图片」的调用次数。
    pub fn image_calls(&self) -> usize {
        self.lock().image_calls
    }

    /// 设置「确认框」的回应（`Reply::Yes` / `Reply::No` / `Reply::Cancelled`）。
    ///
    /// 未设置时按**取消**处理——与其余三类一致（「用户在空对话框里点了取消」）。
    /// 这正好是 V11-06 要钉住的默认语义：不确认就不删。
    pub fn set_confirm(&self, reply: Reply) {
        self.lock().confirm = Some(reply);
    }

    /// 最近一次确认框收到的提示文案（用于断言「确认框说清了要删什么」）。
    pub fn last_message(&self) -> String {
        self.lock().last_message.clone()
    }

    /// 确认框的调用次数。
    pub fn confirm_calls(&self) -> usize {
        self.lock().confirm_calls
    }

    /// 最近一次收到的过滤器。
    pub fn last_filters(&self) -> Vec<FileFilter> {
        self.lock().last_filters.clone()
    }

    /// 最近一次收到的建议文件名。
    pub fn last_suggested(&self) -> String {
        self.lock().last_suggested.clone()
    }

    /// 三类请求各自的调用次数 `(open, save, folder)`。
    pub fn call_counts(&self) -> (usize, usize, usize) {
        let s = self.lock();
        (s.open_calls, s.save_calls, s.folder_calls)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // 测试替身里的 `expect` 可接受：拿不到锁说明测试自己写错了
        // （同一次断言里又取又放），不是被测代码的问题。
        self.inner.lock().expect("替身状态锁不应被毒化")
    }
}

impl DialogPort for FakeDialog {
    fn open_file(&self, filters: &[FileFilter]) -> Result<FilePath, AppError> {
        let mut s = self.lock();
        s.open_calls += 1;
        s.last_filters = filters.to_vec();
        s.open_file
            .clone()
            .unwrap_or(Reply::Cancelled)
            .into_result()
    }

    fn save_file(
        &self,
        suggested_name: &str,
        filters: &[FileFilter],
    ) -> Result<FilePath, AppError> {
        let mut s = self.lock();
        s.save_calls += 1;
        s.last_filters = filters.to_vec();
        s.last_suggested = suggested_name.to_string();
        s.save_file
            .clone()
            .unwrap_or(Reply::Cancelled)
            .into_result()
    }

    fn pick_folder(&self) -> Result<FilePath, AppError> {
        let mut s = self.lock();
        s.folder_calls += 1;
        s.pick_folder
            .clone()
            .unwrap_or(Reply::Cancelled)
            .into_result()
    }

    fn pick_image(&self) -> Result<FilePath, AppError> {
        let mut s = self.lock();
        s.image_calls += 1;
        s.pick_image
            .clone()
            .unwrap_or(Reply::Cancelled)
            .into_result()
    }

    fn confirm(&self, message: &str) -> Result<bool, AppError> {
        let mut s = self.lock();
        s.confirm_calls += 1;
        s.last_message = message.to_string();
        match s.confirm.clone().unwrap_or(Reply::Cancelled) {
            Reply::Yes => Ok(true),
            Reply::No => Ok(false),
            Reply::Cancelled => Err(AppError::Cancelled),
            Reply::Unavailable(m) => Err(AppError::Dialog { message: m }),
            // 路径类回应对确认框无意义：按「不可用」处理，让调用方显式报错，
            // 而不是静默当成「否」把删除意图吞掉。
            Reply::Path(p) => Err(AppError::Dialog {
                message: format!("确认框收到路径类回应 {p}"),
            }),
        }
    }

    fn show_error(&self, _message: &str) -> Result<(), AppError> {
        Ok(())
    }
}
