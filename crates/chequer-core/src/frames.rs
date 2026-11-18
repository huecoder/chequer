use std::collections::HashMap;
use std::sync::Arc;

use crate::document::{Document, Layer};
use crate::geom::IPoint;
use crate::history::{Change, HistoryKind};
use crate::raster::Raster;

pub const MAX_FRAMES: usize = 1024;
pub const DEFAULT_FRAME_DURATION_MS: u32 = 100;
pub const MIN_FRAME_DURATION_MS: u32 = 10;
pub const MAX_FRAME_DURATION_MS: u32 = 60_000;
const FIRST_FRAME: usize = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub duration_ms: u32,
}

impl Frame {
    pub const DEFAULT: Frame = Frame { duration_ms: DEFAULT_FRAME_DURATION_MS };

    pub fn clamped(duration_ms: u32) -> Self {
        Self { duration_ms: duration_ms.clamp(MIN_FRAME_DURATION_MS, MAX_FRAME_DURATION_MS) }
    }
}

#[derive(Clone, Debug)]
pub struct CelClip {
    frames: usize,
    layers: usize,
    cels: Vec<Arc<Raster>>,
}

impl CelClip {
    pub fn frame_count(&self) -> usize {
        self.frames
    }

    pub fn layer_count(&self) -> usize {
        self.layers
    }
}

fn sorted_unique(indices: &[usize], limit: usize) -> Vec<usize> {
    let mut valid: Vec<usize> = indices.iter().copied().filter(|&index| index < limit).collect();
    valid.sort_unstable();
    valid.dedup();
    valid
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CelMark {
    Empty,
    Inherited,
    Drawn,
}

impl Layer {
    pub fn frame_count(&self) -> usize {
        self.cels.len()
    }

    pub fn cel(&self, frame: usize) -> &Arc<Raster> {
        &self.cels[frame]
    }

    pub fn cels(&self) -> &[Arc<Raster>] {
        &self.cels
    }

    pub(crate) fn cel_mut(&mut self, frame: usize) -> &mut Raster {
        Arc::make_mut(&mut self.cels[frame])
    }

    pub(crate) fn show_frame(&mut self, frame: usize) {
        self.frame = frame.min(self.cels.len().saturating_sub(1));
    }

    pub(crate) fn insert_cel(&mut self, frame: usize, cel: Arc<Raster>) {
        self.cels.insert(frame, cel);
    }

    pub(crate) fn remove_cel(&mut self, frame: usize) -> Arc<Raster> {
        self.cels.remove(frame)
    }

    pub(crate) fn move_cel(&mut self, from: usize, to: usize) {
        let cel = self.cels.remove(from);
        self.cels.insert(to, cel);
    }

    pub fn map_cels(&self, f: impl Fn(&Raster) -> Raster) -> Layer {
        let mut mapped: HashMap<*const Raster, Arc<Raster>> = HashMap::new();
        let cels = self
            .cels
            .iter()
            .map(|cel| Arc::clone(mapped.entry(Arc::as_ptr(cel)).or_insert_with(|| Arc::new(f(cel)))))
            .collect();
        Layer { cels, ..self.clone() }
    }

    pub fn cel_marks(&self) -> Vec<CelMark> {
        let mut transparent: HashMap<*const Raster, bool> = HashMap::new();
        let mut marks = Vec::with_capacity(self.cels.len());
        for (frame, cel) in self.cels.iter().enumerate() {
            let empty = *transparent.entry(Arc::as_ptr(cel)).or_insert_with(|| cel.is_transparent());
            let repeats_previous = frame > FIRST_FRAME && (Arc::ptr_eq(cel, &self.cels[frame - 1]) || **cel == *self.cels[frame - 1]);
            marks.push(match (empty, repeats_previous) {
                (true, _) => CelMark::Empty,
                (false, true) => CelMark::Inherited,
                (false, false) => CelMark::Drawn,
            });
        }
        marks
    }
}

impl Document {
    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn current_frame(&self) -> usize {
        self.frame
    }

    pub fn set_current_frame(&mut self, frame: usize) -> bool {
        if frame >= self.frames.len() || frame == self.frame {
            return false;
        }
        self.settle();
        self.frame = frame;
        self.sync_frame();
        true
    }

    pub(crate) fn sync_frame(&mut self) {
        self.frame = self.frame.min(self.frames.len().saturating_sub(1));
        for layer in &mut self.layers {
            layer.show_frame(self.frame);
        }
    }

    pub fn add_empty_frame(&mut self) -> bool {
        let blank = Arc::new(Raster::transparent(self.width, self.height));
        let cels = vec![blank; self.layers.len()];
        self.insert_frame(HistoryKind::AddFrame, self.frame + 1, Frame::DEFAULT, cels)
    }

    pub fn duplicate_frame(&mut self) -> bool {
        !self.duplicate_frames(&[self.frame]).is_empty()
    }

    pub fn duplicate_frames(&mut self, frames: &[usize]) -> Vec<usize> {
        let sources = sorted_unique(frames, self.frames.len());
        if sources.is_empty() || self.frames.len() + sources.len() > MAX_FRAMES {
            return Vec::new();
        }
        self.settle();
        let frame_before = self.frame;
        let first_new = sources[sources.len() - 1] + 1;
        let created: Vec<usize> = (first_new..first_new + sources.len()).collect();
        let mut changes = Vec::with_capacity(sources.len());
        for (&source, &index) in sources.iter().zip(&created) {
            let frame = self.frames[source];
            let cels: Vec<Arc<Raster>> = self.layers.iter().map(|layer| Arc::clone(layer.cel(source))).collect();
            self.apply_frame_insert(index, frame, &cels);
            changes.push(Change::FrameInserted { index, frame, cels });
        }
        self.frame = created[0];
        self.sync_frame();
        self.push_entry_from(HistoryKind::DuplicateFrame, changes, self.active, frame_before);
        created
    }

    fn insert_frame(&mut self, kind: HistoryKind, index: usize, frame: Frame, cels: Vec<Arc<Raster>>) -> bool {
        if self.frames.len() >= MAX_FRAMES {
            return false;
        }
        self.settle();
        let frame_before = self.frame;
        self.apply_frame_insert(index, frame, &cels);
        self.frame = index;
        self.sync_frame();
        self.push_entry_from(kind, vec![Change::FrameInserted { index, frame, cels }], self.active, frame_before);
        true
    }

    pub fn delete_frame(&mut self) -> bool {
        self.delete_frames(&[self.frame])
    }

    pub fn delete_frames(&mut self, frames: &[usize]) -> bool {
        let doomed = sorted_unique(frames, self.frames.len());
        if doomed.is_empty() || doomed.len() >= self.frames.len() {
            return false;
        }
        self.settle();
        let frame_before = self.frame;
        let mut changes = Vec::with_capacity(doomed.len());
        for &index in doomed.iter().rev() {
            let frame = self.frames[index];
            let cels = self.apply_frame_remove(index);
            changes.push(Change::FrameRemoved { index, frame, cels });
        }
        self.frame = doomed[0].min(self.frames.len() - 1);
        self.sync_frame();
        self.push_entry_from(HistoryKind::DeleteFrame, changes, self.active, frame_before);
        true
    }

    pub fn shift_frames(&mut self, frames: &[usize], delta: isize) -> Vec<usize> {
        let moved = sorted_unique(frames, self.frames.len());
        let (Some(&first), Some(&last)) = (moved.first(), moved.last()) else {
            return moved;
        };
        let count = self.frames.len();
        let delta = delta.clamp(-(first as isize), (count - 1 - last) as isize);
        if delta == 0 {
            return moved;
        }
        self.settle();
        let frame_before = self.frame;
        let mut selected = vec![false; count];
        moved.iter().for_each(|&index| selected[index] = true);
        let mut changes = Vec::new();
        for _ in 0..delta.unsigned_abs() {
            let pairs: Vec<(usize, usize)> = if delta < 0 { (1..count).map(|i| (i, i - 1)).collect() } else { (0..count - 1).rev().map(|i| (i, i + 1)).collect() };
            for (from, to) in pairs {
                if selected[from] && !selected[to] {
                    selected.swap(from, to);
                    self.apply_frame_move(from, to);
                    changes.push(Change::FrameMoved { from, to });
                    self.frame = if self.frame == from { to } else if self.frame == to { from } else { self.frame };
                }
            }
        }
        self.sync_frame();
        self.push_entry_from(HistoryKind::MoveFrame, changes, self.active, frame_before);
        selected.iter().enumerate().filter(|&(_, &on)| on).map(|(index, _)| index).collect()
    }

    pub fn reverse_frames(&mut self, frames: &[usize]) -> bool {
        let slots = sorted_unique(frames, self.frames.len());
        if slots.len() < 2 {
            return false;
        }
        self.settle();
        let frame_before = self.frame;
        let mut changes = Vec::new();
        for (&low, &high) in slots.iter().zip(slots.iter().rev()).take(slots.len() / 2) {
            for (from, to) in [(low, high), (high - 1, low)] {
                if from != to {
                    self.apply_frame_move(from, to);
                    changes.push(Change::FrameMoved { from, to });
                }
            }
        }
        if let Some(position) = slots.iter().position(|&slot| slot == frame_before) {
            self.frame = slots[slots.len() - 1 - position];
        }
        self.sync_frame();
        self.push_entry_from(HistoryKind::ReverseFrames, changes, self.active, frame_before);
        true
    }

    pub fn move_frame(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= self.frames.len() || to >= self.frames.len() {
            return false;
        }
        self.settle();
        let frame_before = self.frame;
        self.apply_frame_move(from, to);
        self.frame = to;
        self.sync_frame();
        self.push_entry_from(HistoryKind::MoveFrame, vec![Change::FrameMoved { from, to }], self.active, frame_before);
        true
    }

    pub fn set_frame_duration(&mut self, index: usize, duration_ms: u32) -> bool {
        let Some(&before) = self.frames.get(index) else {
            return false;
        };
        let after = Frame::clamped(duration_ms);
        if before == after {
            return false;
        }
        self.settle();
        self.frames[index] = after;
        self.push_entry(HistoryKind::FrameDuration, vec![Change::FrameDuration { index, before, after }], self.active);
        true
    }

    pub fn set_frames_duration(&mut self, frames: &[usize], duration_ms: u32) -> bool {
        let after = Frame::clamped(duration_ms);
        let changes: Vec<Change> = sorted_unique(frames, self.frames.len())
            .into_iter()
            .filter(|&index| self.frames[index] != after)
            .map(|index| Change::FrameDuration { index, before: self.frames[index], after })
            .collect();
        if changes.is_empty() {
            return false;
        }
        self.settle();
        for change in &changes {
            if let Change::FrameDuration { index, after, .. } = change {
                self.frames[*index] = *after;
            }
        }
        self.push_entry(HistoryKind::FrameDuration, changes, self.active);
        true
    }

    pub fn clear_cels(&mut self, frames: &[usize], layers: &[usize]) -> bool {
        let blank = Arc::new(Raster::transparent(self.width, self.height));
        let targets = self.cel_targets(frames, layers).into_iter().map(|(layer, frame)| (layer, frame, Arc::clone(&blank))).collect();
        self.replace_cels(HistoryKind::ClearCels, targets)
    }

    pub fn copy_cels(&self, frames: &[usize], layers: &[usize]) -> Option<CelClip> {
        let (frames, layers) = (sorted_unique(frames, self.frames.len()), sorted_unique(layers, self.layers.len()));
        if frames.is_empty() || layers.is_empty() {
            return None;
        }
        let cels = frames.iter().flat_map(|&frame| layers.iter().map(move |&layer| (layer, frame))).map(|(layer, frame)| Arc::clone(self.layers[layer].cel(frame))).collect();
        Some(CelClip { frames: frames.len(), layers: layers.len(), cels })
    }

    pub fn paste_cels(&mut self, clip: &CelClip, frame: usize, layer: usize) -> bool {
        let targets = (0..clip.frames)
            .flat_map(|frame_offset| (0..clip.layers).map(move |layer_offset| (frame_offset, layer_offset)))
            .filter(|&(frame_offset, layer_offset)| frame + frame_offset < self.frames.len() && layer + layer_offset < self.layers.len())
            .map(|(frame_offset, layer_offset)| (layer + layer_offset, frame + frame_offset, Arc::clone(&clip.cels[frame_offset * clip.layers + layer_offset])))
            .collect();
        self.replace_cels(HistoryKind::PasteCels, targets)
    }

    fn cel_targets(&self, frames: &[usize], layers: &[usize]) -> Vec<(usize, usize)> {
        let layers = sorted_unique(layers, self.layers.len());
        sorted_unique(frames, self.frames.len()).into_iter().flat_map(|frame| layers.iter().map(move |&layer| (layer, frame))).collect()
    }

    fn replace_cels(&mut self, kind: HistoryKind, targets: Vec<(usize, usize, Arc<Raster>)>) -> bool {
        self.settle();
        let mut changes = Vec::new();
        for (layer, frame, cel) in targets {
            let old = Arc::clone(self.layers[layer].cel(frame));
            if (old.width(), old.height()) != (cel.width(), cel.height()) {
                continue;
            }
            let area = old.content_bounds().union(cel.content_bounds());
            let (before, after) = (old.crop(area), cel.crop(area));
            if area.is_empty() || before == after {
                continue;
            }
            self.layers[layer].cels[frame] = cel;
            changes.push(Change::Pixels { layer, frame, origin: IPoint::new(area.x0, area.y0), before, after });
        }
        if changes.is_empty() {
            return false;
        }
        self.push_entry(kind, changes, self.active);
        true
    }

    pub fn uniform_duration(&self) -> Option<u32> {
        let first = self.frames.first()?.duration_ms;
        self.frames.iter().all(|frame| frame.duration_ms == first).then_some(first)
    }

    pub fn set_all_frame_durations(&mut self, duration_ms: u32) -> bool {
        let after = Frame::clamped(duration_ms);
        let changes: Vec<Change> = self.frames.iter().enumerate().filter(|(_, before)| **before != after).map(|(index, &before)| Change::FrameDuration { index, before, after }).collect();
        if changes.is_empty() {
            return false;
        }
        self.settle();
        self.frames.fill(after);
        self.push_entry(HistoryKind::FrameDuration, changes, self.active);
        true
    }

    pub(crate) fn apply_frame_insert(&mut self, index: usize, frame: Frame, cels: &[Arc<Raster>]) {
        self.frames.insert(index, frame);
        for (layer, cel) in self.layers.iter_mut().zip(cels) {
            layer.insert_cel(index, Arc::clone(cel));
        }
    }

    pub(crate) fn apply_frame_remove(&mut self, index: usize) -> Vec<Arc<Raster>> {
        self.frames.remove(index);
        self.layers.iter_mut().map(|layer| layer.remove_cel(index)).collect()
    }

    pub(crate) fn apply_frame_move(&mut self, from: usize, to: usize) {
        let frame = self.frames.remove(from);
        self.frames.insert(to, frame);
        for layer in &mut self.layers {
            layer.move_cel(from, to);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::IRect;
    use crate::raster::Rgba;

    const RED: Rgba = [255, 0, 0, 255];
    const SIZE: u32 = 8;

    fn document() -> Document {
        Document::new(SIZE, SIZE, [10, 10, 10, 255], "фон".into()).unwrap()
    }

    fn paint(document: &mut Document, x: u32, color: Rgba) {
        document.modify_active_layer(HistoryKind::Fill, IRect::from_size(x as i32, 0, 1, 1), |_, _, _, _| color);
    }

    #[test]
    fn duplicate_shares_the_cel_until_it_is_drawn_on() {
        let mut document = document();
        assert!(document.duplicate_frame());
        let layer = &document.layers()[0];
        assert!(Arc::ptr_eq(layer.cel(0), layer.cel(1)));
        paint(&mut document, 3, RED);
        let layer = &document.layers()[0];
        assert!(!Arc::ptr_eq(layer.cel(0), layer.cel(1)));
        assert_eq!(layer.cel(1).get(3, 0), RED);
        assert_ne!(layer.cel(0).get(3, 0), RED);
    }

    #[test]
    fn frame_operations_undo_and_redo_to_the_same_frame() {
        let mut document = document();
        document.duplicate_frame();
        paint(&mut document, 1, RED);
        document.add_empty_frame();
        assert_eq!((document.frame_count(), document.current_frame()), (3, 2));
        document.undo();
        assert_eq!((document.frame_count(), document.current_frame()), (2, 1));
        document.undo();
        assert_eq!(document.layers()[0].cel(1).get(1, 0), [10, 10, 10, 255]);
        document.undo();
        assert_eq!(document.frame_count(), 1);
        document.redo();
        document.redo();
        document.redo();
        assert_eq!((document.frame_count(), document.current_frame()), (3, 2));
        assert_eq!(document.layers()[0].cel(1).get(1, 0), RED);
    }

    #[test]
    fn drawing_is_undone_on_the_frame_it_was_made() {
        let mut document = document();
        document.duplicate_frame();
        paint(&mut document, 2, RED);
        document.set_current_frame(0);
        document.undo();
        assert_eq!(document.current_frame(), 1);
        assert_ne!(document.layers()[0].cel(1).get(2, 0), RED);
    }

    #[test]
    fn delete_and_move_keep_cels_with_their_frames() {
        let mut document = document();
        document.duplicate_frame();
        paint(&mut document, 1, RED);
        document.add_empty_frame();
        assert!(document.move_frame(2, 0));
        assert!(document.layers()[0].cel(0).is_transparent());
        assert_eq!(document.layers()[0].cel(2).get(1, 0), RED);
        assert!(document.delete_frame());
        assert_eq!(document.frame_count(), 2);
        document.undo();
        assert_eq!(document.frame_count(), 3);
        assert!(!document.set_current_frame(3));
    }

    #[test]
    fn last_frame_cannot_be_deleted_and_duration_is_clamped_and_undoable() {
        let mut document = document();
        assert!(!document.delete_frame());
        assert!(document.set_frame_duration(0, 1));
        assert_eq!(document.frames()[0].duration_ms, MIN_FRAME_DURATION_MS);
        document.undo();
        assert_eq!(document.frames()[0], Frame::DEFAULT);
    }

    #[test]
    fn setting_every_duration_is_one_undo_step() {
        let mut document = document();
        document.duplicate_frame();
        document.duplicate_frame();
        document.set_frame_duration(1, 400);
        assert_eq!(document.uniform_duration(), None);
        assert!(document.set_all_frame_durations(50));
        assert_eq!(document.uniform_duration(), Some(50));
        assert!(!document.set_all_frame_durations(50));
        document.undo();
        assert_eq!(document.frames().iter().map(|frame| frame.duration_ms).collect::<Vec<_>>(), [100, 400, 100]);
    }

    #[test]
    fn marks_tell_empty_inherited_and_drawn_cels_apart() {
        let mut document = document();
        document.add_layer("верх".into());
        document.duplicate_frame();
        document.duplicate_frame();
        paint(&mut document, 4, RED);
        assert_eq!(document.layers()[0].cel_marks(), [CelMark::Drawn, CelMark::Inherited, CelMark::Inherited]);
        assert_eq!(document.layers()[1].cel_marks(), [CelMark::Empty, CelMark::Empty, CelMark::Drawn]);
    }

    #[test]
    fn layer_added_later_has_a_cel_for_every_frame_and_flatten_keeps_frames() {
        let mut document = document();
        document.duplicate_frame();
        document.add_layer("верх".into());
        assert_eq!(document.layers()[1].frame_count(), 2);
        paint(&mut document, 5, RED);
        document.set_current_frame(0);
        assert!(document.flatten("итог".into()));
        assert_eq!(document.layers().len(), 1);
        assert_eq!(document.layers()[0].cel(1).get(5, 0), RED);
        assert_ne!(document.layers()[0].cel(0).get(5, 0), RED);
    }

    #[test]
    fn merge_down_and_canvas_changes_apply_to_every_frame() {
        let mut document = document();
        document.add_layer("верх".into());
        document.duplicate_frame();
        paint(&mut document, 2, RED);
        assert!(document.merge_layer_down());
        assert_eq!(document.layers()[0].cel(1).get(2, 0), RED);
        document.resize_canvas(12, 6, crate::document::Anchor::TopLeft).unwrap();
        assert!(document.layers()[0].cels().iter().all(|cel| (cel.width(), cel.height()) == (12, 6)));
        document.undo();
        assert!(document.layers()[0].cels().iter().all(|cel| (cel.width(), cel.height()) == (SIZE, SIZE)));
    }
    fn marked_frames(count: usize) -> Document {
        let mut document = document();
        for _ in 1..count {
            document.duplicate_frame();
        }
        for frame in 0..count {
            document.set_current_frame(frame);
            paint(&mut document, frame as u32, RED);
        }
        document
    }

    fn order(document: &Document) -> Vec<usize> {
        (0..document.frame_count()).map(|frame| (0..SIZE).find(|&x| document.layers()[0].cel(frame).get(x, 0) == RED).unwrap() as usize).collect()
    }

    #[test]
    fn duplicating_a_range_inserts_copies_after_it_as_one_undo_step() {
        let mut document = marked_frames(4);
        assert_eq!(document.duplicate_frames(&[1, 2]), [3, 4]);
        assert_eq!(order(&document), [0, 1, 2, 1, 2, 3]);
        document.undo();
        assert_eq!(order(&document), [0, 1, 2, 3]);
    }

    #[test]
    fn deleting_a_range_keeps_at_least_one_frame_and_undoes_in_one_step() {
        let mut document = marked_frames(5);
        assert!(document.delete_frames(&[1, 3]));
        assert_eq!(order(&document), [0, 2, 4]);
        document.undo();
        assert_eq!(order(&document), [0, 1, 2, 3, 4]);
        assert!(!document.delete_frames(&[0, 1, 2, 3, 4]));
    }

    #[test]
    fn shifting_a_scattered_selection_moves_every_frame_and_stops_at_the_edge() {
        let mut document = marked_frames(5);
        assert_eq!(document.shift_frames(&[1, 2, 4], -1), [0, 1, 3]);
        assert_eq!(order(&document), [1, 2, 0, 4, 3]);
        assert_eq!(document.shift_frames(&[0, 1, 3], -3), [0, 1, 3]);
        document.undo();
        assert_eq!(order(&document), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn reversing_a_selection_swaps_only_the_selected_slots() {
        let mut document = marked_frames(5);
        assert!(document.reverse_frames(&[0, 2, 4]));
        assert_eq!(order(&document), [4, 1, 2, 3, 0]);
        assert!(document.reverse_frames(&[0, 1, 2, 3, 4]));
        assert_eq!(order(&document), [0, 3, 2, 1, 4]);
        document.undo();
        document.undo();
        assert_eq!(order(&document), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn durations_apply_to_the_chosen_frames_only() {
        let mut document = marked_frames(3);
        assert!(document.set_frames_duration(&[0, 2], 250));
        assert_eq!(document.frames().iter().map(|frame| frame.duration_ms).collect::<Vec<_>>(), [250, 100, 250]);
        document.undo();
        assert_eq!(document.uniform_duration(), Some(100));
    }

    #[test]
    fn clearing_and_pasting_cels_undo_without_touching_other_cels() {
        let mut document = marked_frames(3);
        assert!(document.clear_cels(&[0, 1], &[0]));
        assert!(document.layers()[0].cel(0).is_transparent() && document.layers()[0].cel(1).is_transparent());
        assert_eq!(document.layers()[0].cel(2).get(2, 0), RED);
        let clip = document.copy_cels(&[2], &[0]).unwrap();
        assert!(document.paste_cels(&clip, 0, 0));
        assert_eq!(document.layers()[0].cel(0).get(2, 0), RED);
        document.undo();
        assert!(document.layers()[0].cel(0).is_transparent());
        document.undo();
        assert_eq!(document.layers()[0].cel(0).get(0, 0), RED);
        assert_eq!(document.layers()[0].cel(1).get(1, 0), RED);
    }
}
