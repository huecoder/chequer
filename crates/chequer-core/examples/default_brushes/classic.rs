use chequer_core::brush::{Brush, BrushMeta, BrushSettings, BrushTip, MaskTip, hash_unit};

const BASIC_CATEGORY: &str = "basic";
const PAINT_CATEGORY: &str = "paint";
const SOFT_CATEGORY: &str = "soft";
const TEXTURE_CATEGORY: &str = "texture";
const DECOR_CATEGORY: &str = "decor";
const ROUND_ID: &str = "builtin.round";
const PAINT_BRUSH_ID: &str = "paint.brush";
const PAINT_CALLIGRAPHY_FORWARD_ID: &str = "paint.calligraphy_forward";
const PAINT_CALLIGRAPHY_BACKWARD_ID: &str = "paint.calligraphy_backward";
const PAINT_SPRAY_ID: &str = "paint.spray";
const PAINT_MARKER_ID: &str = "paint.marker";
const AIRBRUSH_ID: &str = "builtin.airbrush";
const CHALK_ID: &str = "builtin.chalk";
const SPLATTER_ID: &str = "builtin.splatter";
const STAR_ID: &str = "builtin.star";
const SQUARE_ID: &str = "builtin.square";

pub const TIP_SIDE: u32 = 64;
const SQUARE_SIDE: u32 = 16;
const STAR_POINTS: f32 = 5.0;
const STAR_DEPTH: f32 = 0.45;
const SPLATTER_DOTS: u32 = 16;
const SPRAY_SIDE: u32 = 32;
const SPRAY_DENSITY: f32 = 0.22;
const CALLIGRAPHY_ROUNDNESS: f32 = 0.1;
const CALLIGRAPHY_SLANT: f32 = 45.0;
const MARKER_FLOW: f32 = 0.5;
const SOLID_HARDNESS: f32 = 1.0;
const GRAIN_THRESHOLD: f32 = 0.35;
const EDGE_SOFTNESS_TEXELS: f32 = 1.5;

pub fn classic_brushes() -> Vec<Brush> {
    let solid = BrushSettings { hardness: SOLID_HARDNESS, ..BrushSettings::default() };
    let calligraphy = BrushSettings { spacing: 0.05, roundness: CALLIGRAPHY_ROUNDNESS, ..solid.clone() };
    vec![
        brush(PAINT_BRUSH_ID, PAINT_CATEGORY, solid.clone(), BrushTip::Round),
        brush(PAINT_CALLIGRAPHY_FORWARD_ID, PAINT_CATEGORY, BrushSettings { angle: -CALLIGRAPHY_SLANT, ..calligraphy.clone() }, BrushTip::Round),
        brush(PAINT_CALLIGRAPHY_BACKWARD_ID, PAINT_CATEGORY, BrushSettings { angle: CALLIGRAPHY_SLANT, ..calligraphy }, BrushTip::Round),
        brush(PAINT_SPRAY_ID, PAINT_CATEGORY, BrushSettings { spacing: 0.15, angle_jitter: 1.0, ..solid.clone() }, mask(SPRAY_SIDE, spray_texel)),
        brush(PAINT_MARKER_ID, PAINT_CATEGORY, BrushSettings { flow: MARKER_FLOW, ..solid }, BrushTip::Round),
        brush(ROUND_ID, BASIC_CATEGORY, BrushSettings::default(), BrushTip::Round),
        brush(AIRBRUSH_ID, SOFT_CATEGORY, BrushSettings { spacing: 0.04, flow: 0.07, accumulate: true, ..BrushSettings::default() }, BrushTip::Round),
        brush(SQUARE_ID, BASIC_CATEGORY, BrushSettings { spacing: 0.1, follow_direction: true, ..BrushSettings::default() }, mask(SQUARE_SIDE, |_, _| 1.0)),
        brush(
            CHALK_ID,
            TEXTURE_CATEGORY,
            BrushSettings { spacing: 0.12, flow: 0.45, accumulate: true, angle_jitter: 1.0, opacity_jitter: 0.3, ..BrushSettings::default() },
            mask(TIP_SIDE, chalk_texel),
        ),
        brush(
            SPLATTER_ID,
            DECOR_CATEGORY,
            BrushSettings { spacing: 0.7, angle_jitter: 1.0, size_jitter: 0.6, scatter: 0.7, ..BrushSettings::default() },
            mask(TIP_SIDE, splatter_texel),
        ),
        brush(STAR_ID, DECOR_CATEGORY, BrushSettings { spacing: 0.9, angle_jitter: 1.0, size_jitter: 0.4, scatter: 0.3, ..BrushSettings::default() }, mask(TIP_SIDE, star_texel)),
    ]
}

pub fn brush(id: &str, category: &str, settings: BrushSettings, tip: BrushTip) -> Brush {
    Brush { meta: BrushMeta { id: id.into(), category: category.into(), ..BrushMeta::default() }, settings, tip, textures: Default::default() }
}

pub fn mask(side: u32, coverage: impl Fn(f32, f32) -> f32) -> BrushTip {
    let center = side as f32 / 2.0;
    let pixels = (0..side * side)
        .map(|index| {
            let (x, y) = ((index % side) as f32 + 0.5 - center, (index / side) as f32 + 0.5 - center);
            (coverage(x / center, y / center).clamp(0.0, 1.0) * u8::MAX as f32).round() as u8
        })
        .collect();
    match MaskTip::new(side, side, pixels) {
        Ok(tip) => BrushTip::Mask(tip),
        Err(_) => BrushTip::Round,
    }
}

pub fn soft_edge(inside: f32) -> f32 {
    (inside * TIP_SIDE as f32 / (2.0 * EDGE_SOFTNESS_TEXELS)).clamp(0.0, 1.0)
}

fn chalk_texel(x: f32, y: f32) -> f32 {
    let edge = soft_edge(1.0 - x.hypot(y));
    let grain = hash_unit((x * 1000.0) as i32, (y * 1000.0) as i32, 1);
    if grain < GRAIN_THRESHOLD { 0.0 } else { edge * (0.6 + 0.4 * grain) }
}

fn splatter_texel(x: f32, y: f32) -> f32 {
    (0..SPLATTER_DOTS)
        .map(|dot| {
            let angle = hash_unit(dot as i32, 3, 2) * std::f32::consts::TAU;
            let reach = hash_unit(dot as i32, 5, 2).sqrt() * 0.8;
            let radius = 0.05 + 0.2 * hash_unit(dot as i32, 7, 2) * (1.0 - reach);
            let distance = (x - angle.cos() * reach).hypot(y - angle.sin() * reach);
            soft_edge(radius - distance)
        })
        .fold(0.0, f32::max)
}

fn spray_texel(x: f32, y: f32) -> f32 {
    let (column, row) = ((x * SPRAY_SIDE as f32).floor() as i32, (y * SPRAY_SIDE as f32).floor() as i32);
    if x.hypot(y) < 1.0 && hash_unit(column, row, 4) < SPRAY_DENSITY { 1.0 } else { 0.0 }
}

fn star_texel(x: f32, y: f32) -> f32 {
    let angle = y.atan2(x);
    let spike = (0.5 + 0.5 * (angle * STAR_POINTS).cos()).powf(1.5);
    let radius = 0.95 * (STAR_DEPTH + (1.0 - STAR_DEPTH) * spike);
    soft_edge(radius - x.hypot(y))
}
