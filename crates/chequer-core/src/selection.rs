use std::collections::HashMap;
use std::sync::Arc;

use tiny_skia::{FillRule, Mask, PathBuilder, Rect, Transform};

use crate::geom::{FPoint, FRect, IPoint, IRect};
use crate::raster::{Raster, mul_div_255};

const INSIDE_THRESHOLD: u8 = 128;
const FULL_COVERAGE: u8 = u8::MAX;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CombineMode {
    #[default]
    Replace,
    Add,
    Subtract,
    Xor,
    Intersect,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LassoSmoothing {
    #[default]
    Classic,
    Full,
    Off,
}

const CLASSIC_SUBPIXELS: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: IPoint,
    pub to: IPoint,
}

#[derive(Debug)]
pub struct Selection {
    bounds: IRect,
    coverage: Vec<u8>,
    outline: Vec<Edge>,
    contours: Vec<Vec<IPoint>>,
}

pub type SharedSelection = Arc<Selection>;

impl Selection {
    pub fn from_coverage(bounds: IRect, coverage: Vec<u8>) -> Option<Selection> {
        debug_assert_eq!(coverage.len(), bounds.width() as usize * bounds.height() as usize);
        if bounds.is_empty() || coverage.iter().all(|&c| c == 0) {
            return None;
        }
        let (outline, contours) = if coverage.iter().all(|&c| c >= INSIDE_THRESHOLD) {
            rectangle_outline(bounds)
        } else {
            let outline = trace_outline(bounds, &coverage);
            let contours = chain_contours(&outline);
            (outline, contours)
        };
        Some(Selection { bounds, coverage, outline, contours })
    }

    pub fn from_rect(rect: IRect) -> Option<Selection> {
        if rect.is_empty() {
            return None;
        }
        let size = rect.width() as usize * rect.height() as usize;
        Selection::from_coverage(rect, vec![FULL_COVERAGE; size])
    }

    pub fn from_frame(frame: FRect, antialias: bool) -> Option<Selection> {
        Selection::from_shape(frame, antialias, |rect| Some(PathBuilder::from_rect(rect)))
    }

    pub fn from_ellipse(frame: FRect, antialias: bool) -> Option<Selection> {
        Selection::from_shape(frame, antialias, PathBuilder::from_oval)
    }

    fn from_shape(frame: FRect, antialias: bool, path_in: impl FnOnce(Rect) -> Option<tiny_skia::Path>) -> Option<Selection> {
        let frame = frame.normalized();
        let bounds = frame.outer();
        let rect = Rect::from_ltrb(frame.x0 - bounds.x0 as f32, frame.y0 - bounds.y0 as f32, frame.x1 - bounds.x0 as f32, frame.y1 - bounds.y0 as f32)?;
        Selection::rasterize(bounds, &path_in(rect)?, antialias, Transform::identity())
    }

    pub fn from_polygon(points: &[FPoint], smoothing: LassoSmoothing) -> Option<Selection> {
        if points.len() < 3 {
            return None;
        }
        let min_x = points.iter().map(|p| p.x).fold(f32::MAX, f32::min).floor() as i32;
        let min_y = points.iter().map(|p| p.y).fold(f32::MAX, f32::min).floor() as i32;
        let max_x = points.iter().map(|p| p.x).fold(f32::MIN, f32::max).ceil() as i32;
        let max_y = points.iter().map(|p| p.y).fold(f32::MIN, f32::max).ceil() as i32;
        let bounds = IRect::new(min_x, min_y, max_x.max(min_x + 1), max_y.max(min_y + 1));
        let mut builder = PathBuilder::new();
        builder.move_to(points[0].x, points[0].y);
        for p in &points[1..] {
            builder.line_to(p.x, p.y);
        }
        builder.close();
        let path = builder.finish()?;
        let shift = Transform::from_translate(-bounds.x0 as f32, -bounds.y0 as f32);
        match smoothing {
            LassoSmoothing::Full => Selection::rasterize(bounds, &path, true, shift),
            LassoSmoothing::Off => Selection::rasterize(bounds, &path, false, shift),
            LassoSmoothing::Classic => Selection::rasterize_on_subpixels(bounds, &path, shift),
        }
    }

    fn rasterize_on_subpixels(bounds: IRect, path: &tiny_skia::Path, shift: Transform) -> Option<Selection> {
        let (width, height) = (bounds.width() as u32, bounds.height() as u32);
        let factor = CLASSIC_SUBPIXELS;
        let mut fine = Mask::new(width * factor, height * factor)?;
        fine.fill_path(path, FillRule::Winding, false, shift.post_scale(factor as f32, factor as f32));
        let samples = factor * factor;
        let coverage = (0..height * width)
            .map(|index| {
                let (x, y) = (index % width, index / width);
                let filled = (0..samples).filter(|sub| fine.data()[((y * factor + sub / factor) * width * factor + x * factor + sub % factor) as usize] != 0).count() as u32;
                (filled * FULL_COVERAGE as u32 / samples) as u8
            })
            .collect();
        Selection::from_coverage(bounds, coverage)
    }

    fn rasterize(bounds: IRect, path: &tiny_skia::Path, antialias: bool, transform: Transform) -> Option<Selection> {
        let mut mask = Mask::new(bounds.width() as u32, bounds.height() as u32)?;
        mask.fill_path(path, FillRule::Winding, antialias, transform);
        Selection::from_coverage(bounds, mask.take())
    }

    pub fn bounds(&self) -> IRect {
        self.bounds
    }

    pub fn outline(&self) -> &[Edge] {
        &self.outline
    }

    pub fn contours(&self) -> &[Vec<IPoint>] {
        &self.contours
    }

    pub fn coverage_at(&self, x: i32, y: i32) -> u8 {
        if !self.bounds.contains(IPoint::new(x, y)) {
            return 0;
        }
        let index = (y - self.bounds.y0) as usize * self.bounds.width() as usize + (x - self.bounds.x0) as usize;
        self.coverage[index]
    }

    pub fn coverage_row(&self, y: i32, x0: i32, x1: i32) -> &[u8] {
        let start = (y - self.bounds.y0) as usize * self.bounds.width() as usize + (x0 - self.bounds.x0) as usize;
        &self.coverage[start..start + (x1 - x0) as usize]
    }

    pub fn contains(&self, p: IPoint) -> bool {
        self.coverage_at(p.x, p.y) >= INSIDE_THRESHOLD
    }

    pub fn translate(&mut self, dx: i32, dy: i32) {
        self.bounds = self.bounds.translate(dx, dy);
        for edge in &mut self.outline {
            edge.from = IPoint::new(edge.from.x + dx, edge.from.y + dy);
            edge.to = IPoint::new(edge.to.x + dx, edge.to.y + dy);
        }
        for point in self.contours.iter_mut().flatten() {
            *point = IPoint::new(point.x + dx, point.y + dy);
        }
    }

    pub fn resampled_to(&self, frame: IRect) -> Option<Selection> {
        if frame.is_empty() {
            return None;
        }
        let (source_w, source_h) = (self.bounds.width() as f32, self.bounds.height() as f32);
        let (width, height) = (frame.width() as usize, frame.height() as usize);
        let columns: Vec<i32> = (0..width).map(|x| self.bounds.x0 + ((x as f32 + 0.5) / width as f32 * source_w) as i32).collect();
        let mut coverage = Vec::with_capacity(width * height);
        for y in 0..height {
            let source_y = self.bounds.y0 + ((y as f32 + 0.5) / height as f32 * source_h) as i32;
            coverage.extend(columns.iter().map(|&source_x| self.coverage_at(source_x, source_y)));
        }
        Selection::from_coverage(frame, coverage)
    }

    pub fn clipped_to(&self, canvas: IRect) -> Option<Selection> {
        let area = self.bounds.intersect(canvas);
        if area == self.bounds {
            return Some(self.clone());
        }
        if area.is_empty() {
            return None;
        }
        let mut coverage = Vec::with_capacity(area.width() as usize * area.height() as usize);
        for y in area.y0..area.y1 {
            coverage.extend_from_slice(self.coverage_row(y, area.x0, area.x1));
        }
        Selection::from_coverage(area, coverage)
    }

    pub fn inverted(&self, canvas: IRect) -> Option<Selection> {
        let mut coverage = vec![FULL_COVERAGE; canvas.width() as usize * canvas.height() as usize];
        let area = self.bounds.intersect(canvas);
        for y in area.y0..area.y1 {
            let start = (y - canvas.y0) as usize * canvas.width() as usize + (area.x0 - canvas.x0) as usize;
            let inner = self.coverage_row(y, area.x0, area.x1);
            for (dst, &c) in coverage[start..start + inner.len()].iter_mut().zip(inner) {
                *dst = FULL_COVERAGE - c;
            }
        }
        Selection::from_coverage(canvas, coverage)
    }

    pub fn mask_alpha(&self, raster: &mut Raster) {
        for y in 0..raster.height() {
            for x in 0..raster.width() {
                let coverage = self.coverage_at(self.bounds.x0 + x as i32, self.bounds.y0 + y as i32);
                let mut pixel = raster.get(x, y);
                pixel[3] = mul_div_255(pixel[3] as u32, coverage as u32) as u8;
                raster.set(x, y, pixel);
            }
        }
    }
}

impl Clone for Selection {
    fn clone(&self) -> Self {
        Selection { bounds: self.bounds, coverage: self.coverage.clone(), outline: self.outline.clone(), contours: self.contours.clone() }
    }
}

impl PartialEq for Selection {
    fn eq(&self, other: &Self) -> bool {
        self.bounds == other.bounds && self.coverage == other.coverage
    }
}

pub fn combine(base: Option<&Selection>, shape: &Selection, mode: CombineMode) -> Option<Selection> {
    let Some(base) = base.filter(|_| mode != CombineMode::Replace) else {
        return mode_without_base(shape, mode);
    };
    let area = match mode {
        CombineMode::Intersect => base.bounds.intersect(shape.bounds),
        CombineMode::Subtract => base.bounds,
        CombineMode::Replace | CombineMode::Add | CombineMode::Xor => base.bounds.union(shape.bounds),
    };
    if area.is_empty() {
        return None;
    }
    let mut coverage = Vec::with_capacity(area.width() as usize * area.height() as usize);
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            coverage.push(combine_pixel(base.coverage_at(x, y), shape.coverage_at(x, y), mode));
        }
    }
    Selection::from_coverage(area, coverage)
}

fn mode_without_base(shape: &Selection, mode: CombineMode) -> Option<Selection> {
    match mode {
        CombineMode::Subtract | CombineMode::Intersect => None,
        CombineMode::Replace | CombineMode::Add | CombineMode::Xor => Some(shape.clone()),
    }
}

fn combine_pixel(a: u8, b: u8, mode: CombineMode) -> u8 {
    let (a, b) = (a as u32, b as u32);
    let full = FULL_COVERAGE as u32;
    let value = match mode {
        CombineMode::Replace => b,
        CombineMode::Add => a + b - mul_div_255(a, b),
        CombineMode::Subtract => mul_div_255(a, full - b),
        CombineMode::Xor => mul_div_255(a, full - b) + mul_div_255(b, full - a),
        CombineMode::Intersect => mul_div_255(a, b),
    };
    value.min(full) as u8
}

fn rectangle_outline(bounds: IRect) -> (Vec<Edge>, Vec<Vec<IPoint>>) {
    let unit = IRect::new(bounds.x0, bounds.y0, bounds.x0 + 1, bounds.y0 + 1);
    let stretch = |p: IPoint| IPoint::new(if p.x > bounds.x0 { bounds.x1 } else { p.x }, if p.y > bounds.y0 { bounds.y1 } else { p.y });
    let outline: Vec<Edge> = trace_outline(unit, &[FULL_COVERAGE]).into_iter().map(|e| Edge { from: stretch(e.from), to: stretch(e.to) }).collect();
    let contours = chain_contours(&outline);
    (outline, contours)
}

fn trace_outline(bounds: IRect, coverage: &[u8]) -> Vec<Edge> {
    let width = bounds.width();
    let height = bounds.height();
    let inside = |x: i32, y: i32| -> bool {
        x >= 0
            && y >= 0
            && x < width
            && y < height
            && coverage[(y * width + x) as usize] >= INSIDE_THRESHOLD
    };
    let mut edges = Vec::new();
    for y in 0..=height {
        let mut run: Option<(i32, bool)> = None;
        for x in 0..=width {
            let side = (x < width && inside(x, y) != inside(x, y - 1)).then(|| inside(x, y));
            if run.map(|(_, s)| s) != side {
                if let Some((start, below)) = run {
                    edges.push(if below { edge(bounds, start, y, x, y) } else { edge(bounds, x, y, start, y) });
                }
                run = side.map(|s| (x, s));
            }
        }
    }
    for x in 0..=width {
        let mut run: Option<(i32, bool)> = None;
        for y in 0..=height {
            let side = (y < height && inside(x, y) != inside(x - 1, y)).then(|| inside(x - 1, y));
            if run.map(|(_, s)| s) != side {
                if let Some((start, left)) = run {
                    edges.push(if left { edge(bounds, x, start, x, y) } else { edge(bounds, x, y, x, start) });
                }
                run = side.map(|s| (y, s));
            }
        }
    }
    edges
}

fn chain_contours(edges: &[Edge]) -> Vec<Vec<IPoint>> {
    let mut outgoing: HashMap<IPoint, Vec<usize>> = HashMap::new();
    for (index, edge) in edges.iter().enumerate() {
        outgoing.entry(edge.from).or_default().push(index);
    }
    let mut used = vec![false; edges.len()];
    let mut contours = Vec::new();
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        let mut points = vec![edges[start].from];
        let mut current = Some(start);
        while let Some(index) = current {
            used[index] = true;
            let end = edges[index].to;
            points.push(end);
            current = outgoing.get(&end).and_then(|candidates| candidates.iter().copied().find(|&next| !used[next]));
        }
        contours.push(points);
    }
    contours
}

fn edge(bounds: IRect, x0: i32, y0: i32, x1: i32, y1: i32) -> Edge {
    Edge {
        from: IPoint::new(bounds.x0 + x0, bounds.y0 + y0),
        to: IPoint::new(bounds.x0 + x1, bounds.y0 + y1),
    }
}

pub fn flood_select(source: &Raster, seed: IPoint, tolerance: f32, contiguous: bool, wrap: bool) -> Option<Selection> {
    let target = source.get_at(seed)?;
    let limit = tolerance_limit(tolerance);
    let width = source.width() as usize;
    let height = source.height() as usize;
    let mut matched = vec![false; width * height];
    if contiguous && wrap {
        flood_wrapping(source, seed, target, limit, &mut matched);
    } else if contiguous {
        flood_contiguous(source, seed, target, limit, &mut matched);
    } else {
        for y in 0..source.height() {
            for x in 0..source.width() {
                matched[y as usize * width + x as usize] = color_distance(source.get(x, y), target) <= limit;
            }
        }
    }
    trimmed_selection(source.bounds(), &matched)
}

fn trimmed_selection(canvas: IRect, matched: &[bool]) -> Option<Selection> {
    let width = canvas.width() as usize;
    let mut frame = IRect::EMPTY;
    for (index, _) in matched.iter().enumerate().filter(|(_, m)| **m) {
        let (x, y) = ((index % width) as i32, (index / width) as i32);
        frame = frame.union(IRect::new(x, y, x + 1, y + 1));
    }
    if frame.is_empty() {
        return None;
    }
    let mut coverage = Vec::with_capacity(frame.width() as usize * frame.height() as usize);
    for y in frame.y0..frame.y1 {
        let row = y as usize * width;
        coverage.extend(matched[row + frame.x0 as usize..row + frame.x1 as usize].iter().map(|&m| if m { FULL_COVERAGE } else { 0 }));
    }
    Selection::from_coverage(frame, coverage)
}

fn tolerance_limit(tolerance: f32) -> u32 {
    let scaled = tolerance.clamp(0.0, 1.0) * FULL_COVERAGE as f32;
    (scaled * scaled) as u32
}

fn color_distance(a: [u8; 4], b: [u8; 4]) -> u32 {
    let sum: u32 = a.iter().zip(b.iter()).map(|(&p, &q)| (p as i32 - q as i32).unsigned_abs().pow(2)).sum();
    sum / 4
}

fn flood_wrapping(source: &Raster, seed: IPoint, target: [u8; 4], limit: u32, matched: &mut [bool]) {
    let (width, height) = (source.width() as i32, source.height() as i32);
    let mut take = |p: IPoint, stack: &mut Vec<IPoint>| {
        let index = (p.y * width + p.x) as usize;
        if !matched[index] && color_distance(source.get(p.x as u32, p.y as u32), target) <= limit {
            matched[index] = true;
            stack.push(p);
        }
    };
    let mut stack = Vec::new();
    take(seed, &mut stack);
    while let Some(p) = stack.pop() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            take(IPoint::new((p.x + dx).rem_euclid(width), (p.y + dy).rem_euclid(height)), &mut stack);
        }
    }
}

fn flood_contiguous(source: &Raster, seed: IPoint, target: [u8; 4], limit: u32, matched: &mut [bool]) {
    let width = source.width() as i32;
    let height = source.height() as i32;
    let accepts = |x: i32, y: i32, matched: &[bool]| {
        !matched[(y * width + x) as usize] && color_distance(source.get(x as u32, y as u32), target) <= limit
    };
    let mut stack = vec![seed];
    while let Some(p) = stack.pop() {
        if !accepts(p.x, p.y, matched) {
            continue;
        }
        let mut left = p.x;
        while left > 0 && accepts(left - 1, p.y, matched) {
            left -= 1;
        }
        let mut right = p.x;
        while right + 1 < width && accepts(right + 1, p.y, matched) {
            right += 1;
        }
        for x in left..=right {
            matched[(p.y * width + x) as usize] = true;
        }
        for row in [p.y - 1, p.y + 1] {
            if row < 0 || row >= height {
                continue;
            }
            let mut in_run = false;
            for x in left..=right {
                let can = accepts(x, row, matched);
                if can && !in_run {
                    stack.push(IPoint::new(x, row));
                }
                in_run = can;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_selection(x0: i32, y0: i32, x1: i32, y1: i32) -> Selection {
        Selection::from_rect(IRect::new(x0, y0, x1, y1)).unwrap()
    }

    #[test]
    fn resampling_a_mask_keeps_its_shape_proportions() {
        let ring = combine(Some(&rect_selection(0, 0, 8, 8)), &rect_selection(2, 2, 6, 6), CombineMode::Subtract).unwrap();
        let doubled = ring.resampled_to(IRect::new(10, 10, 26, 26)).unwrap();
        assert_eq!(doubled.bounds(), IRect::new(10, 10, 26, 26));
        assert!(doubled.contains(IPoint::new(11, 11)));
        assert!(!doubled.contains(IPoint::new(18, 18)));
    }

    #[test]
    fn rect_outline_is_four_edges() {
        assert_eq!(rect_selection(2, 3, 6, 8).outline().len(), 4);
    }

    #[test]
    fn rect_outline_runs_clockwise() {
        let selection = rect_selection(2, 3, 6, 8);
        let edges = selection.outline();
        let has = |from: (i32, i32), to: (i32, i32)| {
            edges.iter().any(|e| e.from == IPoint::new(from.0, from.1) && e.to == IPoint::new(to.0, to.1))
        };
        assert!(has((2, 3), (6, 3)) && has((6, 3), (6, 8)) && has((6, 8), (2, 8)) && has((2, 8), (2, 3)));
    }

    #[test]
    fn rect_has_one_closed_contour() {
        let selection = rect_selection(2, 3, 6, 8);
        let [contour] = selection.contours() else {
            panic!("ожидался один контур");
        };
        assert_eq!(contour.first(), contour.last());
        assert_eq!(contour.len(), 5);
    }

    #[test]
    fn disjoint_rects_have_two_contours_and_a_ring_has_two() {
        let a = rect_selection(0, 0, 2, 2);
        let b = rect_selection(10, 10, 12, 12);
        assert_eq!(combine(Some(&a), &b, CombineMode::Add).unwrap().contours().len(), 2);
        let ring = combine(Some(&rect_selection(0, 0, 9, 9)), &rect_selection(3, 3, 6, 6), CombineMode::Subtract).unwrap();
        assert_eq!(ring.contours().len(), 2);
    }

    #[test]
    fn diagonal_touching_squares_still_chain_into_closed_loops() {
        let both = combine(Some(&rect_selection(0, 0, 2, 2)), &rect_selection(2, 2, 4, 4), CombineMode::Add).unwrap();
        assert!(both.contours().iter().all(|contour| contour.first() == contour.last()));
        let edges: usize = both.contours().iter().map(|contour| contour.len() - 1).sum();
        assert_eq!(edges, both.outline().len());
    }

    #[test]
    fn contours_follow_the_selection_when_it_moves() {
        let mut moved = rect_selection(2, 3, 6, 8);
        moved.translate(5, -1);
        assert_eq!(moved.contours(), rect_selection(7, 2, 11, 7).contours());
    }

    #[test]
    fn add_of_disjoint_rects_covers_both() {
        let a = rect_selection(0, 0, 2, 2);
        let b = rect_selection(10, 10, 12, 12);
        let sum = combine(Some(&a), &b, CombineMode::Add).unwrap();
        assert!(sum.contains(IPoint::new(1, 1)) && sum.contains(IPoint::new(11, 11)));
        assert!(!sum.contains(IPoint::new(5, 5)));
    }

    #[test]
    fn subtract_everything_gives_none() {
        let a = rect_selection(0, 0, 4, 4);
        assert!(combine(Some(&a), &a, CombineMode::Subtract).is_none());
    }

    #[test]
    fn intersect_keeps_overlap_only() {
        let a = rect_selection(0, 0, 4, 4);
        let b = rect_selection(2, 2, 6, 6);
        let both = combine(Some(&a), &b, CombineMode::Intersect).unwrap();
        assert_eq!(both.bounds(), IRect::new(2, 2, 4, 4));
    }

    #[test]
    fn replace_ignores_base() {
        let a = rect_selection(0, 0, 4, 4);
        let b = rect_selection(10, 10, 12, 12);
        let out = combine(Some(&a), &b, CombineMode::Replace).unwrap();
        assert!(!out.contains(IPoint::new(1, 1)));
    }

    #[test]
    fn invert_swaps_inside_and_outside() {
        let canvas = IRect::new(0, 0, 8, 8);
        let inverted = rect_selection(2, 2, 4, 4).inverted(canvas).unwrap();
        assert!(!inverted.contains(IPoint::new(3, 3)));
        assert!(inverted.contains(IPoint::new(0, 0)));
    }

    #[test]
    fn ellipse_contains_center_not_corner() {
        let e = Selection::from_ellipse(IRect::new(0, 0, 20, 20).into(), true).unwrap();
        assert!(e.contains(IPoint::new(10, 10)));
        assert!(!e.contains(IPoint::new(0, 0)));
    }

    #[test]
    fn polygon_triangle_covers_inside_point() {
        let tri = [FPoint::new(0.0, 0.0), FPoint::new(20.0, 0.0), FPoint::new(0.0, 20.0)];
        let s = Selection::from_polygon(&tri, LassoSmoothing::Full).unwrap();
        assert!(s.contains(IPoint::new(3, 3)));
        assert!(!s.contains(IPoint::new(18, 18)));
    }

    #[test]
    fn wand_contiguous_stops_at_different_color() {
        let mut r = Raster::filled(6, 1, [10, 10, 10, 255]);
        r.set(3, 0, [200, 0, 0, 255]);
        let s = flood_select(&r, IPoint::new(0, 0), 0.1, true, false).unwrap();
        assert!(s.contains(IPoint::new(2, 0)));
        assert!(!s.contains(IPoint::new(3, 0)));
        assert!(!s.contains(IPoint::new(4, 0)));
    }

    #[test]
    fn wand_global_picks_all_matching() {
        let mut r = Raster::filled(6, 1, [10, 10, 10, 255]);
        r.set(3, 0, [200, 0, 0, 255]);
        let s = flood_select(&r, IPoint::new(0, 0), 0.1, false, false).unwrap();
        assert!(s.contains(IPoint::new(4, 0)));
    }

    #[test]
    fn clipped_selection_stays_inside_canvas() {
        let s = rect_selection(-5, -5, 5, 5).clipped_to(IRect::new(0, 0, 10, 10)).unwrap();
        assert_eq!(s.bounds(), IRect::new(0, 0, 5, 5));
    }

    #[test]
    fn wrapping_flood_crosses_the_edge() {
        let mut r = Raster::filled(6, 1, [255, 255, 255, 255]);
        r.set(2, 0, [0, 0, 0, 255]);
        let plain = flood_select(&r, IPoint::new(0, 0), 0.1, true, false).unwrap();
        assert_eq!(plain.coverage_at(5, 0), 0);
        let wrapped = flood_select(&r, IPoint::new(0, 0), 0.1, true, true).unwrap();
        assert_eq!(wrapped.coverage_at(5, 0), 255);
        assert_eq!(wrapped.coverage_at(2, 0), 0);
    }
}
