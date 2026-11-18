
use std::sync::Arc;

use super::EffectId;
use crate::document::Document;
use crate::edit::PixelEdit;
use crate::geom::IRect;
use crate::history::HistoryKind;
use crate::raster::{BYTES_PER_PIXEL, Raster, lerp_premultiplied};

pub struct EffectSession {
    layer: usize,
    area: IRect,
    source: Arc<Raster>,
    edit: PixelEdit,
}

impl EffectSession {
    pub fn area(&self) -> IRect {
        self.area
    }

    pub fn source(&self) -> &Arc<Raster> {
        &self.source
    }
}

impl Document {
    pub fn begin_effect(&mut self) -> Option<EffectSession> {
        self.settle();
        let area = self.selection_or_canvas().intersect(self.bounds());
        if area.is_empty() {
            return None;
        }
        let layer = self.active;
        let raster = self.layers[layer].raster();
        let mut edit = PixelEdit::new(layer, self.frame, raster);
        edit.touch(raster, area);
        Some(EffectSession { layer, area, source: Arc::new(raster.clone()), edit })
    }

    pub fn show_effect(&mut self, session: &mut EffectSession, rendered: &Raster) -> IRect {
        let area = session.area;
        let selection = self.selection.as_deref();
        let target = self.layers[session.layer].raster_mut();
        let (x0, x1, width) = (area.x0 as u32, area.x1 as u32, area.width() as u32);
        for y in area.y0..area.y1 {
            let new_row = rendered.row_bytes((y - area.y0) as u32, 0, width);
            let row = target.row_bytes_mut(y as u32, x0, x1);
            let Some(selection) = selection else {
                row.copy_from_slice(new_row);
                continue;
            };
            let coverage = selection.coverage_row(y, area.x0, area.x1);
            let old_row = session.source.row_bytes(y as u32, x0, x1);
            let pixels = row.chunks_exact_mut(BYTES_PER_PIXEL).zip(new_row.chunks_exact(BYTES_PER_PIXEL));
            for ((out, new), (old, &cover)) in pixels.zip(old_row.chunks_exact(BYTES_PER_PIXEL).zip(coverage)) {
                let mixed = lerp_premultiplied([old[0], old[1], old[2], old[3]], [new[0], new[1], new[2], new[3]], cover);
                out.copy_from_slice(&mixed);
            }
        }
        session.edit.mark_dirty(area);
        area
    }

    pub fn finish_effect(&mut self, session: EffectSession, id: EffectId) -> IRect {
        let (area, layer) = (session.area, session.layer);
        if let Some(change) = session.edit.finish(self.layers[layer].raster()) {
            self.push_entry(HistoryKind::Effect(id), vec![change], layer);
        }
        area
    }

    pub fn cancel_effect(&mut self, session: EffectSession) -> IRect {
        let area = session.area;
        session.edit.revert(self.layers[session.layer].raster_mut());
        area
    }
}
