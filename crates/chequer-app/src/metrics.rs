use eframe::egui::{Margin, Vec2, vec2};

pub const GAP_TIGHT: f32 = 4.0;
pub const GAP_ROW: f32 = 6.0;
pub const GAP_SECTION: f32 = 8.0;

pub const ITEM_SPACING: Vec2 = vec2(6.0, 4.0);
pub const INLINE_SPACING: Vec2 = vec2(6.0, 0.0);
pub const INLINE_SPACING_WIDE: Vec2 = vec2(8.0, 0.0);
pub const INLINE_SPACING_TIGHT: Vec2 = vec2(2.0, 0.0);
pub const GRID_SPACING_TIGHT: Vec2 = vec2(2.0, 2.0);
pub const FORM_GRID_SPACING: Vec2 = vec2(10.0, 6.0);

pub const AUTO_SIZE: Vec2 = Vec2::ZERO;
pub const PRESSED_OFFSET: Vec2 = vec2(1.0, 1.0);
pub const DEFAULT_WORKSPACE_SIZE: Vec2 = vec2(1280.0, 720.0);

#[derive(Clone, Copy)]
pub struct BarMargin {
    x: i8,
    y: i8,
}

impl BarMargin {
    pub const fn new(x: i8, y: i8) -> Self {
        Self { x, y }
    }

    pub fn frame_margin(self) -> Margin {
        Margin::symmetric(self.x, self.y)
    }

    pub fn grain_inset(self) -> Vec2 {
        vec2(f32::from(self.x), f32::from(self.y))
    }
}

pub const TAB_BAR_MARGIN: BarMargin = BarMargin::new(4, 3);
pub const OPTIONS_BAR_MARGIN: BarMargin = BarMargin::new(6, 2);
pub const STATUS_BAR_MARGIN: BarMargin = BarMargin::new(6, 3);
