use chequer_core::brush::{Brush, BrushSettings, BrushTip, TipSpec, decode_pack, generate_tip, hash_unit, value_noise};

use super::classic::{TIP_SIDE, brush, mask, soft_edge};

const DRAWING_CATEGORY: &str = "drawing";
const PAINTING_CATEGORY: &str = "painting";
const PENCIL_ID: &str = "art.pencil";
const PENCIL_SOFT_ID: &str = "art.pencil_soft";
const CHARCOAL_ID: &str = "art.charcoal";
const PASTEL_ID: &str = "art.pastel";
const INK_PEN_ID: &str = "art.ink_pen";
const FELT_MARKER_ID: &str = "art.felt_marker";
const WATERCOLOR_ID: &str = "art.watercolor";
const OIL_ID: &str = "art.oil";
const FLAT_BRISTLE_ID: &str = "art.flat_bristle";
const DRY_BRUSH_ID: &str = "art.dry_brush";
const SPONGE_ID: &str = "art.sponge";
const CRAYON_ID: &str = "art.crayon";
const STIPPLE_ID: &str = "art.stipple";
const FUR_ID: &str = "art.fur";
const SUMI_ID: &str = "art.sumi";
const FUR_BRISTLES: u32 = 90;
const FUR_BRISTLE_RADIUS: f32 = 0.025;
const FUR_SALT: u32 = 14;
const STIPPLE_SIDE: u32 = 24;
const STIPPLE_DOT_RADIUS: f32 = 0.35;
const STIPPLE_SALT: u32 = 41;
const INK_BLOT_ID: &str = "art.ink_blot";
const INK_BLOT_PRESSURE_SIZE: f32 = 0.2;
const DIP_PEN_ID: &str = "art.dip_pen";
const TECHNICAL_PENCIL_ID: &str = "art.technical_pencil";
const GOUACHE_ID: &str = "art.gouache";
const INK_BLOT_PACK: &[u8] = include_bytes!("../../assets/sources/ink_blot.chqbrush");

const OIL_BRISTLES: u32 = 140;
const FLAT_BRISTLES: u32 = 160;
const DRY_BRISTLES: u32 = 30;
const OIL_BRISTLE_RADIUS: f32 = 0.07;
const DRY_BRISTLE_RADIUS: f32 = 0.05;
const OIL_SALT: u32 = 11;
const FLAT_SALT: u32 = 12;
const DRY_SALT: u32 = 13;
const CHARCOAL_RADIUS: f32 = 0.8;
const CHARCOAL_RAGGEDNESS: f32 = 0.5;
const CHARCOAL_EDGE_FREQUENCY: f32 = 4.0;
const CHARCOAL_BODY_FREQUENCY: f32 = 14.0;
const CHARCOAL_BODY_FLOOR: f32 = 0.45;
const CHARCOAL_EDGE_SALT: u32 = 21;
const CHARCOAL_BODY_SALT: u32 = 22;
const SPONGE_COARSE_FREQUENCY: f32 = 5.0;
const SPONGE_FINE_FREQUENCY: f32 = 13.0;
const SPONGE_COARSE_WEIGHT: f32 = 0.6;
const SPONGE_THRESHOLD: f32 = 0.45;
const SPONGE_CONTRAST: f32 = 8.0;
const SPONGE_REACH: f32 = 0.95;
const SPONGE_COARSE_SALT: u32 = 31;
const SPONGE_FINE_SALT: u32 = 32;
const CHISEL_HALF_EXTENT: f32 = 0.95;
const CHISEL_CORNER: f32 = 0.35;
const CHISEL_ANGLE: f32 = -35.0;
const CHISEL_ROUNDNESS: f32 = 0.4;
const FLAT_BRUSH_ANGLE: f32 = 90.0;

pub fn art_brushes() -> Vec<Brush> {
    let base = BrushSettings::default();
    let pencil = BrushSettings {
        spacing: 0.06,
        hardness: 0.95,
        flow: 0.12,
        accumulate: true,
        pressure_size: 0.3,
        pressure_opacity: 0.8,
        tilt_angle: 1.0,
        tilt_roundness: 0.5,
        grain: 0.6,
        grain_scale: 2.0,
        ..base.clone()
    };
    let flat_bristle = BrushSettings {
        angle: FLAT_BRUSH_ANGLE,
        follow_direction: true,
        pressure_size: 0.3,
        ..base.clone()
    };
    let mut brushes = vec![
        brush(PENCIL_ID, DRAWING_CATEGORY, pencil.clone(), BrushTip::Round),
        brush(
            PENCIL_SOFT_ID,
            DRAWING_CATEGORY,
            BrushSettings { spacing: 0.05, hardness: 0.5, flow: 0.1, pressure_size: 0.5, pressure_opacity: 1.0, grain: 0.45, grain_scale: 2.5, tilt_roundness: 0.6, ..pencil },
            BrushTip::Round,
        ),
        brush(
            CHARCOAL_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                spacing: 0.08,
                flow: 0.22,
                accumulate: true,
                angle_jitter: 1.0,
                opacity_jitter: 0.25,
                pressure_size: 0.4,
                pressure_opacity: 0.9,
                grain: 0.8,
                grain_scale: 3.5,
                ..base.clone()
            },
            mask(TIP_SIDE, charcoal_texel),
        ),
        brush(
            PASTEL_ID,
            DRAWING_CATEGORY,
            BrushSettings { spacing: 0.05, hardness: 0.55, flow: 0.85, pressure_size: 0.2, pressure_opacity: 0.6, grain: 0.7, grain_scale: 4.0, ..base.clone() },
            BrushTip::Round,
        ),
        brush(
            INK_PEN_ID,
            DRAWING_CATEGORY,
            BrushSettings { spacing: 0.04, hardness: 1.0, velocity_size: 0.35, size_start: 0.35, size_fade: 3.0, ..base.clone() },
            BrushTip::Round,
        ),
        brush(
            FELT_MARKER_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                spacing: 0.06,
                angle: CHISEL_ANGLE,
                roundness: CHISEL_ROUNDNESS,
                flow: 0.75,
                pressure_size: 0.0,
                grain: 0.12,
                grain_scale: 1.5,
                ..base.clone()
            },
            mask(TIP_SIDE, chisel_texel),
        ),
        brush(
            WATERCOLOR_ID,
            PAINTING_CATEGORY,
            BrushSettings {
                spacing: 0.06,
                hardness: 0.15,
                flow: 0.55,
                opacity_jitter: 0.2,
                pressure_size: 0.5,
                pressure_opacity: 0.7,
                grain: 0.3,
                grain_scale: 6.0,
                ..base.clone()
            },
            BrushTip::Round,
        ),
        brush(
            OIL_ID,
            PAINTING_CATEGORY,
            BrushSettings { spacing: 0.05, flow: 0.9, pressure_size: 0.5, pressure_opacity: 0.3, grain: 0.15, grain_scale: 4.0, ..base.clone() },
            bristle_tip(OIL_BRISTLES, OIL_BRISTLE_RADIUS, OIL_SALT),
        ),
        brush(
            FLAT_BRISTLE_ID,
            PAINTING_CATEGORY,
            BrushSettings { spacing: 0.03, roundness: 0.35, flow: 0.9, pressure_opacity: 0.2, grain: 0.2, grain_scale: 4.0, ..flat_bristle.clone() },
            bristle_tip(FLAT_BRISTLES, OIL_BRISTLE_RADIUS, FLAT_SALT),
        ),
        brush(
            DRY_BRUSH_ID,
            PAINTING_CATEGORY,
            BrushSettings { spacing: 0.03, roundness: 0.6, flow: 0.85, pressure_opacity: 0.8, grain: 1.0, grain_scale: 2.5, ..flat_bristle },
            bristle_tip(DRY_BRISTLES, DRY_BRISTLE_RADIUS, DRY_SALT),
        ),
        brush(
            SPONGE_ID,
            PAINTING_CATEGORY,
            BrushSettings {
                spacing: 0.3,
                flow: 0.5,
                accumulate: true,
                angle_jitter: 1.0,
                size_jitter: 0.2,
                scatter: 0.1,
                grain: 0.3,
                grain_scale: 3.0,
                ..base.clone()
            },
            mask(TIP_SIDE, sponge_texel),
        ),
        brush(
            DIP_PEN_ID,
            DRAWING_CATEGORY,
            BrushSettings { spacing: 0.03, hardness: 1.0, pressure_size: 1.0, velocity_size: 0.25, size_start: 0.3, size_fade: 4.0, ..base.clone() },
            BrushTip::Round,
        ),
        brush(
            TECHNICAL_PENCIL_ID,
            DRAWING_CATEGORY,
            BrushSettings { spacing: 0.05, hardness: 1.0, flow: 0.55, accumulate: true, pressure_size: 0.1, pressure_opacity: 0.7, grain: 0.35, grain_scale: 1.5, ..base.clone() },
            BrushTip::Round,
        ),
        brush(
            GOUACHE_ID,
            PAINTING_CATEGORY,
            BrushSettings { spacing: 0.06, hardness: 0.9, flow: 0.95, opacity_jitter: 0.05, pressure_size: 0.4, grain: 0.12, grain_scale: 3.0, ..base.clone() },
            BrushTip::Round,
        ),
        brush(
            CRAYON_ID,
            DRAWING_CATEGORY,
            BrushSettings { spacing: 0.06, hardness: 0.8, flow: 0.8, accumulate: true, pressure_size: 0.2, pressure_opacity: 0.6, grain: 0.9, grain_scale: 2.0, ..base.clone() },
            BrushTip::Round,
        ),
        brush(
            SUMI_ID,
            DRAWING_CATEGORY,
            BrushSettings { spacing: 0.03, hardness: 0.7, flow: 0.9, pressure_size: 1.0, velocity_size: 0.4, size_start: 0.2, size_fade: 5.0, grain: 0.3, grain_scale: 2.0, ..base.clone() },
            BrushTip::Round,
        ),
        brush(
            STIPPLE_ID,
            DRAWING_CATEGORY,
            BrushSettings { spacing: 0.4, scatter: 0.8, size_jitter: 0.4, opacity_jitter: 0.3, pressure_size: 0.5, ..base.clone() },
            mask(STIPPLE_SIDE, stipple_texel),
        ),
        brush(
            FUR_ID,
            PAINTING_CATEGORY,
            BrushSettings { spacing: 0.04, roundness: 0.6, angle: FLAT_BRUSH_ANGLE, follow_direction: true, angle_jitter: 0.05, flow: 0.8, pressure_size: 0.4, pressure_opacity: 0.4, ..base },
            bristle_tip(FUR_BRISTLES, FUR_BRISTLE_RADIUS, FUR_SALT),
        ),
    ];
    brushes.extend(ink_blot());
    brushes
}

fn ink_blot() -> Option<Brush> {
    let packed = decode_pack(INK_BLOT_PACK).ok()?.into_iter().next()?;
    let settings = BrushSettings { pressure_size: INK_BLOT_PRESSURE_SIZE, ..packed.settings };
    Some(brush(INK_BLOT_ID, DRAWING_CATEGORY, settings, packed.tip))
}

pub fn bristle_tip(count: u32, min_radius: f32, salt: u32) -> BrushTip {
    let spec = TipSpec { generator: "bristles".into(), params: [("count".to_owned(), count as f32), ("radius".to_owned(), min_radius)].into(), seed: salt };
    generate_tip(&spec, TIP_SIDE).map_or(BrushTip::Round, BrushTip::Mask)
}

fn charcoal_texel(x: f32, y: f32) -> f32 {
    let ragged = CHARCOAL_RAGGEDNESS * (value_noise(x * CHARCOAL_EDGE_FREQUENCY, y * CHARCOAL_EDGE_FREQUENCY, CHARCOAL_EDGE_SALT) - 0.5);
    let body = soft_edge(CHARCOAL_RADIUS + ragged - x.hypot(y));
    let crumble = value_noise(x * CHARCOAL_BODY_FREQUENCY, y * CHARCOAL_BODY_FREQUENCY, CHARCOAL_BODY_SALT);
    body * (CHARCOAL_BODY_FLOOR + (1.0 - CHARCOAL_BODY_FLOOR) * crumble)
}

fn stipple_texel(x: f32, y: f32) -> f32 {
    let jitter = |salt: i32| hash_unit(salt, STIPPLE_SALT as i32, STIPPLE_SALT) - 0.5;
    soft_edge(STIPPLE_DOT_RADIUS - (x - jitter(1) * 0.3).hypot(y - jitter(2) * 0.3))
}

fn sponge_texel(x: f32, y: f32) -> f32 {
    let coarse = value_noise(x * SPONGE_COARSE_FREQUENCY, y * SPONGE_COARSE_FREQUENCY, SPONGE_COARSE_SALT);
    let fine = value_noise(x * SPONGE_FINE_FREQUENCY, y * SPONGE_FINE_FREQUENCY, SPONGE_FINE_SALT);
    let density = coarse * SPONGE_COARSE_WEIGHT + fine * (1.0 - SPONGE_COARSE_WEIGHT);
    ((density - SPONGE_THRESHOLD) * SPONGE_CONTRAST).clamp(0.0, 1.0) * soft_edge(SPONGE_REACH - x.hypot(y))
}

fn chisel_texel(x: f32, y: f32) -> f32 {
    let inner = CHISEL_HALF_EXTENT - CHISEL_CORNER;
    let (beyond_x, beyond_y) = (x.abs() - inner, y.abs() - inner);
    let distance = beyond_x.max(0.0).hypot(beyond_y.max(0.0)) + beyond_x.max(beyond_y).min(0.0) - CHISEL_CORNER;
    soft_edge(-distance)
}
