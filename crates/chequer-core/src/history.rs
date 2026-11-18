use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::document::{CanvasState, Document, Layer, LayerProps};
use crate::effects::EffectId;
use crate::frames::Frame;
use crate::geom::{IPoint, IRect};
use crate::raster::Raster;
use crate::selection::SharedSelection;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoryKind {
    NewImage,
    Pencil,
    Brush,
    Eraser,
    CloneStamp,
    Fill,
    Gradient,
    Line,
    Rectangle,
    RoundedRectangle,
    Ellipse,
    FreeformShape,
    Shape,
    TextInsert,
    SelectRectangle,
    SelectEllipse,
    SelectLasso,
    SelectWand,
    SelectAll,
    Deselect,
    InvertSelection,
    PasteSelection,
    MoveSelection,
    ModifySelection,
    MoveSelectedPixels,
    Paste,
    Cut,
    Delete,
    FillSelection,
    AddLayer,
    DuplicateLayer,
    DeleteLayer,
    MergeLayerDown,
    MoveLayerUp,
    MoveLayerDown,
    MaskLayer,
    UnmaskLayer,
    LayerProperties,
    CropToSelection,
    ResizeImage,
    ResizeCanvas,
    FlipHorizontal,
    FlipVertical,
    RotateClockwise,
    RotateCounterClockwise,
    RotateHalfTurn,
    OffsetHalf,
    Flatten,
    AddFrame,
    DuplicateFrame,
    DeleteFrame,
    MoveFrame,
    FrameDuration,
    InvertColors,
    BlackAndWhite,
    Effect(EffectId),
    ClearCels,
    PasteCels,
    ReverseFrames,
}

impl HistoryKind {
    fn is_involution(self) -> bool {
        matches!(self, Self::InvertColors | Self::FlipHorizontal | Self::FlipVertical)
    }

    fn undoes_geometry_of(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::FlipHorizontal, Self::FlipHorizontal)
                | (Self::FlipVertical, Self::FlipVertical)
                | (Self::RotateHalfTurn, Self::RotateHalfTurn)
                | (Self::RotateClockwise, Self::RotateCounterClockwise)
                | (Self::RotateCounterClockwise, Self::RotateClockwise)
        )
    }
}

#[derive(Clone, Debug)]
pub enum Change {
    Pixels { layer: usize, frame: usize, origin: IPoint, before: Raster, after: Raster },
    Selection { before: Option<SharedSelection>, after: Option<SharedSelection> },
    LayerInserted { index: usize, layer: Layer },
    LayerRemoved { index: usize, layer: Layer },
    LayerMoved { from: usize, to: usize },
    LayerProps { index: usize, before: LayerProps, after: LayerProps },
    Canvas { before: CanvasState, after: CanvasState },
    FrameInserted { index: usize, frame: Frame, cels: Vec<Arc<Raster>> },
    FrameRemoved { index: usize, frame: Frame, cels: Vec<Arc<Raster>> },
    FrameMoved { from: usize, to: usize },
    FrameDuration { index: usize, before: Frame, after: Frame },
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: HistoryKind,
    pub(crate) changes: Vec<Change>,
    pub(crate) active_before: usize,
    pub(crate) active_after: usize,
    pub(crate) frame_before: usize,
    pub(crate) frame_after: usize,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    pub(crate) entries: Vec<Arc<Entry>>,
    pub(crate) cursor: usize,
}

impl History {
    pub fn kinds(&self) -> impl Iterator<Item = HistoryKind> + '_ {
        self.entries.iter().map(|e| e.kind)
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor < self.entries.len()
    }

    pub(crate) fn coalesce(&mut self, kind: HistoryKind, changes: &[Change]) -> bool {
        if self.can_redo() {
            return false;
        }
        let Some(last) = self.entries.last_mut().map(Arc::make_mut) else {
            return false;
        };
        let outcome = match (last.changes.as_mut_slice(), changes) {
            ([Change::Pixels { layer, frame, origin, before, after }], [Change::Pixels { layer: next_layer, frame: next_frame, origin: next_origin, after: next_after, .. }])
                if last.kind == kind
                    && kind.is_involution()
                    && layer == next_layer
                    && frame == next_frame
                    && origin == next_origin
                    && after.bounds() == next_after.bounds() =>
            {
                *after = next_after.clone();
                Some(before == after)
            }
            ([Change::Selection { before, after }], [Change::Selection { after: next_after, .. }])
                if last.kind == kind && kind == HistoryKind::InvertSelection =>
            {
                *after = next_after.clone();
                Some(before.as_deref() == after.as_deref())
            }
            ([Change::Canvas { .. }], [Change::Canvas { .. }]) => last.kind.undoes_geometry_of(kind).then_some(true),
            ([Change::LayerMoved { from, to }], [Change::LayerMoved { from: next_from, to: next_to }]) => (from == next_to && to == next_from).then_some(true),
            _ => None,
        };
        let Some(cancelled) = outcome else {
            return false;
        };
        if cancelled {
            self.entries.pop();
            self.cursor -= 1;
        }
        true
    }

    pub(crate) fn coalesce_layer_props(&mut self, index: usize, before: &LayerProps, after: &LayerProps) -> bool {
        if self.can_redo() {
            return false;
        }
        let Some(last) = self.entries.last_mut().map(Arc::make_mut) else {
            return false;
        };
        let [Change::LayerProps { index: last_index, before: last_before, after: last_after }] = last.changes.as_mut_slice() else {
            return false;
        };
        if last.kind != HistoryKind::LayerProperties
            || *last_index != index
            || changed_fields(last_before, last_after) != changed_fields(before, after)
        {
            return false;
        }
        *last_after = after.clone();
        if last_before == last_after {
            self.entries.pop();
            self.cursor -= 1;
        }
        true
    }
}

fn changed_fields(before: &LayerProps, after: &LayerProps) -> [bool; 5] {
    [
        before.name != after.name,
        before.visible != after.visible,
        before.opacity != after.opacity,
        before.blend != after.blend,
        before.masked != after.masked,
    ]
}

impl Document {
    pub(crate) fn push_entry(&mut self, kind: HistoryKind, changes: Vec<Change>, active_before: usize) {
        self.push_entry_from(kind, changes, active_before, self.frame);
    }

    pub(crate) fn push_entry_from(&mut self, kind: HistoryKind, changes: Vec<Change>, active_before: usize, frame_before: usize) {
        if changes.is_empty() {
            return;
        }
        if self.history.coalesce(kind, &changes) {
            self.revision += 1;
            return;
        }
        self.history.entries.truncate(self.history.cursor);
        self.history.entries.push(Arc::new(Entry { kind, changes, active_before, active_after: self.active, frame_before, frame_after: self.frame }));
        self.history.cursor += 1;
        self.revision += 1;
    }

    pub fn undo(&mut self) -> IRect {
        if self.floating.is_some() {
            return self.cancel_floating();
        }
        if !self.history.can_undo() {
            return IRect::EMPTY;
        }
        self.history.cursor -= 1;
        self.apply_entry(self.history.cursor, false)
    }

    pub fn redo(&mut self) -> IRect {
        self.settle();
        if !self.history.can_redo() {
            return IRect::EMPTY;
        }
        let area = self.apply_entry(self.history.cursor, true);
        self.history.cursor += 1;
        area
    }

    pub fn jump_to(&mut self, target: usize) -> IRect {
        self.settle();
        let target = target.min(self.history.entries.len());
        let mut area = IRect::EMPTY;
        while self.history.cursor > target {
            area = area.union(self.undo());
        }
        while self.history.cursor < target {
            area = area.union(self.redo());
        }
        area
    }

    fn apply_entry(&mut self, index: usize, forward: bool) -> IRect {
        let entries = std::mem::take(&mut self.history.entries);
        let entry = &entries[index];
        let mut area = IRect::EMPTY;
        let order: Box<dyn Iterator<Item = &Change>> =
            if forward { Box::new(entry.changes.iter()) } else { Box::new(entry.changes.iter().rev()) };
        for change in order {
            area = area.union(self.apply_change(change, forward));
        }
        self.active = if forward { entry.active_after } else { entry.active_before };
        self.active = self.active.min(self.layers.len().saturating_sub(1));
        self.frame = if forward { entry.frame_after } else { entry.frame_before };
        self.sync_frame();
        self.history.entries = entries;
        self.revision += 1;
        area
    }

    fn apply_change(&mut self, change: &Change, forward: bool) -> IRect {
        match change {
            Change::Pixels { layer, frame, origin, before, after } => {
                let source = if forward { after } else { before };
                self.layers[*layer].cel_mut(*frame).blit(source, *origin);
                source.bounds().translate(origin.x, origin.y)
            }
            Change::Selection { before, after } => {
                self.selection = if forward { after.clone() } else { before.clone() };
                IRect::EMPTY
            }
            Change::LayerInserted { index, layer } => self.insert_or_remove(*index, layer, forward),
            Change::LayerRemoved { index, layer } => self.insert_or_remove(*index, layer, !forward),
            Change::LayerMoved { from, to } => {
                let (from, to) = if forward { (*from, *to) } else { (*to, *from) };
                let layer = self.layers.remove(from);
                self.layers.insert(to, layer);
                self.bounds()
            }
            Change::LayerProps { index, before, after } => {
                self.layers[*index].set_props(if forward { after } else { before });
                self.bounds()
            }
            Change::Canvas { before, after } => {
                let state = if forward { after } else { before };
                self.width = state.width;
                self.height = state.height;
                self.layers = state.layers.clone();
                self.bounds()
            }
            Change::FrameInserted { index, frame, cels } => self.insert_or_remove_frame(*index, *frame, cels, forward),
            Change::FrameRemoved { index, frame, cels } => self.insert_or_remove_frame(*index, *frame, cels, !forward),
            Change::FrameMoved { from, to } => {
                let (from, to) = if forward { (*from, *to) } else { (*to, *from) };
                self.apply_frame_move(from, to);
                self.bounds()
            }
            Change::FrameDuration { index, before, after } => {
                self.frames[*index] = if forward { *after } else { *before };
                IRect::EMPTY
            }
        }
    }

    fn insert_or_remove_frame(&mut self, index: usize, frame: Frame, cels: &[Arc<Raster>], insert: bool) -> IRect {
        if insert {
            self.apply_frame_insert(index, frame, cels);
        } else {
            self.apply_frame_remove(index);
        }
        self.bounds()
    }

    fn insert_or_remove(&mut self, index: usize, layer: &Layer, insert: bool) -> IRect {
        if insert {
            self.layers.insert(index, layer.clone());
        } else {
            self.layers.remove(index);
        }
        self.bounds()
    }
}
