//! 预览渲染用例：大文件降级判定、渲染缓存读写、内容获取。

use crate::domain::markdown::value_objects::RenderedBlock;
use crate::shared::error::AppError;
use crate::shared::ids::DocumentId;
use crate::shared::result::AppResult;

use crate::application::dto::RenderedView;

use super::super::queries::*;
use super::view_mapper::to_block_view;
use super::{MarkdownAppService, TRUNCATE_THRESHOLD_BYTES};

impl MarkdownAppService {
    /// 渲染预览：返回块的 DTO 视图。
    ///
    /// 流程：取内容 + 版本 → 超大源直接返回 `truncated` → 缓存命中返回 → 解析并缓存。
    /// `version` 与注册表版本不一致时（内容已变）强制重解析。
    pub fn render_preview(&self, cmd: RenderPreviewQuery) -> AppResult<RenderedView> {
        let (content, revision) = self.fetch(cmd.doc_id)?;
        if content.len() > TRUNCATE_THRESHOLD_BYTES {
            // 大文件降级：不解析、不缓存，直接返回截断标记。
            return Ok(RenderedView {
                blocks: Vec::new(),
                version: revision,
                truncated: true,
            });
        }
        let blocks = self.parse_or_cache(cmd.doc_id, &content, revision)?;
        let blocks = blocks.iter().map(to_block_view).collect();
        Ok(RenderedView {
            blocks,
            version: revision,
            truncated: false,
        })
    }

    // ---------- 内部 ----------

    pub(super) fn fetch(&self, doc_id: DocumentId) -> AppResult<(String, u64)> {
        self.provider
            .content_and_revision(doc_id)
            .ok_or_else(|| AppError::domain("文档未打开"))
    }

    /// 取缓存命中结果；未命中时解析并写入缓存（返回克隆，便于并发读取）。
    pub(super) fn parse_or_cache(
        &self,
        doc_id: DocumentId,
        content: &str,
        revision: u64,
    ) -> AppResult<Vec<RenderedBlock>> {
        {
            let cache = self
                .cache
                .lock()
                .map_err(|_| AppError::io("渲染缓存锁已损坏"))?;
            if let Some(blocks) = cache.get(doc_id, revision) {
                return Ok(blocks.clone());
            }
        }
        let blocks = self.parser.parse(content).map_err(AppError::from)?;
        self.cache
            .lock()
            .map_err(|_| AppError::io("渲染缓存锁已损坏"))?
            .insert(doc_id, revision, blocks.clone());
        Ok(blocks)
    }
}
