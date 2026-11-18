use std::f32::consts::{FRAC_PI_4, TAU};

use chequer_core::brush::{Brush, BrushSettings, BrushTip, Mark};

use super::classic::brush;
use super::dashes::{LINES_CATEGORY, marked_line};
use super::shapes::{rotated, unit_edge, unit_mask};

const RUNNING_STITCH_ID: &str = "stitch.running";
const CROSS_STITCH_ID: &str = "stitch.cross";
const WAVE_ID: &str = "line.wave";

const STITCH_PITCH: f32 = 9.5;
const STITCH_THICKNESS: f32 = 0.6;
const STITCHES: [(f32, f32, f32); 2] = [(0.5, 3.6, 0.02), (5.0, 3.9, -0.03)];

const WAVE_PITCH: f32 = 6.0;
const WAVE_THICKNESS: f32 = 0.3;
const WAVE_SWAY: f32 = 0.35;

const CROSS_ARM_LENGTH: f32 = 0.82;
const CROSS_ARM_HALF_WIDTH: f32 = 0.17;
const CROSS_TWIST_PITCH: f32 = 0.2;
const CROSS_TWIST_DEPTH: f32 = 0.2;
const CROSS_TWIST_LEAN: f32 = 0.6;
const CROSS_UNDER_ALPHA: f32 = 0.78;

pub fn seam_brushes() -> Vec<Brush> {
    let stitches = STITCHES.iter().map(|&(start, length, offset)| Mark { start, length, offset, thickness: STITCH_THICKNESS, rounding: 1.0, ..Mark::default() }).collect();
    let wave = Mark { length: WAVE_PITCH, thickness: WAVE_THICKNESS, sway: WAVE_SWAY, ..Mark::default() };
    vec![
        marked_line(RUNNING_STITCH_ID, STITCH_PITCH, stitches),
        brush(CROSS_STITCH_ID, LINES_CATEGORY, BrushSettings { spacing: 1.0, hardness: 1.0, pressure_size: 0.0, ..BrushSettings::default() }, cross_stitch()),
        marked_line(WAVE_ID, WAVE_PITCH, vec![wave]),
    ]
}

fn capsule_inside(x: f32, y: f32, half_length: f32, radius: f32) -> f32 {
    let beyond = (x.abs() - half_length).max(0.0);
    radius - beyond.hypot(y)
}

fn cross_stitch() -> BrushTip {
    BrushTip::Mask(unit_mask(|x, y| {
        let arm = |angle: f32| {
            let (along, across) = rotated(x, y, angle);
            let inside = capsule_inside(along, across, CROSS_ARM_LENGTH - CROSS_ARM_HALF_WIDTH, CROSS_ARM_HALF_WIDTH);
            let twist = ((along / CROSS_TWIST_PITCH + across / CROSS_TWIST_PITCH * CROSS_TWIST_LEAN) * TAU).sin() * 0.5 + 0.5;
            (unit_edge(inside), twist)
        };
        let (under, under_twist) = arm(FRAC_PI_4);
        let (over, over_twist) = arm(-FRAC_PI_4);
        let under_alpha = under * CROSS_UNDER_ALPHA * (1.0 - CROSS_TWIST_DEPTH * under_twist);
        let over_alpha = over * (1.0 - CROSS_TWIST_DEPTH * over_twist);
        over_alpha.max(under_alpha)
    }))
}
