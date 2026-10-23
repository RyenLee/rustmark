//! 导航用例：编辑区 ↔ 预览双向滚动映射、大纲重建。

use crate::domain::markdown::services::BlockMapper;
use crate::domain::markdown::value_objects::BlockId;
use crate::shared::geometry::ByteOffset;
use crate::shared::result::AppResult;

use crate::application::dto::{OutlineItemView, OutlineView};

use super::super::queries::*;
use super::{MarkdownAppService, TRUNCATE_THRESHOLD_BYTES};

impl MarkdownAppService {
    /// 滚动同步：编辑区偏移 ↔ 预览块，双向映射。
    pub fn map_scroll(&self, cmd: MapScrollQuery) -> AppResult<ScrollTarget> {
        let (content, revision) = self.fetch(cmd.doc_id)?;
        // 超大文档不解析，直接给出默认目标（UI 此时已降级为纯文本预览）。
        if content.len() > TRUNCATE_THRESHOLD_BYTES {
            return Ok(match cmd.source {
                ScrollSource::Editor { .. } => ScrollTarget::Preview {
                    block_id: 0,
                    ratio: 0.0,
                },
                ScrollSource::Preview { .. } => ScrollTarget::Editor { offset: 0 },
            });
        }
        let blocks = self.parse_or_cache(cmd.doc_id, &content, revision)?;
        match cmd.source {
            ScrollSource::Editor { offset } => {
                match BlockMapper::offset_to_block(&blocks, ByteOffset::new(offset)) {
                    Some((id, ratio)) => Ok(ScrollTarget::Preview {
                        block_id: id.as_u64(),
                        ratio,
                    }),
                    None => Ok(ScrollTarget::Preview {
                        block_id: 0,
                        ratio: 0.0,
                    }),
                }
            }
            ScrollSource::Preview { block_id } => {
                let id = BlockId::from_u64(block_id);
                match blocks.iter().find(|block| block.id() == id) {
                    Some(block) => Ok(ScrollTarget::Editor {
                        offset: block.source_range().start().value(),
                    }),
                    None => Ok(ScrollTarget::Editor { offset: 0 }),
                }
            }
        }
    }

    /// 重排大纲：由当前内容重建标题大纲。
    pub fn rebuild_outline(&self, cmd: RebuildOutlineQuery) -> AppResult<OutlineView> {
        let (content, revision) = self.fetch(cmd.doc_id)?;
        let blocks = self.parse_or_cache(cmd.doc_id, &content, revision)?;
        let items = BlockMapper::build_outline(&blocks, &content);
        Ok(OutlineView(
            items
                .iter()
                .map(|item| OutlineItemView {
                    level: item.level(),
                    text: item.text().to_string(),
                    line: item.line(),
                })
                .collect(),
        ))
    }
}
