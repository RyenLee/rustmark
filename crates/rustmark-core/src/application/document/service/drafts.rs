//! 草稿 / 崩溃恢复用例（DEF-02）：落盘 / 读取 / 清理 / 扫描遗留草稿。

use std::path::Path;

use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;
use crate::shared::result::AppResult;

use super::super::queries::*;
use super::DocumentAppService;

impl DocumentAppService {
    // ---------- 草稿 / 恢复（DEF-02）----------

    /// 把未保存草稿落到磁盘（异常退出后可恢复，需求 §4.3 / NFR-REL-03）。
    ///
    /// 内容取自注册表当前态，**不**走领域事件：写草稿只是「崩溃保险」，
    /// 不应污染文档的脏标记或触发保存流程。
    ///
    /// 路径走 [`Self::get_tab_summary`]（O(1) 轻量查询，不做全文快照）：
    /// 草稿要连文档身份一起存，否则恢复出来的文档一律是「未命名」，
    /// 标签条丢失原始文件名（问题 069）。
    pub fn save_draft(&self, drafts_dir: &Path, id: DocumentId) -> AppResult<()> {
        let content = self.get_content(GetContentQuery { id, range: None })?;
        let path = self.get_tab_summary(GetDocumentQuery { id })?.path;
        self.repository
            .save_draft(drafts_dir, id, &content, path.as_ref())
    }

    /// 读取某文档的遗留草稿（不存在返回 `None`）。
    pub(crate) fn load_draft(
        &self,
        drafts_dir: &Path,
        id: DocumentId,
    ) -> AppResult<Option<String>> {
        self.repository.load_draft(drafts_dir, id)
    }

    /// 删除某文档的草稿（已正常保存 / 已恢复 / 已丢弃后清理）。
    pub fn clear_draft(&self, drafts_dir: &Path, id: DocumentId) -> AppResult<()> {
        self.repository.clear_draft(drafts_dir, id)
    }

    /// 扫描草稿目录，返回全部遗留草稿 `(文档 ID, 内容, 路径)`（启动恢复用）。
    ///
    /// 路径来自旁车文件（问题 069）：崩溃前文档的磁盘身份。`None` 表示
    /// 该草稿属于从未保存过的未命名文档，恢复后仍是「未命名」。
    pub fn pending_recoveries(
        &self,
        drafts_dir: &Path,
    ) -> AppResult<Vec<(DocumentId, String, Option<FilePath>)>> {
        let ids = self.repository.list_drafts(drafts_dir)?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(content) = self.load_draft(drafts_dir, id)? {
                let path = self.repository.load_draft_path(drafts_dir, id)?;
                out.push((id, content, path));
            }
        }
        Ok(out)
    }
}
