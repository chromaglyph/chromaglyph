//! harfrust 前端：oracle 与 ligature 重物化共用的最小 shaping 入口。
//!
//! rustybuzz 已停止维护（RUSTSEC-2026-0206），shaping 层迁到官方继任者
//! harfrust——跟踪 HarfBuzz 13，与本项目同用 read-fonts 解析栈。

use harfrust::{Buffer, GlyphInfo, ShaperFont};

/// 用默认 features 对 `text` shaping，返回 glyph 流（cluster 对齐原文）。
///
/// 旧 `rustybuzz::shape` 会隐式猜测 segment properties 且从不失败；
/// harfrust 要求 direction 已设置并可能返回 Err。猜测在此补上，Err 折叠
/// 为 `None`，由调用方按 oracle 失败 / ligature 降级处理。
pub(crate) fn shape_text(shaper_font: &ShaperFont<'_, '_>, text: &str) -> Option<Vec<GlyphInfo>> {
    let mut buffer = Buffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    harfrust::shape(shaper_font, &mut buffer, harfrust::ShapeOptions::new()).ok()?;
    Some(buffer.glyph_infos().to_vec())
}
