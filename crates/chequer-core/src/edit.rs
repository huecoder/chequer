use crate::geom::{IPoint, IRect};
use crate::history::Change;
use crate::raster::{Raster, Rgba, mul_div_255};

pub const TILE_SIZE: u32 = 64;

const NO_COVERAGE: u8 = 0;
const FULL_COVERAGE: u32 = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverageMerge {
    Max,
    Accumulate,
}

impl CoverageMerge {
    fn merge(self, stored: u8, incoming: u8) -> u8 {
        match self {
            Self::Max => stored.max(incoming),
            Self::Accumulate => (stored as u32 + incoming as u32 - mul_div_255(stored as u32, incoming as u32)).min(FULL_COVERAGE) as u8,
        }
    }
}

struct Tile {
    origin: IPoint,
    before: Raster,
    coverage: Vec<u8>,
}

pub struct PixelEdit {
    layer: usize,
    frame: usize,
    bounds: IRect,
    columns: u32,
    tiles: Vec<Option<Tile>>,
    dirty: IRect,
}

impl PixelEdit {
    pub fn new(layer: usize, frame: usize, raster: &Raster) -> Self {
        let columns = raster.width().div_ceil(TILE_SIZE);
        let rows = raster.height().div_ceil(TILE_SIZE);
        Self {
            layer,
            frame,
            bounds: raster.bounds(),
            columns,
            tiles: (0..columns * rows).map(|_| None).collect(),
            dirty: IRect::EMPTY,
        }
    }

    pub fn layer(&self) -> usize {
        self.layer
    }

    pub fn dirty(&self) -> IRect {
        self.dirty
    }

    pub fn mark_dirty(&mut self, rect: IRect) {
        self.dirty = self.dirty.union(rect.intersect(self.bounds));
    }

    fn tile_slot(&self, x: i32, y: i32) -> usize {
        (y as u32 / TILE_SIZE * self.columns + x as u32 / TILE_SIZE) as usize
    }

    fn ensure_tile(&mut self, raster: &Raster, x: i32, y: i32) -> &mut Tile {
        let slot = self.tile_slot(x, y);
        self.tiles[slot].get_or_insert_with(|| {
            let origin = IPoint::new(x - x % TILE_SIZE as i32, y - y % TILE_SIZE as i32);
            let area = IRect::from_size(origin.x, origin.y, TILE_SIZE, TILE_SIZE).intersect(raster.bounds());
            Tile { origin, before: raster.crop(area), coverage: Vec::new() }
        })
    }

    pub fn touch(&mut self, raster: &Raster, rect: IRect) {
        let area = rect.intersect(self.bounds);
        if area.is_empty() {
            return;
        }
        let tile = TILE_SIZE as i32;
        for ty in (area.y0 / tile)..=((area.y1 - 1) / tile) {
            for tx in (area.x0 / tile)..=((area.x1 - 1) / tile) {
                self.ensure_tile(raster, tx * tile, ty * tile);
            }
        }
    }

    pub fn paint_pixel(&mut self, raster: &mut Raster, x: i32, y: i32, coverage: u8, compose: impl Fn(Rgba, u8) -> Rgba) {
        self.paint_pixel_merged(raster, x, y, coverage, CoverageMerge::Max, compose);
    }

    pub fn paint_pixel_merged(
        &mut self,
        raster: &mut Raster,
        x: i32,
        y: i32,
        coverage: u8,
        merge: CoverageMerge,
        compose: impl Fn(Rgba, u8) -> Rgba,
    ) {
        if !self.bounds.contains(IPoint::new(x, y)) || coverage == NO_COVERAGE {
            return;
        }
        let tile = self.ensure_tile(raster, x, y);
        if tile.coverage.is_empty() {
            tile.coverage = vec![NO_COVERAGE; tile.before.width() as usize * tile.before.height() as usize];
        }
        let local_x = (x - tile.origin.x) as u32;
        let local_y = (y - tile.origin.y) as u32;
        let index = local_y as usize * tile.before.width() as usize + local_x as usize;
        let merged = merge.merge(tile.coverage[index], coverage);
        if merged == tile.coverage[index] {
            return;
        }
        tile.coverage[index] = merged;
        let original = tile.before.get(local_x, local_y);
        raster.set(x as u32, y as u32, compose(original, merged));
    }

    pub fn original_pixel(&self, raster: &Raster, x: i32, y: i32) -> Rgba {
        let (tile_x, tile_y) = (x - x % TILE_SIZE as i32, y - y % TILE_SIZE as i32);
        match &self.tiles[self.tile_slot(x, y)] {
            Some(tile) => tile.before.get((x - tile_x) as u32, (y - tile_y) as u32),
            None => raster.get(x as u32, y as u32),
        }
    }

    pub fn restore_pixel(&mut self, raster: &mut Raster, x: i32, y: i32) {
        if !self.bounds.contains(IPoint::new(x, y)) {
            return;
        }
        let tile = self.ensure_tile(raster, x, y);
        if tile.coverage.is_empty() {
            return;
        }
        let local_x = (x - tile.origin.x) as u32;
        let local_y = (y - tile.origin.y) as u32;
        tile.coverage[local_y as usize * tile.before.width() as usize + local_x as usize] = NO_COVERAGE;
        raster.set(x as u32, y as u32, tile.before.get(local_x, local_y));
    }

    pub fn finish(self, raster: &Raster) -> Option<Change> {
        let area = self.dirty.intersect(self.bounds);
        if area.is_empty() {
            return None;
        }
        let after = raster.crop(area);
        let mut before = after.clone();
        for tile in self.tiles.iter().flatten() {
            before.blit(&tile.before, IPoint::new(tile.origin.x - area.x0, tile.origin.y - area.y0));
        }
        if before == after {
            return None;
        }
        Some(Change::Pixels { layer: self.layer, frame: self.frame, origin: IPoint::new(area.x0, area.y0), before, after })
    }

    pub fn revert(self, raster: &mut Raster) {
        for tile in self.tiles.iter().flatten() {
            raster.blit(&tile.before, tile.origin);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_reports_before_and_after_of_dirty_area() {
        let mut raster = Raster::filled(100, 100, [1, 1, 1, 255]);
        let mut edit = PixelEdit::new(0, 0, &raster);
        edit.paint_pixel(&mut raster, 70, 5, 255, |_, _| [9, 9, 9, 255]);
        edit.mark_dirty(IRect::new(70, 5, 71, 6));
        let Some(Change::Pixels { origin, before, after, .. }) = edit.finish(&raster) else {
            panic!("ожидалась запись Pixels");
        };
        assert_eq!(origin, IPoint::new(70, 5));
        assert_eq!(before.get(0, 0), [1, 1, 1, 255]);
        assert_eq!(after.get(0, 0), [9, 9, 9, 255]);
    }

    #[test]
    fn finish_without_change_is_none() {
        let mut raster = Raster::filled(10, 10, [1, 1, 1, 255]);
        let mut edit = PixelEdit::new(0, 0, &raster);
        edit.touch(&raster, IRect::new(0, 0, 5, 5));
        edit.mark_dirty(IRect::new(0, 0, 5, 5));
        raster.set(0, 0, [1, 1, 1, 255]);
        assert!(edit.finish(&raster).is_none());
    }

    #[test]
    fn stroke_does_not_accumulate_alpha_on_repaint() {
        let mut raster = Raster::filled(4, 4, [0, 0, 0, 255]);
        let mut edit = PixelEdit::new(0, 0, &raster);
        let half_white = |before: Rgba, cov: u8| crate::raster::over(before, [255, 255, 255, cov]);
        edit.paint_pixel(&mut raster, 1, 1, 128, half_white);
        let once = raster.get(1, 1);
        edit.paint_pixel(&mut raster, 1, 1, 128, half_white);
        assert_eq!(raster.get(1, 1), once);
    }

    #[test]
    fn restored_pixel_gets_its_original_color_and_can_be_painted_again() {
        let mut raster = Raster::filled(10, 10, [1, 1, 1, 255]);
        let mut edit = PixelEdit::new(0, 0, &raster);
        edit.paint_pixel(&mut raster, 3, 3, 255, |_, _| [7, 7, 7, 255]);
        edit.restore_pixel(&mut raster, 3, 3);
        assert_eq!(raster.get(3, 3), [1, 1, 1, 255]);
        edit.paint_pixel(&mut raster, 3, 3, 200, |_, _| [8, 8, 8, 255]);
        assert_eq!(raster.get(3, 3), [8, 8, 8, 255]);
    }

    #[test]
    fn revert_restores_touched_tiles() {
        let mut raster = Raster::filled(10, 10, [1, 1, 1, 255]);
        let mut edit = PixelEdit::new(0, 0, &raster);
        edit.paint_pixel(&mut raster, 3, 3, 255, |_, _| [7, 7, 7, 255]);
        edit.revert(&mut raster);
        assert_eq!(raster.get(3, 3), [1, 1, 1, 255]);
    }
}
