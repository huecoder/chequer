use std::f32::consts::TAU;

use chequer_core::brush::{Brush, BrushSettings, BrushTip, Curve, MaskTip, hash_unit};

use super::classic::brush;
use super::texture::{COARSE_OCTAVES, FINE_OCTAVES, edge, fbm, salted, smooth_step, texture, variant_set};

const DRAWING_CATEGORY: &str = "drawing";
const DECOR_CATEGORY: &str = "decor";
const LEAF_NIB_ID: &str = "cal.leaf_nib";
const COMET_ID: &str = "cal.comet";
const BROAD_NIB_ID: &str = "cal.broad_nib";
const BRUSH_PEN_ID: &str = "cal.brush_pen";
const TAPER_OUT_ID: &str = "cal.taper_out";
const SWELL_ID: &str = "cal.swell";
const BUBBLES_ID: &str = "dyn.bubbles";
const DOTS_ID: &str = "dyn.dots";
const LEAVES_ID: &str = "dyn.leaves";
const BEADS_ID: &str = "dyn.beads";

const NIB_VARIANTS: u32 = 6;
const NIB_WOBBLE_FREQUENCY: f32 = 9.0;
const NIB_WOBBLE_DEPTH: f32 = 0.2;
const NIB_SIDE_OFFSET: f32 = 0.5;
const LEAF_NIB_BELLY: f32 = 0.9;
const LEAF_NIB_SALT: u32 = 161;
const COMET_HEAD_CENTER: f32 = 0.35;
const COMET_HEAD_RADIUS: f32 = 0.6;
const COMET_TAIL_CURVE: f32 = 1.4;
const COMET_SALT: u32 = 171;
const BROAD_NIB_EXTENT: f32 = 0.92;
const BROAD_NIB_ROUNDING: f32 = 6.0;
const BROAD_NIB_ANGLE: f32 = 30.0;
const BROAD_NIB_ROUNDNESS: f32 = 0.2;
const BROAD_NIB_SALT: u32 = 181;
const RAGGED_DISC_REACH: f32 = 0.92;
const RAGGED_DISC_ROUGHNESS: f32 = 0.12;
const RAGGED_DISC_FREQUENCY: f32 = 6.0;
const BRUSH_PEN_SALT: u32 = 191;
const TAPER_OUT_SALT: u32 = 201;
const SWELL_SALT: u32 = 211;
const BRUSH_PEN_MIN_SIZE: f32 = 0.08;
const BRUSH_PEN_MID_SIZE: f32 = 0.5;
const TAPER_HAIRLINE: f32 = 0.05;
const SWELL_HAIRLINE: f32 = 0.06;

const BUBBLE_VARIANTS: u32 = 8;
const BUBBLE_OUTER_RADIUS: f32 = 0.93;
const BUBBLE_MIN_WALL: f32 = 0.05;
const BUBBLE_WALL_SPAN: f32 = 0.04;
const BUBBLE_FILM: f32 = 0.1;
const BUBBLE_SHINE_DISTANCE: f32 = 0.55;
const BUBBLE_SHINE_RADIUS: f32 = 0.1;
const BUBBLE_SHINE_STRETCH: f32 = 2.6;
const BUBBLE_SHINE_STRENGTH: f32 = 0.85;
const BUBBLE_SALT: u32 = 221;

const DOT_VARIANTS: u32 = 6;
const DOT_REACH: f32 = 0.9;
const DOT_ROUGHNESS: f32 = 0.08;
const DOT_FREQUENCY: f32 = 7.0;
const DOT_SALT: u32 = 231;

const LEAF_VARIANTS: u32 = 10;
const LEAF_BELLY: f32 = 0.75;
const LEAF_WIDTH: f32 = 0.5;
const LEAF_SKEW: f32 = 0.25;
const LEAF_BEND: f32 = 0.18;
const LEAF_BEND_PIVOT: f32 = 0.3;
const LEAF_RIB_HALF_WIDTH: f32 = 0.025;
const LEAF_VEIN_FREQUENCY: f32 = 5.0;
const LEAF_VEIN_SLANT: f32 = 4.5;
const LEAF_VEIN_SHARPNESS: f32 = 0.18;
const LEAF_BLADE_FLOOR: f32 = 0.82;
const LEAF_MOTTLING_FREQUENCY: f32 = 4.0;
const LEAF_MOTTLING_DEPTH: f32 = 0.25;
const LEAF_SALT: u32 = 241;

pub fn dynamic_brushes() -> Vec<Brush> {
    let base = BrushSettings::default();
    let pen_curve = Curve::new(vec![(0.0, BRUSH_PEN_MIN_SIZE), (0.5, BRUSH_PEN_MID_SIZE), (1.0, 1.0)]);
    vec![
        brush(
            LEAF_NIB_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                spacing: 0.05,
                follow_direction: true,
                roundness: 0.35,
                pressure_size: 0.35,
                pressure_roundness: 0.7,
                velocity_size: 0.3,
                size_start: 0.5,
                size_fade: 2.5,
                ..base.clone()
            },
            variant_set(NIB_VARIANTS, leaf_nib_tip),
        ),
        brush(
            COMET_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                spacing: 0.06,
                follow_direction: true,
                roundness: 0.6,
                pressure_size: 0.5,
                velocity_size: 0.45,
                size_start: 0.6,
                size_fade: 2.0,
                ..base.clone()
            },
            variant_set(NIB_VARIANTS, comet_tip),
        ),
        brush(
            BROAD_NIB_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                spacing: 0.04,
                angle: BROAD_NIB_ANGLE,
                roundness: BROAD_NIB_ROUNDNESS,
                pressure_size: 0.6,
                velocity_size: 0.4,
                size_start: 0.35,
                size_fade: 3.0,
                ..base.clone()
            },
            variant_set(NIB_VARIANTS, broad_nib_tip),
        ),
        brush(
            BRUSH_PEN_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                spacing: 0.05,
                pressure_size: 1.0,
                pressure_size_curve: pen_curve,
                pressure_opacity: 0.25,
                velocity_size: 0.6,
                size_start: 0.45,
                size_fade: 2.2,
                ..base.clone()
            },
            variant_set(NIB_VARIANTS, |variant| ragged_disc_tip(salted(BRUSH_PEN_SALT, variant))),
        ),
        brush(
            TAPER_OUT_ID,
            DRAWING_CATEGORY,
            BrushSettings { spacing: 0.04, pressure_size: 0.3, velocity_size: 0.25, size_start: 1.0, size_end: TAPER_HAIRLINE, size_fade: 12.0, ..base.clone() },
            variant_set(NIB_VARIANTS, |variant| ragged_disc_tip(salted(TAPER_OUT_SALT, variant))),
        ),
        brush(
            SWELL_ID,
            DRAWING_CATEGORY,
            BrushSettings { spacing: 0.04, pressure_size: 0.3, size_start: SWELL_HAIRLINE, size_end: 1.0, size_fade: 8.0, ..base.clone() },
            variant_set(NIB_VARIANTS, |variant| ragged_disc_tip(salted(SWELL_SALT, variant))),
        ),
        brush(
            BUBBLES_ID,
            DECOR_CATEGORY,
            BrushSettings { spacing: 1.05, angle_jitter: 1.0, size_jitter: 0.85, opacity_jitter: 0.2, scatter: 0.9, ..base.clone() },
            variant_set(BUBBLE_VARIANTS, bubble_tip),
        ),
        brush(
            DOTS_ID,
            DECOR_CATEGORY,
            BrushSettings { spacing: 0.75, angle_jitter: 1.0, size_jitter: 0.85, opacity_jitter: 0.25, scatter: 0.5, pressure_size: 0.5, ..base.clone() },
            variant_set(DOT_VARIANTS, dot_tip),
        ),
        brush(
            LEAVES_ID,
            DECOR_CATEGORY,
            BrushSettings { spacing: 0.9, angle_jitter: 1.0, size_jitter: 0.55, opacity_jitter: 0.2, scatter: 0.7, pressure_size: 0.4, value_jitter: 0.15, ..base.clone() },
            variant_set(LEAF_VARIANTS, leaf_tip),
        ),
        brush(
            BEADS_ID,
            DECOR_CATEGORY,
            BrushSettings { spacing: 1.08, hardness: 1.0, pressure_size: 0.3, size_start: 0.08, size_end: 1.0, size_fade: 10.0, ..base },
            BrushTip::Round,
        ),
    ]
}

fn nib_wobble(x: f32, side: f32, salt: u32) -> f32 {
    1.0 + (fbm(x * NIB_WOBBLE_FREQUENCY, NIB_SIDE_OFFSET + side, FINE_OCTAVES, salt) - 0.5) * NIB_WOBBLE_DEPTH
}

fn leaf_nib_tip(variant: u32) -> MaskTip {
    let salt = salted(LEAF_NIB_SALT, variant);
    texture(|x, y| {
        let half_width = (1.0 - x * x).max(0.0).powf(LEAF_NIB_BELLY) * nib_wobble(x, y.signum(), salt);
        edge(half_width - y.abs())
    })
}

fn comet_half_width(x: f32) -> f32 {
    if x >= COMET_HEAD_CENTER {
        return (COMET_HEAD_RADIUS * COMET_HEAD_RADIUS - (x - COMET_HEAD_CENTER).powi(2)).max(0.0).sqrt();
    }
    COMET_HEAD_RADIUS * ((x + 1.0) / (COMET_HEAD_CENTER + 1.0)).clamp(0.0, 1.0).powf(COMET_TAIL_CURVE)
}

fn comet_tip(variant: u32) -> MaskTip {
    let salt = salted(COMET_SALT, variant);
    texture(|x, y| edge(comet_half_width(x) * nib_wobble(x, y.signum(), salt) - y.abs()))
}

fn broad_nib_tip(variant: u32) -> MaskTip {
    let salt = salted(BROAD_NIB_SALT, variant);
    texture(|x, y| {
        let squared = (x.abs().powf(BROAD_NIB_ROUNDING) + y.abs().powf(BROAD_NIB_ROUNDING)).powf(BROAD_NIB_ROUNDING.recip());
        edge(BROAD_NIB_EXTENT * nib_wobble(x, y.signum(), salt) - squared)
    })
}

fn ragged_disc_tip(salt: u32) -> MaskTip {
    texture(|x, y| {
        let roughness = (fbm(x * RAGGED_DISC_FREQUENCY, y * RAGGED_DISC_FREQUENCY, COARSE_OCTAVES, salt) - 0.5) * RAGGED_DISC_ROUGHNESS;
        edge(RAGGED_DISC_REACH + roughness - x.hypot(y))
    })
}

fn bubble_tip(variant: u32) -> MaskTip {
    let salt = salted(BUBBLE_SALT, variant);
    let wall = BUBBLE_MIN_WALL + BUBBLE_WALL_SPAN * hash_unit(1, 1, salt);
    let shine_angle = hash_unit(2, 2, salt) * TAU;
    let (shine_cos, shine_sin) = (shine_angle.cos(), shine_angle.sin());
    texture(|x, y| {
        let radius = x.hypot(y);
        let ring = edge(wall - (radius - (BUBBLE_OUTER_RADIUS - wall)).abs());
        let film = BUBBLE_FILM * edge(BUBBLE_OUTER_RADIUS - radius);
        let (dx, dy) = (x - shine_cos * BUBBLE_SHINE_DISTANCE, y - shine_sin * BUBBLE_SHINE_DISTANCE);
        let along = (dx * -shine_sin + dy * shine_cos) / BUBBLE_SHINE_STRETCH;
        let across = dx * shine_cos + dy * shine_sin;
        let shine = edge(BUBBLE_SHINE_RADIUS - along.hypot(across)) * BUBBLE_SHINE_STRENGTH;
        ring.max(film).max(shine)
    })
}

fn dot_tip(variant: u32) -> MaskTip {
    let salt = salted(DOT_SALT, variant);
    texture(|x, y| {
        let roughness = (fbm(x * DOT_FREQUENCY, y * DOT_FREQUENCY, COARSE_OCTAVES, salt) - 0.5) * DOT_ROUGHNESS;
        edge(DOT_REACH + roughness - x.hypot(y))
    })
}

fn leaf_tip(variant: u32) -> MaskTip {
    let salt = salted(LEAF_SALT, variant);
    texture(|x, y| {
        let axis_y = y - LEAF_BEND * (x * x - LEAF_BEND_PIVOT);
        let half_width = LEAF_WIDTH * (1.0 - x * x).max(0.0).powf(LEAF_BELLY) * (1.0 - LEAF_SKEW * x);
        let blade = edge(half_width - axis_y.abs());
        let vein_phase = (x * LEAF_VEIN_FREQUENCY - axis_y.abs() * LEAF_VEIN_SLANT).rem_euclid(1.0);
        let vein = 1.0 - smooth_step(0.0, LEAF_VEIN_SHARPNESS, (vein_phase - 0.5).abs());
        let rib = edge(LEAF_RIB_HALF_WIDTH - axis_y.abs());
        let mottling = 1.0 - LEAF_MOTTLING_DEPTH * fbm(x * LEAF_MOTTLING_FREQUENCY, y * LEAF_MOTTLING_FREQUENCY, COARSE_OCTAVES, salt);
        let surface = (LEAF_BLADE_FLOOR + (1.0 - LEAF_BLADE_FLOOR) * vein.max(rib)) * mottling;
        blade * surface
    })
}
