//! 拼写用例：整篇错词区间检查（带缓存、失败降级为空集）、偏移处候选建议。

use crate::shared::text::word_bounds_at;

use super::super::queries::*;
use super::{MarkdownAppService, TRUNCATE_THRESHOLD_BYTES, lock_or_recover};

impl MarkdownAppService {
    /// 拼写检查：返回错词**字节区间**列表（`(start, end)`，按起点升序）。
    ///
    /// 与 [`Self::highlight`] 的三处刻意差异：
    ///
    /// 1. **不报错**：词典缺失 / 检查失败一律降级为空集。拼写是辅助提示，
    ///    让它把编辑器卡住或弹错是本末倒置。
    /// 2. **不防抖**：防抖是为 syntect 那种「停手后才算」的高亮设计的；
    ///    拼写检查本身已是 O(词长) 的哈希查找（实测 900KB / 94ms 在 debug，
    ///    release 下远低于此），且**开着就应立刻反映最新输入**，延迟显示反而像 bug。
    ///    版本号缓存已保证未变的帧不重复付费。
    /// 3. **大文件降级**：超过 [`TRUNCATE_THRESHOLD_BYTES`] 直接返回空集——
    ///    与高亮 / 预览同策略，保证大文档下编辑帧不因可选功能退化。
    ///
    /// [`TRUNCATE_THRESHOLD_BYTES`]: super::TRUNCATE_THRESHOLD_BYTES
    pub fn spell_check(&self, q: SpellCheckQuery) -> Vec<(usize, usize)> {
        let Some(checker) = self.spell.as_ref() else {
            return Vec::new();
        };
        if !q.enabled || !checker.is_available() {
            return Vec::new();
        }
        let Ok((content, revision)) = self.fetch(q.doc_id) else {
            return Vec::new();
        };
        if content.len() > TRUNCATE_THRESHOLD_BYTES {
            return Vec::new();
        }
        if let Some(hits) = lock_or_recover(&self.spell_cache).get(q.doc_id, revision) {
            return hits;
        }
        let Ok(marks) = checker.check(&content) else {
            // 检查失败按「无错词」处理：宁可漏标，不可弹错。
            return Vec::new();
        };
        let hits: Vec<(usize, usize)> = marks
            .iter()
            .map(|m| (m.range.start().value(), m.range.end().value()))
            .collect();
        lock_or_recover(&self.spell_cache).insert(q.doc_id, revision, hits.clone());
        hits
    }

    /// 拼写建议（V11-07）：给 `offset` 处的那个词返回候选替换。
    ///
    /// # 为什么只按 `offset` 定位，不接受「词」本身
    ///
    /// 表现层手里只有**字节偏移**（egui 的排版结果给不出词），
    /// 若把「扩到整词」的责任推给调用方，就会有两份分词规则
    /// （一份在视图、一份在检查器），改了一处必然漏另一处。
    /// 这里复用与检查器同源的「ASCII 字母即词」约定，保持单一真相。
    ///
    /// # 边界
    ///
    /// - `offset` 落在非字母（空白 / 中文 / 标点）上 → 空集（没有词可建议）；
    /// - 该词**未被判错**（在词表或找不到近邻）→ 空集；
    /// - 检查器不可用 / 未装配 → 空集。
    ///
    /// 不开拼写检查时也照常返回建议：用户既然右键了，说明他看到了波浪线
    /// 或觉得这个词可疑，此时拒绝回答没有意义。
    pub fn spell_suggestions(&self, q: SpellSuggestQuery) -> Vec<String> {
        let Some(checker) = self.spell.as_ref() else {
            return Vec::new();
        };
        if !checker.is_available() || q.limit == 0 {
            return Vec::new();
        }
        let Ok((content, _revision)) = self.fetch(q.doc_id) else {
            return Vec::new();
        };
        let Some((start, end)) = word_bounds_at(&content, q.offset) else {
            return Vec::new();
        };
        let word = content[start..end].to_ascii_lowercase();
        checker.suggest(&word, q.limit)
    }
}
