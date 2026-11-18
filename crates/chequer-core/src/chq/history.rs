use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::document::{CanvasState, Layer, LayerProps, check_size};
use crate::frames::Frame;
use crate::geom::{IPoint, IRect};
use crate::history::{Change, Entry, History, HistoryKind};
use crate::raster::Raster;
use crate::selection::{Selection, SharedSelection};

use super::layout::Parsed;
use super::tile::{BYTES_PER_PIXEL, WriteMode};
use super::{ChqError, DEFAULT_TILE_LOG2, LayerEntry, TileGrid, TileStore, decode_layer, layer_cells};

const COVERAGE_CHANNEL: usize = 3;
const COVERAGE_PIXEL_COLOR: [u8; COVERAGE_CHANNEL] = [0; COVERAGE_CHANNEL];

#[derive(Clone, Serialize, Deserialize)]
struct RasterRef {
    width: u32,
    height: u32,
    cells: Vec<u32>,
}

#[derive(Serialize, Deserialize)]
struct LayerRef {
    #[serde(flatten)]
    entry: LayerEntry,
    raster: RasterRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    later_cels: Vec<RasterRef>,
}

#[derive(Serialize, Deserialize)]
struct SelectionRef {
    bounds: [i32; 4],
    coverage: RasterRef,
}

#[derive(Serialize, Deserialize)]
struct CanvasRef {
    width: u32,
    height: u32,
    layers: Vec<LayerRef>,
}

#[derive(Serialize, Deserialize)]
enum ChangeRef {
    Pixels {
        layer: usize,
        #[serde(default)]
        frame: usize,
        origin: [i32; 2],
        before: RasterRef,
        after: RasterRef,
    },
    Selection { before: Option<SelectionRef>, after: Option<SelectionRef> },
    LayerInserted { index: usize, layer: LayerRef },
    LayerRemoved { index: usize, layer: LayerRef },
    LayerMoved { from: usize, to: usize },
    LayerProps { index: usize, before: LayerProps, after: LayerProps },
    Canvas { before: CanvasRef, after: CanvasRef },
    FrameInserted { index: usize, duration_ms: u32, cels: Vec<RasterRef> },
    FrameRemoved { index: usize, duration_ms: u32, cels: Vec<RasterRef> },
    FrameMoved { from: usize, to: usize },
    FrameDuration { index: usize, before_ms: u32, after_ms: u32 },
}

#[derive(Serialize, Deserialize)]
struct EntryRef {
    kind: HistoryKind,
    active_before: usize,
    active_after: usize,
    #[serde(default)]
    frame_before: usize,
    #[serde(default)]
    frame_after: usize,
    changes: Vec<ChangeRef>,
}

#[derive(Serialize, Deserialize)]
struct HistoryRef {
    cursor: usize,
    entries: Vec<EntryRef>,
}

fn coverage_raster(selection: &Selection) -> Result<Raster, ChqError> {
    let bounds = selection.bounds();
    let mut rgba = Vec::with_capacity(bounds.width() as usize * bounds.height() as usize * BYTES_PER_PIXEL);
    for y in bounds.y0..bounds.y1 {
        for &coverage in selection.coverage_row(y, bounds.x0, bounds.x1) {
            rgba.extend_from_slice(&COVERAGE_PIXEL_COLOR);
            rgba.push(coverage);
        }
    }
    Ok(Raster::from_rgba(bounds.width() as u32, bounds.height() as u32, rgba)?)
}

fn coverage_values(raster: &Raster) -> Vec<u8> {
    raster.as_bytes().chunks_exact(BYTES_PER_PIXEL).map(|pixel| pixel[COVERAGE_CHANNEL]).collect()
}

struct Writer<'a> {
    mode: WriteMode,
    store: &'a mut TileStore,
    written_cels: HashMap<usize, RasterRef>,
}

impl Writer<'_> {
    fn raster(&mut self, raster: &Raster) -> Result<RasterRef, ChqError> {
        let grid = TileGrid::new(raster.width(), raster.height(), 1 << DEFAULT_TILE_LOG2);
        Ok(RasterRef { width: raster.width(), height: raster.height(), cells: layer_cells(raster, None, grid, self.mode, self.store)? })
    }

    fn cel(&mut self, cel: &Arc<Raster>) -> Result<RasterRef, ChqError> {
        let pointer = Arc::as_ptr(cel) as usize;
        if let Some(written) = self.written_cels.get(&pointer) {
            return Ok(written.clone());
        }
        let reference = self.raster(cel)?;
        self.written_cels.insert(pointer, reference.clone());
        Ok(reference)
    }

    fn cels(&mut self, cels: &[Arc<Raster>]) -> Result<Vec<RasterRef>, ChqError> {
        cels.iter().map(|cel| self.cel(cel)).collect()
    }

    fn layer(&mut self, layer: &Layer) -> Result<LayerRef, ChqError> {
        let mut references = self.cels(layer.cels())?.into_iter();
        let raster = references.next().ok_or(ChqError::Invalid("у слоя нет ячеек"))?;
        Ok(LayerRef { entry: LayerEntry::of(layer), raster, later_cels: references.collect() })
    }

    fn selection(&mut self, selection: &Option<SharedSelection>) -> Result<Option<SelectionRef>, ChqError> {
        let Some(selection) = selection else {
            return Ok(None);
        };
        let bounds = selection.bounds();
        Ok(Some(SelectionRef { bounds: [bounds.x0, bounds.y0, bounds.x1, bounds.y1], coverage: self.raster(&coverage_raster(selection)?)? }))
    }

    fn canvas(&mut self, state: &CanvasState) -> Result<CanvasRef, ChqError> {
        let layers = state.layers.iter().map(|layer| self.layer(layer)).collect::<Result<_, _>>()?;
        Ok(CanvasRef { width: state.width, height: state.height, layers })
    }

    fn change(&mut self, change: &Change) -> Result<ChangeRef, ChqError> {
        Ok(match change {
            Change::Pixels { layer, frame, origin, before, after } => {
                ChangeRef::Pixels { layer: *layer, frame: *frame, origin: [origin.x, origin.y], before: self.raster(before)?, after: self.raster(after)? }
            }
            Change::Selection { before, after } => ChangeRef::Selection { before: self.selection(before)?, after: self.selection(after)? },
            Change::LayerInserted { index, layer } => ChangeRef::LayerInserted { index: *index, layer: self.layer(layer)? },
            Change::LayerRemoved { index, layer } => ChangeRef::LayerRemoved { index: *index, layer: self.layer(layer)? },
            Change::LayerMoved { from, to } => ChangeRef::LayerMoved { from: *from, to: *to },
            Change::LayerProps { index, before, after } => ChangeRef::LayerProps { index: *index, before: before.clone(), after: after.clone() },
            Change::Canvas { before, after } => ChangeRef::Canvas { before: self.canvas(before)?, after: self.canvas(after)? },
            Change::FrameInserted { index, frame, cels } => ChangeRef::FrameInserted { index: *index, duration_ms: frame.duration_ms, cels: self.cels(cels)? },
            Change::FrameRemoved { index, frame, cels } => ChangeRef::FrameRemoved { index: *index, duration_ms: frame.duration_ms, cels: self.cels(cels)? },
            Change::FrameMoved { from, to } => ChangeRef::FrameMoved { from: *from, to: *to },
            Change::FrameDuration { index, before, after } => ChangeRef::FrameDuration { index: *index, before_ms: before.duration_ms, after_ms: after.duration_ms },
        })
    }

    fn entry(&mut self, entry: &Entry) -> Result<EntryRef, ChqError> {
        let changes = entry.changes.iter().map(|change| self.change(change)).collect::<Result<_, _>>()?;
        Ok(EntryRef {
            kind: entry.kind,
            active_before: entry.active_before,
            active_after: entry.active_after,
            frame_before: entry.frame_before,
            frame_after: entry.frame_after,
            changes,
        })
    }
}

pub(super) fn write(history: &History, mode: WriteMode, store: &mut TileStore) -> Result<Vec<u8>, ChqError> {
    let mut writer = Writer { mode, store, written_cels: HashMap::new() };
    let entries = history.entries.iter().map(|entry| writer.entry(entry)).collect::<Result<_, _>>()?;
    Ok(serde_json::to_vec(&HistoryRef { cursor: history.cursor, entries })?)
}

struct Loader<'a, 'file> {
    file: &'a Parsed<'file>,
    tile_side: u32,
    decoded_cels: RefCell<HashMap<(u32, u32, Vec<u32>), Arc<Raster>>>,
}

impl Loader<'_, '_> {
    fn raster(&self, reference: &RasterRef) -> Result<Raster, ChqError> {
        check_size(reference.width, reference.height)?;
        let grid = TileGrid::new(reference.width, reference.height, self.tile_side);
        if reference.cells.len() != grid.cells() {
            return Err(ChqError::Invalid("сетка тайлов истории не по размеру"));
        }
        decode_layer(self.file, grid, &reference.cells)
    }

    fn cel(&self, reference: &RasterRef) -> Result<Arc<Raster>, ChqError> {
        let key = (reference.width, reference.height, reference.cells.clone());
        if let Some(decoded) = self.decoded_cels.borrow().get(&key) {
            return Ok(Arc::clone(decoded));
        }
        let decoded = Arc::new(self.raster(reference)?);
        self.decoded_cels.borrow_mut().insert(key, Arc::clone(&decoded));
        Ok(decoded)
    }

    fn cels(&self, references: &[RasterRef]) -> Result<Vec<Arc<Raster>>, ChqError> {
        references.iter().map(|reference| self.cel(reference)).collect()
    }

    fn layer(&self, reference: &LayerRef) -> Result<Layer, ChqError> {
        let cels = std::iter::once(&reference.raster).chain(&reference.later_cels).map(|cel| self.cel(cel)).collect::<Result<_, _>>()?;
        Ok(reference.entry.layer(cels))
    }

    fn selection(&self, reference: &Option<SelectionRef>) -> Result<Option<SharedSelection>, ChqError> {
        let Some(reference) = reference else {
            return Ok(None);
        };
        let [x0, y0, x1, y1] = reference.bounds;
        let coverage = self.raster(&reference.coverage)?;
        let fits = i64::from(x1) - i64::from(x0) == i64::from(coverage.width()) && i64::from(y1) - i64::from(y0) == i64::from(coverage.height());
        if !fits {
            return Err(ChqError::Invalid("выделение в истории не по размеру"));
        }
        let selection = Selection::from_coverage(IRect::new(x0, y0, x1, y1), coverage_values(&coverage)).ok_or(ChqError::Invalid("пустое выделение в истории"))?;
        Ok(Some(Arc::new(selection)))
    }

    fn canvas(&self, reference: &CanvasRef) -> Result<CanvasState, ChqError> {
        check_size(reference.width, reference.height)?;
        let layers = reference.layers.iter().map(|layer| self.layer(layer)).collect::<Result<_, _>>()?;
        Ok(CanvasState { width: reference.width, height: reference.height, layers })
    }

    fn change(&self, reference: &ChangeRef) -> Result<Change, ChqError> {
        Ok(match reference {
            ChangeRef::Pixels { layer, frame, origin, before, after } => {
                Change::Pixels { layer: *layer, frame: *frame, origin: IPoint::new(origin[0], origin[1]), before: self.raster(before)?, after: self.raster(after)? }
            }
            ChangeRef::Selection { before, after } => Change::Selection { before: self.selection(before)?, after: self.selection(after)? },
            ChangeRef::LayerInserted { index, layer } => Change::LayerInserted { index: *index, layer: self.layer(layer)? },
            ChangeRef::LayerRemoved { index, layer } => Change::LayerRemoved { index: *index, layer: self.layer(layer)? },
            ChangeRef::LayerMoved { from, to } => Change::LayerMoved { from: *from, to: *to },
            ChangeRef::LayerProps { index, before, after } => Change::LayerProps { index: *index, before: before.clone(), after: after.clone() },
            ChangeRef::Canvas { before, after } => Change::Canvas { before: self.canvas(before)?, after: self.canvas(after)? },
            ChangeRef::FrameInserted { index, duration_ms, cels } => Change::FrameInserted { index: *index, frame: Frame::clamped(*duration_ms), cels: self.cels(cels)? },
            ChangeRef::FrameRemoved { index, duration_ms, cels } => Change::FrameRemoved { index: *index, frame: Frame::clamped(*duration_ms), cels: self.cels(cels)? },
            ChangeRef::FrameMoved { from, to } => Change::FrameMoved { from: *from, to: *to },
            ChangeRef::FrameDuration { index, before_ms, after_ms } => {
                Change::FrameDuration { index: *index, before: Frame::clamped(*before_ms), after: Frame::clamped(*after_ms) }
            }
        })
    }

    fn entry(&self, reference: &EntryRef) -> Result<Entry, ChqError> {
        let changes = reference.changes.iter().map(|change| self.change(change)).collect::<Result<_, _>>()?;
        Ok(Entry {
            kind: reference.kind,
            changes,
            active_before: reference.active_before,
            active_after: reference.active_after,
            frame_before: reference.frame_before,
            frame_after: reference.frame_after,
        })
    }
}

pub(super) fn read(file: &Parsed, payload: &[u8]) -> Result<History, ChqError> {
    let reference: HistoryRef = serde_json::from_slice(payload)?;
    if reference.cursor > reference.entries.len() {
        return Err(ChqError::Invalid("позиция в истории за её концом"));
    }
    let loader = Loader { file, tile_side: 1 << file.tile_log2, decoded_cels: RefCell::new(HashMap::new()) };
    let entries = reference.entries.iter().map(|entry| loader.entry(entry).map(Arc::new)).collect::<Result<_, _>>()?;
    Ok(History { entries, cursor: reference.cursor })
}

#[cfg(test)]
mod tests {
    use crate::document::{Anchor, Document};
    use crate::effects::ids::GAUSSIAN_BLUR;
    use crate::geom::{FPoint, FRect};
    use crate::selection::CombineMode;

    use super::super::{decode_document, encode_document};
    use super::*;

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const WIDTH: u32 = 150;
    const HEIGHT: u32 = 90;

    fn edited_document() -> Document {
        let mut document = Document::new(WIDTH, HEIGHT, WHITE, "фон".into()).unwrap();
        let ellipse = Selection::from_ellipse(FRect::from_corners(FPoint::new(10.0, 10.0), FPoint::new(100.0, 70.0)), true).unwrap();
        document.select(&ellipse, CombineMode::Replace, HistoryKind::SelectEllipse);
        document.fill_selection([255, 0, 0, 255]);
        document.adjust_colors(HistoryKind::Effect(GAUSSIAN_BLUR), |pixel| [pixel[2], pixel[0], pixel[1], pixel[3]]);
        document.add_layer("верх".into());
        document.resize_canvas(120, 80, Anchor::TopLeft).unwrap();
        document
    }

    fn kinds(document: &Document) -> Vec<HistoryKind> {
        document.history().kinds().collect()
    }

    #[test]
    fn saved_history_restores_steps_and_undo_reaches_the_start() {
        let mut original = edited_document();
        let mut restored = decode_document(&encode_document(&original, WriteMode::Small, true).unwrap()).unwrap();
        assert_eq!(kinds(&restored), kinds(&original));
        assert_eq!(restored.history().cursor(), original.history().cursor());
        let finish = restored.history().cursor();
        assert_eq!(restored.flattened(restored.bounds()), original.flattened(original.bounds()));
        restored.jump_to(0);
        original.jump_to(0);
        assert_eq!((restored.width(), restored.height()), (WIDTH, HEIGHT));
        assert_eq!(restored.flattened(restored.bounds()), original.flattened(original.bounds()));
        assert_eq!(restored.flattened(restored.bounds()), Raster::filled(WIDTH, HEIGHT, WHITE));
        restored.jump_to(finish);
        original.jump_to(finish);
        assert_eq!(restored.flattened(restored.bounds()), original.flattened(original.bounds()));
    }

    #[test]
    fn selection_coverage_survives_the_round_trip() {
        let mut original = edited_document();
        let mut restored = decode_document(&encode_document(&original, WriteMode::Fast, true).unwrap()).unwrap();
        let selection_step = 1;
        original.jump_to(selection_step);
        restored.jump_to(selection_step);
        let (kept, expected) = (restored.selection().unwrap(), original.selection().unwrap());
        assert_eq!(kept.bounds(), expected.bounds());
        let bounds = expected.bounds();
        for y in bounds.y0..bounds.y1 {
            assert_eq!(kept.coverage_row(y, bounds.x0, bounds.x1), expected.coverage_row(y, bounds.x0, bounds.x1));
        }
    }

    #[test]
    fn history_is_left_out_unless_requested() {
        let restored = decode_document(&encode_document(&edited_document(), WriteMode::Fast, false).unwrap()).unwrap();
        assert!(restored.history().is_empty());
    }

    #[test]
    fn malformed_history_payload_is_rejected() {
        let bytes = encode_document(&edited_document(), WriteMode::Fast, true).unwrap();
        let file = super::super::layout::parse(&bytes).unwrap();
        assert!(read(&file, b"{}").is_err());
        assert!(read(&file, br#"{"cursor":5,"entries":[]}"#).is_err());
    }
}
