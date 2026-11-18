
use chequer_core::geom::{FPoint, FRect};
use chequer_core::transform::Sides;
use eframe::egui::{Color32, CursorIcon, Modifiers, Painter, Pos2, Rect, Vec2, vec2};

use crate::ants::Ants;
use crate::skin::{self, Bevel, Plate, Skin};

const HANDLE_RADIUS: f32 = 3.0;
const HANDLE_HOT_EXTRA_RADIUS: f32 = 1.5;
pub const HANDLE_REACH: f32 = 13.0;
const RING_SEGMENTS: usize = 16;
pub const ROTATE_ZONE_WIDTH: f32 = 72.0;
const MOVE_ICON_SIDE: f32 = 22.0;
const MOVE_ICON_OFFSET: f32 = 24.0;
const PIVOT_RADIUS: f32 = 5.5;
const PIVOT_CROSS_HALF: f32 = 3.5;
const PIVOT_HIT_MARGIN: f32 = 6.0;
const MOVE_ICON_HIT_MARGIN: f32 = 5.0;
const ROTATE_ICON_SIDE: f32 = 24.0;
const RIGHT_ANGLE: f32 = std::f32::consts::FRAC_PI_2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
    North,
    South,
    West,
    East,
}

impl Handle {
    pub const ALL: [Handle; 8] = [
        Handle::NorthWest,
        Handle::NorthEast,
        Handle::SouthWest,
        Handle::SouthEast,
        Handle::North,
        Handle::South,
        Handle::West,
        Handle::East,
    ];

    pub fn sides(self) -> Sides {
        let x = match self {
            Handle::NorthWest | Handle::West | Handle::SouthWest => -1,
            Handle::NorthEast | Handle::East | Handle::SouthEast => 1,
            Handle::North | Handle::South => 0,
        };
        let y = match self {
            Handle::NorthWest | Handle::North | Handle::NorthEast => -1,
            Handle::SouthWest | Handle::South | Handle::SouthEast => 1,
            Handle::West | Handle::East => 0,
        };
        Sides { x, y }
    }

    pub fn is_corner(self) -> bool {
        let sides = self.sides();
        sides.x != 0 && sides.y != 0
    }

    pub fn cursor(self, angle: f32) -> CursorIcon {
        let sides = self.sides();
        let direction = f32::from(sides.y).atan2(f32::from(sides.x)) + angle;
        let eighth = (direction / (RIGHT_ANGLE / 2.0)).round() as i32;
        match eighth.rem_euclid(4) {
            0 => CursorIcon::ResizeHorizontal,
            1 => CursorIcon::ResizeNwSe,
            2 => CursorIcon::ResizeVertical,
            _ => CursorIcon::ResizeNeSw,
        }
    }
}

pub type Frame = FRect;

#[derive(Clone, Copy, Debug)]
pub struct ScreenBox {
    pub center: Pos2,
    pub half: Vec2,
    pub angle: f32,
}

impl ScreenBox {
    pub fn from_rect(rect: Rect) -> Self {
        Self { center: rect.center(), half: rect.size() / 2.0, angle: 0.0 }
    }

    fn axes(&self) -> (Vec2, Vec2) {
        let (sin, cos) = self.angle.sin_cos();
        (vec2(cos, sin), vec2(-sin, cos))
    }

    pub fn point(&self, u: f32, v: f32) -> Pos2 {
        let (ex, ey) = self.axes();
        self.center + ex * ((u - 0.5) * 2.0 * self.half.x) + ey * ((v - 0.5) * 2.0 * self.half.y)
    }

    fn local(&self, pos: Pos2) -> Vec2 {
        let (ex, ey) = self.axes();
        let offset = pos - self.center;
        vec2(offset.dot(ex), offset.dot(ey))
    }

    pub fn distance_outside(&self, pos: Pos2) -> f32 {
        let local = self.local(pos);
        vec2((local.x.abs() - self.half.x).max(0.0), (local.y.abs() - self.half.y).max(0.0)).length()
    }

    pub fn handle_position(&self, handle: Handle) -> Pos2 {
        let sides = handle.sides();
        self.point(f32::from(sides.x) / 2.0 + 0.5, f32::from(sides.y) / 2.0 + 0.5)
    }

    pub fn hit_handle(&self, pointer: Pos2) -> Option<Handle> {
        Handle::ALL
            .into_iter()
            .map(|handle| (handle, self.handle_position(handle).distance(pointer)))
            .filter(|(_, distance)| *distance <= HANDLE_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(handle, _)| handle)
    }

    pub fn move_icon_center(&self) -> Pos2 {
        let (ex, ey) = self.axes();
        self.point(1.0, 1.0) + (ex + ey) * MOVE_ICON_OFFSET / std::f32::consts::SQRT_2
    }

    pub fn hits_move_icon(&self, pointer: Pos2) -> bool {
        self.move_icon_center().distance(pointer) <= MOVE_ICON_SIDE / 2.0 + MOVE_ICON_HIT_MARGIN
    }

}

pub fn in_rotate_band(distance_outside: f32) -> bool {
    distance_outside > HANDLE_REACH && distance_outside <= ROTATE_ZONE_WIDTH
}

pub fn hits_pivot(pivot: Pos2, pointer: Pos2) -> bool {
    pivot.distance(pointer) <= PIVOT_RADIUS + PIVOT_HIT_MARGIN
}

pub fn drag_frame(start: Frame, handle: Handle, pointer: FPoint, proportional: bool) -> Frame {
    let mut frame = start;
    match handle.sides().x {
        -1 => frame.x0 = pointer.x,
        1 => frame.x1 = pointer.x,
        _ => {}
    }
    match handle.sides().y {
        -1 => frame.y0 = pointer.y,
        1 => frame.y1 = pointer.y,
        _ => {}
    }
    if !proportional || start.width() <= 0.0 || start.height() <= 0.0 {
        return frame;
    }
    let (scale_x, scale_y) = (frame.width().abs() / start.width(), frame.height().abs() / start.height());
    if handle.is_corner() {
        let scale = scale_x.max(scale_y);
        let (width, height) = (start.width() * scale, start.height() * scale);
        let (anchor_x, anchor_y) = (if handle.sides().x < 0 { start.x1 } else { start.x0 }, if handle.sides().y < 0 { start.y1 } else { start.y0 });
        let (sign_x, sign_y) = (if handle.sides().x < 0 { -1.0 } else { 1.0 }, if handle.sides().y < 0 { -1.0 } else { 1.0 });
        let flip_x = if (pointer.x - anchor_x) * sign_x < 0.0 { -1.0 } else { 1.0 };
        let flip_y = if (pointer.y - anchor_y) * sign_y < 0.0 { -1.0 } else { 1.0 };
        return Frame {
            x0: anchor_x.min(anchor_x + sign_x * flip_x * width),
            x1: anchor_x.max(anchor_x + sign_x * flip_x * width),
            y0: anchor_y.min(anchor_y + sign_y * flip_y * height),
            y1: anchor_y.max(anchor_y + sign_y * flip_y * height),
        };
    }
    if handle.sides().x != 0 {
        let (center, height) = ((start.y0 + start.y1) / 2.0, start.height() * scale_x);
        frame.y0 = center - height / 2.0;
        frame.y1 = center + height / 2.0;
    } else {
        let (center, width) = ((start.x0 + start.x1) / 2.0, start.width() * scale_y);
        frame.x0 = center - width / 2.0;
        frame.x1 = center + width / 2.0;
    }
    frame
}

fn ring(center: Pos2, radius: f32) -> Vec<Pos2> {
    (0..=RING_SEGMENTS)
        .map(|step| {
            let angle = step as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            center + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

pub const TOUCH_HANDLE_REACH: f32 = 30.0;

pub fn near_handle(center: Pos2, pointer: Pos2) -> bool {
    near_handle_within(center, pointer, HANDLE_REACH)
}

pub fn near_handle_within(center: Pos2, pointer: Pos2, reach: f32) -> bool {
    center.distance(pointer) <= reach
}

pub fn add_handle_ring(ants: &mut Ants, center: Pos2, hot: bool) {
    ants.solid(&ring(center, HANDLE_RADIUS));
    if hot {
        ants.solid(&ring(center, HANDLE_RADIUS + HANDLE_HOT_EXTRA_RADIUS));
    }
}

pub fn add_pivot(ants: &mut Ants, center: Pos2, hot: bool) {
    ants.solid(&ring(center, PIVOT_RADIUS));
    if hot {
        ants.solid(&ring(center, PIVOT_RADIUS + HANDLE_HOT_EXTRA_RADIUS));
    }
    ants.solid(&[center - vec2(PIVOT_CROSS_HALF, 0.0), center + vec2(PIVOT_CROSS_HALF, 0.0)]);
    ants.solid(&[center - vec2(0.0, PIVOT_CROSS_HALF), center + vec2(0.0, PIVOT_CROSS_HALF)]);
}

pub fn paint_move_icon(painter: &Painter, skin: &Skin, center: Pos2, hot: bool) {
    let theme = skin::theme();
    let rect = Rect::from_center_size(center, vec2(MOVE_ICON_SIDE, MOVE_ICON_SIDE));
    let fill = if hot { theme.button_hover } else { theme.button };
    skin.paint_plate(painter, rect, &Plate::button(fill, Bevel::Raised));
    let icon = Rect::from_center_size(center, vec2(crate::icons::icon_side(), crate::icons::icon_side()));
    painter.image(skin.icons.texture_id(crate::icons::Icon::Move, icon.width()), icon, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
}

pub fn paint_rotate_cursor(painter: &Painter, skin: &Skin, pointer: Pos2) {
    let rect = Rect::from_center_size(pointer, vec2(ROTATE_ICON_SIDE, ROTATE_ICON_SIDE));
    painter.image(skin.icons.texture_id(crate::icons::Icon::Refresh, rect.width()), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
}

pub fn add_box_outline(ants: &mut Ants, bx: &ScreenBox) {
    ants.polyline(&[bx.point(0.0, 0.0), bx.point(1.0, 0.0), bx.point(1.0, 1.0), bx.point(0.0, 1.0), bx.point(0.0, 0.0)]);
}

fn angle_from(pivot: FPoint, p: FPoint) -> f32 {
    (p.y - pivot.y).atan2(p.x - pivot.x)
}

const RULER_STEP: f32 = std::f32::consts::FRAC_PI_4;
const SLOW_ROTATION_FACTOR: f32 = 0.25;

#[derive(Clone, Copy, Debug)]
pub struct RotationDrag {
    pub pivot: FPoint,
    start_angle: f32,
    last: f32,
    applied: f32,
}

impl RotationDrag {
    pub fn new(start_angle: f32, pivot: FPoint, pointer: FPoint) -> Self {
        Self { pivot, start_angle, last: angle_from(pivot, pointer), applied: 0.0 }
    }

    pub fn target(&mut self, pointer: FPoint, mods: Modifiers) -> f32 {
        let angle = angle_from(self.pivot, pointer);
        let step = (angle - self.last + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        self.applied += if mods.ctrl { step * SLOW_ROTATION_FACTOR } else { step };
        self.last = angle;
        let target = self.start_angle + self.applied;
        if mods.shift { (target / RULER_STEP).round() * RULER_STEP } else { target }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(x0: f32, y0: f32, x1: f32, y1: f32) -> Frame {
        Frame { x0, y0, x1, y1 }
    }

    fn screen(x0: f32, y0: f32, x1: f32, y1: f32) -> ScreenBox {
        ScreenBox::from_rect(Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1)))
    }

    #[test]
    fn eight_handles_sit_on_the_corners_and_edge_middles() {
        let bx = screen(10.0, 20.0, 110.0, 80.0);
        assert_eq!(bx.handle_position(Handle::NorthWest), Pos2::new(10.0, 20.0));
        assert_eq!(bx.handle_position(Handle::East), Pos2::new(110.0, 50.0));
        assert_eq!(bx.handle_position(Handle::South), Pos2::new(60.0, 80.0));
    }

    #[test]
    fn hit_zone_is_wider_than_the_knob() {
        let bx = screen(10.0, 20.0, 110.0, 80.0);
        let just_outside_the_circle = Pos2::new(10.0 - HANDLE_RADIUS - 8.0, 20.0);
        assert_eq!(bx.hit_handle(just_outside_the_circle), Some(Handle::NorthWest));
        assert_eq!(bx.hit_handle(Pos2::new(10.0 - HANDLE_REACH - 1.0, 20.0)), None);
        assert_eq!(bx.hit_handle(Pos2::new(60.0, 50.0)), None);
    }

    #[test]
    fn overlapping_handles_of_a_tiny_frame_prefer_the_nearest_then_a_corner() {
        let bx = screen(10.0, 10.0, 14.0, 14.0);
        assert_eq!(bx.hit_handle(Pos2::new(10.0, 10.0)), Some(Handle::NorthWest));
        assert_eq!(bx.hit_handle(Pos2::new(14.0, 14.0)), Some(Handle::SouthEast));
    }

    #[test]
    fn rotate_zone_starts_beyond_the_handles_and_ends_at_its_width() {
        let bx = screen(100.0, 100.0, 200.0, 200.0);
        assert!(!in_rotate_band(bx.distance_outside(Pos2::new(95.0, 150.0))));
        assert!(in_rotate_band(bx.distance_outside(Pos2::new(80.0, 150.0))));
        assert!(!in_rotate_band(bx.distance_outside(Pos2::new(100.0 - ROTATE_ZONE_WIDTH - 1.0, 150.0))));
        assert!(!in_rotate_band(bx.distance_outside(Pos2::new(150.0, 150.0))));
    }

    #[test]
    fn dragging_an_edge_handle_moves_only_that_side() {
        let start = frame(10.0, 10.0, 50.0, 30.0);
        let moved = drag_frame(start, Handle::East, FPoint::new(80.0, 99.0), false);
        assert_eq!(moved, frame(10.0, 10.0, 80.0, 30.0));
        let moved = drag_frame(start, Handle::North, FPoint::new(99.0, 0.0), false);
        assert_eq!(moved, frame(10.0, 0.0, 50.0, 30.0));
    }

    #[test]
    fn dragging_a_corner_moves_both_sides_and_keeps_the_opposite_corner() {
        let moved = drag_frame(frame(10.0, 10.0, 50.0, 30.0), Handle::NorthWest, FPoint::new(0.0, 5.0), false);
        assert_eq!(moved, frame(0.0, 5.0, 50.0, 30.0));
    }

    #[test]
    fn shift_keeps_proportions_at_a_corner() {
        let start = frame(0.0, 0.0, 40.0, 20.0);
        let moved = drag_frame(start, Handle::SouthEast, FPoint::new(80.0, 25.0), true);
        assert_eq!((moved.width(), moved.height()), (80.0, 40.0));
        assert_eq!((moved.x0, moved.y0), (0.0, 0.0));
    }

    #[test]
    fn shift_on_an_edge_grows_the_other_axis_from_its_middle() {
        let start = frame(0.0, 0.0, 40.0, 20.0);
        let moved = drag_frame(start, Handle::East, FPoint::new(80.0, 0.0), true);
        assert_eq!((moved.width(), moved.height()), (80.0, 40.0));
        assert_eq!(((moved.y0 + moved.y1) / 2.0, moved.x0), (10.0, 0.0));
    }
}
