use std::f32::consts::TAU;

use serde::{Deserialize, Serialize};

use super::{MAX_TEXTURE_SIDE, PITCH_RANGE, Texture};

pub const MAX_MARKS: usize = 8;
pub const MARK_START_RANGE: (f32, f32) = (0.0, 20.0);
pub const MARK_LENGTH_RANGE: (f32, f32) = (0.01, 20.0);
pub const MARK_OFFSET_RANGE: (f32, f32) = (-0.5, 0.5);
pub const MARK_THICKNESS_RANGE: (f32, f32) = (0.01, 1.0);
pub const MARK_UNIT_RANGE: (f32, f32) = (0.0, 1.0);
pub const MARK_SWAY_RANGE: (f32, f32) = (0.0, 0.5);
pub const MARK_SWAY_CYCLES_RANGE: (f32, f32) = (0.0, 16.0);
const ACROSS_TEXELS: u32 = 96;
const HALF: f32 = 0.5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mark {
    pub start: f32,
    pub length: f32,
    pub offset: f32,
    pub thickness: f32,
    pub rounding: f32,
    pub sway: f32,
    pub sway_cycles: f32,
}

impl Default for Mark {
    fn default() -> Self {
        Self { start: 0.0, length: 1.0, offset: 0.0, thickness: 1.0, rounding: 0.0, sway: 0.0, sway_cycles: 1.0 }
    }
}

impl Mark {
    pub fn clamped(self) -> Self {
        Self {
            start: self.start.clamp(MARK_START_RANGE.0, MARK_START_RANGE.1),
            length: self.length.clamp(MARK_LENGTH_RANGE.0, MARK_LENGTH_RANGE.1),
            offset: self.offset.clamp(MARK_OFFSET_RANGE.0, MARK_OFFSET_RANGE.1),
            thickness: self.thickness.clamp(MARK_THICKNESS_RANGE.0, MARK_THICKNESS_RANGE.1),
            rounding: self.rounding.clamp(MARK_UNIT_RANGE.0, MARK_UNIT_RANGE.1),
            sway: self.sway.clamp(MARK_SWAY_RANGE.0, MARK_SWAY_RANGE.1),
            sway_cycles: self.sway_cycles.clamp(MARK_SWAY_CYCLES_RANGE.0, MARK_SWAY_CYCLES_RANGE.1),
        }
    }

    fn inside(&self, along: f32, across: f32) -> f32 {
        let phase = TAU * self.sway_cycles * (along - self.start) / self.length;
        let centre = self.offset + self.sway * phase.sin();
        let slope = self.sway * TAU * self.sway_cycles / self.length * phase.cos();
        let (half_length, half_thickness) = (self.length * HALF, self.thickness * HALF);
        let radius = self.rounding * half_length.min(half_thickness);
        let from_middle = (along - self.start - half_length).abs() - (half_length - radius);
        let beside = (across - centre).abs() / slope.hypot(1.0) - (half_thickness - radius);
        radius - from_middle.max(0.0).hypot(beside.max(0.0)) - from_middle.max(beside).min(0.0)
    }
}

pub fn render_marks(pitch: f32, marks: &[Mark]) -> Texture {
    let pitch = pitch.clamp(PITCH_RANGE.0, PITCH_RANGE.1);
    let rows = ((pitch * ACROSS_TEXELS as f32).round() as u32).clamp(1, MAX_TEXTURE_SIDE);
    let pixels = (0..rows * ACROSS_TEXELS)
        .map(|index| {
            let (column, row) = (index % ACROSS_TEXELS, index / ACROSS_TEXELS);
            let across = (column as f32 + HALF) / ACROSS_TEXELS as f32 - HALF;
            let along = (1.0 - (row as f32 + HALF) / rows as f32) * pitch;
            let inside = marks.iter().map(|mark| mark.inside(along, across)).fold(f32::MIN, f32::max);
            ((inside * ACROSS_TEXELS as f32 + HALF).clamp(0.0, 1.0) * f32::from(u8::MAX)).round() as u8
        })
        .collect();
    Texture::new(ACROSS_TEXELS, rows, pixels).expect("mark texture side is within texture limits")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(texture: &Texture, across: f32, along_from_top: f32) -> bool {
        texture.profile(across, along_from_top) > 0.5
    }

    #[test]
    fn a_dash_covers_its_own_stretch_of_the_period_and_leaves_the_gap_empty() {
        let dash = Mark { start: 0.0, length: 2.0, ..Mark::default() };
        let texture = render_marks(3.0, &[dash]);
        assert_eq!((texture.width(), texture.height()), (ACROSS_TEXELS, 3 * ACROSS_TEXELS));
        assert!(ink(&texture, 0.5, 1.0 - 1.0 / 3.0));
        assert!(!ink(&texture, 0.5, 1.0 - 2.5 / 3.0));
    }

    #[test]
    fn fully_rounded_square_mark_is_a_disc_with_empty_corners() {
        let dot = Mark { length: 1.0, thickness: 1.0, rounding: 1.0, ..Mark::default() };
        let texture = render_marks(1.0, &[dot]);
        assert!(ink(&texture, 0.5, 0.5));
        assert!(!ink(&texture, 0.02, 0.02));
    }

    #[test]
    fn sway_moves_the_mark_across_the_stroke_and_one_cycle_closes_the_period() {
        let wave = Mark { length: 4.0, thickness: 0.2, sway: 0.3, ..Mark::default() };
        let texture = render_marks(4.0, &[wave]);
        assert!(ink(&texture, 0.5 + 0.3, 1.0 - 1.0 / 4.0));
        assert!(ink(&texture, 0.5 - 0.3, 1.0 - 3.0 / 4.0));
        assert!(!ink(&texture, 0.5 - 0.3, 1.0 - 1.0 / 4.0));
        assert!((texture.profile(0.5, 0.001) - texture.profile(0.5, 0.999)).abs() < 0.1);
    }

    #[test]
    fn hostile_marks_are_clamped_into_the_declared_ranges() {
        let mark = Mark { start: -5.0, length: 0.0, offset: 9.0, thickness: 9.0, rounding: 9.0, sway: 9.0, sway_cycles: 99.0 }.clamped();
        assert!(mark.start >= MARK_START_RANGE.0 && mark.length >= MARK_LENGTH_RANGE.0 && mark.offset == MARK_OFFSET_RANGE.1);
        assert!(mark.thickness == MARK_THICKNESS_RANGE.1 && mark.rounding == MARK_UNIT_RANGE.1 && mark.sway == MARK_SWAY_RANGE.1 && mark.sway_cycles == MARK_SWAY_CYCLES_RANGE.1);
    }
}
