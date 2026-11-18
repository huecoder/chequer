use std::f32::consts::TAU;

use chequer_core::brush::{Brush, BrushSettings, MaskTip, hash_unit};

use super::classic::brush;
use super::texture::{COARSE_OCTAVES, DETAIL_OCTAVES, FINE_OCTAVES, edge, fbm, ragged_reach, salted, smooth_step, texture, variant_set, warped_fbm, worley};

const DRAWING_CATEGORY: &str = "drawing";
const PAINTING_CATEGORY: &str = "painting";
const DECOR_CATEGORY: &str = "decor";
const ROUGH_INK_ID: &str = "nat.rough_ink";
const DRY_STREAK_ID: &str = "nat.dry_streak";
const CHARCOAL_SIDE_ID: &str = "nat.charcoal_side";
const PASTEL_SOFT_ID: &str = "nat.pastel_soft";
const WASH_ID: &str = "nat.wash";
const INK_SPLASH_ID: &str = "nat.ink_splash";
const SEA_SPONGE_ID: &str = "nat.sea_sponge";
const SMOKE_ID: &str = "nat.smoke";
const GRUNGE_ID: &str = "nat.grunge";
const GRASS_ID: &str = "nat.grass";
const CRACKS_ID: &str = "nat.cracks";

const INK_VARIANTS: u32 = 12;
const INK_RADIUS: f32 = 0.78;
const INK_RAGGEDNESS: f32 = 0.7;
const INK_WARP: f32 = 1.8;
const INK_FREQUENCY: f32 = 2.4;
const INK_SPECK_FREQUENCY: f32 = 11.0;
const INK_SPECK_FLOOR: f32 = 0.6;
const INK_SPECK_SHARPNESS: f32 = 0.03;
const INK_SPECK_REACH: f32 = 0.96;
const INK_SALT: u32 = 51;

const STREAK_VARIANTS: u32 = 8;
const STREAK_LANES: f32 = 18.0;
const STREAK_MIN_REACH: f32 = 0.45;
const STREAK_FRAYING: f32 = 0.2;
const STREAK_BREAKUP_FREQUENCY: f32 = 3.5;
const STREAK_LANE_STRIDE: f32 = 17.3;
const STREAK_BREAKUP_WEIGHT: f32 = 0.55;
const STREAK_LOAD_WEIGHT: f32 = 0.65;
const STREAK_DENSITY_FROM: f32 = 0.3;
const STREAK_DENSITY_TO: f32 = 0.5;
const STREAK_ENVELOPE: f32 = 0.88;
const STREAK_SALT: u32 = 61;

const CHARCOAL_SIDE_VARIANTS: u32 = 8;
const CHARCOAL_SIDE_EXTENT: f32 = 0.9;
const CHARCOAL_SIDE_ROUNDING: f32 = 4.0;
const CHARCOAL_SIDE_RAGGEDNESS: f32 = 0.35;
const CHARCOAL_SIDE_EDGE_FREQUENCY: f32 = 3.0;
const CHARCOAL_SIDE_DRAG_ALONG: f32 = 5.0;
const CHARCOAL_SIDE_DRAG_ACROSS: f32 = 45.0;
const CHARCOAL_SIDE_TOOTH_FREQUENCY: f32 = 34.0;
const CHARCOAL_SIDE_DRAG_WEIGHT: f32 = 0.55;
const CHARCOAL_SIDE_LEVEL_FROM: f32 = 0.38;
const CHARCOAL_SIDE_LEVEL_TO: f32 = 0.62;
const CHARCOAL_SIDE_SALT: u32 = 71;

const PASTEL_VARIANTS: u32 = 8;
const PASTEL_REACH: f32 = 0.9;
const PASTEL_RAGGEDNESS: f32 = 0.6;
const PASTEL_EDGE_FREQUENCY: f32 = 16.0;
const PASTEL_CRUMB_FREQUENCY: f32 = 30.0;
const PASTEL_CRUMB_FROM: f32 = 0.3;
const PASTEL_CRUMB_TO: f32 = 0.55;
const PASTEL_SALT: u32 = 81;

const WASH_VARIANTS: u32 = 8;
const WASH_REACH: f32 = 0.95;
const WASH_RAGGEDNESS: f32 = 0.6;
const WASH_FREQUENCY: f32 = 1.8;
const WASH_WARP: f32 = 1.2;
const WASH_BODY: f32 = 0.72;
const WASH_RIM_WIDTH: f32 = 0.3;
const WASH_GRANULE_FREQUENCY: f32 = 10.0;
const WASH_GRANULE_DEPTH: f32 = 0.3;
const WASH_SALT: u32 = 91;

const SPLASH_VARIANTS: u32 = 14;
const SPLASH_DROPS: u32 = 11;
const SPLASH_CORE_RADIUS: f32 = 0.38;
const SPLASH_CORE_RAGGEDNESS: f32 = 0.3;
const SPLASH_CORE_FREQUENCY: f32 = 3.0;
const SPLASH_MIN_REACH: f32 = 0.5;
const SPLASH_REACH_SPAN: f32 = 0.45;
const SPLASH_MIN_RADIUS: f32 = 0.03;
const SPLASH_RADIUS_SPAN: f32 = 0.1;
const SPLASH_ELONGATION: f32 = 1.9;
const SPLASH_SALT: u32 = 101;

const SPONGE_VARIANTS: u32 = 10;
const SPONGE_FREQUENCY: f32 = 3.5;
const SPONGE_FINE_RATIO: f32 = 2.1;
const SPONGE_SIZE_FREQUENCY: f32 = 2.5;
const SPONGE_SIZE_FLOOR: f32 = 0.4;
const SPONGE_PORE_FROM: f32 = 0.2;
const SPONGE_PORE_TO: f32 = 0.3;
const SPONGE_REACH: f32 = 0.92;
const SPONGE_RAGGEDNESS: f32 = 0.4;
const SPONGE_EDGE_FREQUENCY: f32 = 4.0;
const SPONGE_SALT: u32 = 111;

const SMOKE_VARIANTS: u32 = 8;
const SMOKE_FREQUENCY: f32 = 2.2;
const SMOKE_WARP: f32 = 1.4;
const SMOKE_FROM: f32 = 0.35;
const SMOKE_TO: f32 = 0.85;
const SMOKE_CORE: f32 = 0.25;
const SMOKE_SALT: u32 = 121;

const GRUNGE_VARIANTS: u32 = 12;
const GRUNGE_BLOTCH_FREQUENCY: f32 = 3.0;
const GRUNGE_WARP: f32 = 1.0;
const GRUNGE_BLOTCH_FROM: f32 = 0.5;
const GRUNGE_BLOTCH_TO: f32 = 0.62;
const GRUNGE_BLOTCH_STRENGTH: f32 = 0.85;
const GRUNGE_SPECK_FREQUENCY: f32 = 30.0;
const GRUNGE_SPECK_FLOOR: f32 = 0.62;
const GRUNGE_SPECK_SHARPNESS: f32 = 0.04;
const GRUNGE_VIGNETTE_CORE: f32 = 0.75;
const GRUNGE_SALT: u32 = 131;

const GRASS_VARIANTS: u32 = 10;
const GRASS_BLADES: u32 = 16;
const GRASS_BASE: f32 = 0.9;
const GRASS_MIN_HEIGHT: f32 = 0.5;
const GRASS_HEIGHT_SPAN: f32 = 0.9;
const GRASS_FIELD_REACH: f32 = 0.85;
const GRASS_BLADE_HALF_WIDTH: f32 = 0.07;
const GRASS_LEAN: f32 = 0.4;
const GRASS_SALT: u32 = 141;

const CRACK_VARIANTS: u32 = 8;
const CRACK_FREQUENCY: f32 = 4.0;
const CRACK_WARP_FREQUENCY: f32 = 8.0;
const CRACK_WARP_STRENGTH: f32 = 0.5;
const CRACK_WIDTH: f32 = 0.09;
const CRACK_REACH: f32 = 0.95;
const CRACK_SALT: u32 = 151;

struct Droplet {
    x: f32,
    y: f32,
    radius: f32,
    cos: f32,
    sin: f32,
}

pub fn natural_brushes() -> Vec<Brush> {
    let base = BrushSettings::default();
    vec![
        brush(
            ROUGH_INK_ID,
            DRAWING_CATEGORY,
            BrushSettings { spacing: 0.3, angle_jitter: 1.0, size_jitter: 0.4, scatter: 0.12, pressure_size: 0.6, ..base.clone() },
            variant_set(INK_VARIANTS, rough_ink_tip),
        ),
        brush(
            DRY_STREAK_ID,
            PAINTING_CATEGORY,
            BrushSettings {
                spacing: 0.1,
                follow_direction: true,
                angle_jitter: 0.08,
                size_jitter: 0.25,
                opacity_jitter: 0.35,
                scatter: 0.08,
                flow: 0.9,
                pressure_size: 0.5,
                pressure_opacity: 0.6,
                grain: 0.25,
                grain_scale: 2.0,
                ..base.clone()
            },
            variant_set(STREAK_VARIANTS, dry_streak_tip),
        ),
        brush(
            CHARCOAL_SIDE_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                spacing: 0.14,
                follow_direction: true,
                angle_jitter: 0.12,
                size_jitter: 0.3,
                opacity_jitter: 0.4,
                scatter: 0.1,
                flow: 0.45,
                accumulate: true,
                pressure_size: 0.3,
                pressure_opacity: 0.9,
                grain: 0.5,
                grain_scale: 3.0,
                ..base.clone()
            },
            variant_set(CHARCOAL_SIDE_VARIANTS, charcoal_side_tip),
        ),
        brush(
            PASTEL_SOFT_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                spacing: 0.12,
                angle_jitter: 1.0,
                size_jitter: 0.25,
                opacity_jitter: 0.35,
                scatter: 0.15,
                flow: 0.6,
                accumulate: true,
                pressure_size: 0.25,
                pressure_opacity: 0.7,
                grain: 0.6,
                grain_scale: 4.0,
                ..base.clone()
            },
            variant_set(PASTEL_VARIANTS, pastel_tip),
        ),
        brush(
            WASH_ID,
            PAINTING_CATEGORY,
            BrushSettings {
                spacing: 0.14,
                angle_jitter: 1.0,
                size_jitter: 0.25,
                opacity_jitter: 0.12,
                scatter: 0.12,
                flow: 0.45,
                pressure_size: 0.3,
                pressure_opacity: 0.5,
                ..base.clone()
            },
            variant_set(WASH_VARIANTS, wash_tip),
        ),
        brush(
            INK_SPLASH_ID,
            DECOR_CATEGORY,
            BrushSettings { spacing: 1.0, angle_jitter: 1.0, size_jitter: 0.6, opacity_jitter: 0.15, scatter: 0.8, ..base.clone() },
            variant_set(SPLASH_VARIANTS, ink_splash_tip),
        ),
        brush(
            SEA_SPONGE_ID,
            PAINTING_CATEGORY,
            BrushSettings { spacing: 0.3, angle_jitter: 1.0, size_jitter: 0.35, opacity_jitter: 0.3, scatter: 0.15, flow: 0.95, pressure_opacity: 0.5, ..base.clone() },
            variant_set(SPONGE_VARIANTS, sea_sponge_tip),
        ),
        brush(
            SMOKE_ID,
            PAINTING_CATEGORY,
            BrushSettings {
                spacing: 0.14,
                angle_jitter: 1.0,
                size_jitter: 0.4,
                opacity_jitter: 0.4,
                scatter: 0.5,
                flow: 0.12,
                accumulate: true,
                pressure_opacity: 0.6,
                ..base.clone()
            },
            variant_set(SMOKE_VARIANTS, smoke_tip),
        ),
        brush(
            GRUNGE_ID,
            DECOR_CATEGORY,
            BrushSettings { spacing: 0.45, angle_jitter: 1.0, size_jitter: 0.6, opacity_jitter: 0.3, scatter: 0.9, flow: 0.9, value_jitter: 0.1, ..base.clone() },
            variant_set(GRUNGE_VARIANTS, grunge_tip),
        ),
        brush(
            GRASS_ID,
            DECOR_CATEGORY,
            BrushSettings { spacing: 0.5, angle_jitter: 0.12, size_jitter: 0.5, opacity_jitter: 0.2, scatter: 0.7, pressure_size: 0.4, value_jitter: 0.15, ..base.clone() },
            variant_set(GRASS_VARIANTS, grass_tip),
        ),
        brush(
            CRACKS_ID,
            DECOR_CATEGORY,
            BrushSettings { spacing: 0.6, angle_jitter: 1.0, size_jitter: 0.4, opacity_jitter: 0.25, scatter: 0.5, ..base },
            variant_set(CRACK_VARIANTS, cracks_tip),
        ),
    ]
}

fn rough_ink_tip(variant: u32) -> MaskTip {
    let salt = salted(INK_SALT, variant);
    texture(|x, y| {
        let radius = x.hypot(y);
        let ragged = warped_fbm(x * INK_FREQUENCY, y * INK_FREQUENCY, DETAIL_OCTAVES, INK_WARP, salt) - 0.5;
        let body = edge(INK_RADIUS + ragged * INK_RAGGEDNESS - radius);
        let speck_level = fbm(x * INK_SPECK_FREQUENCY, y * INK_SPECK_FREQUENCY, FINE_OCTAVES, salt + 1);
        let speck = smooth_step(INK_SPECK_FLOOR, INK_SPECK_FLOOR + INK_SPECK_SHARPNESS, speck_level) * edge(INK_SPECK_REACH - radius);
        body.max(speck)
    })
}

fn dry_streak_tip(variant: u32) -> MaskTip {
    let salt = salted(STREAK_SALT, variant);
    texture(|x, y| {
        let lane = (y * 0.5 + 0.5) * STREAK_LANES;
        let index = lane.floor() as i32;
        let across = ((lane - lane.floor()) - 0.5).abs() * 2.0;
        let reach = STREAK_MIN_REACH + (1.0 - STREAK_MIN_REACH) * hash_unit(index, 1, salt);
        let load = hash_unit(index, 2, salt);
        let breakup = fbm(x * STREAK_BREAKUP_FREQUENCY, index as f32 * STREAK_LANE_STRIDE, COARSE_OCTAVES, salt);
        let density = breakup * STREAK_BREAKUP_WEIGHT + load * STREAK_LOAD_WEIGHT;
        let present = smooth_step(STREAK_DENSITY_FROM, STREAK_DENSITY_TO, density);
        let profile = 1.0 - across.powi(4);
        let length = smooth_step(reach, reach - STREAK_FRAYING, x.abs());
        present * profile * length * smooth_step(1.0, STREAK_ENVELOPE, y.abs())
    })
}

fn charcoal_side_tip(variant: u32) -> MaskTip {
    let salt = salted(CHARCOAL_SIDE_SALT, variant);
    texture(|x, y| {
        let squared = (x.abs().powf(CHARCOAL_SIDE_ROUNDING) + y.abs().powf(CHARCOAL_SIDE_ROUNDING)).powf(CHARCOAL_SIDE_ROUNDING.recip());
        let ragged = (fbm(x * CHARCOAL_SIDE_EDGE_FREQUENCY, y * CHARCOAL_SIDE_EDGE_FREQUENCY, COARSE_OCTAVES, salt) - 0.5) * CHARCOAL_SIDE_RAGGEDNESS;
        let body = edge(CHARCOAL_SIDE_EXTENT + ragged - squared);
        let drag = fbm(x * CHARCOAL_SIDE_DRAG_ALONG, y * CHARCOAL_SIDE_DRAG_ACROSS, COARSE_OCTAVES, salt + 1);
        let tooth = fbm(x * CHARCOAL_SIDE_TOOTH_FREQUENCY, y * CHARCOAL_SIDE_TOOTH_FREQUENCY, FINE_OCTAVES, salt + 2);
        let level = drag * CHARCOAL_SIDE_DRAG_WEIGHT + tooth * (1.0 - CHARCOAL_SIDE_DRAG_WEIGHT);
        body * smooth_step(CHARCOAL_SIDE_LEVEL_FROM, CHARCOAL_SIDE_LEVEL_TO, level)
    })
}

fn pastel_tip(variant: u32) -> MaskTip {
    let salt = salted(PASTEL_SALT, variant);
    texture(|x, y| {
        let body = edge(ragged_reach(x, y, PASTEL_REACH, PASTEL_RAGGEDNESS, PASTEL_EDGE_FREQUENCY, salt));
        let crumb = fbm(x * PASTEL_CRUMB_FREQUENCY, y * PASTEL_CRUMB_FREQUENCY, FINE_OCTAVES, salt + 1);
        body * smooth_step(PASTEL_CRUMB_FROM, PASTEL_CRUMB_TO, crumb)
    })
}

fn wash_tip(variant: u32) -> MaskTip {
    let salt = salted(WASH_SALT, variant);
    texture(|x, y| {
        let ragged = warped_fbm(x * WASH_FREQUENCY, y * WASH_FREQUENCY, DETAIL_OCTAVES - 1, WASH_WARP, salt) - 0.5;
        let inside = WASH_REACH + ragged * WASH_RAGGEDNESS - x.hypot(y);
        let pooling = WASH_BODY + (1.0 - WASH_BODY) * (1.0 - smooth_step(0.0, WASH_RIM_WIDTH, inside));
        let granule = fbm(x * WASH_GRANULE_FREQUENCY, y * WASH_GRANULE_FREQUENCY, COARSE_OCTAVES, salt + 1);
        edge(inside) * pooling * (1.0 - WASH_GRANULE_DEPTH + WASH_GRANULE_DEPTH * granule)
    })
}

fn splash_drops(salt: u32) -> Vec<Droplet> {
    (0..SPLASH_DROPS)
        .map(|index| {
            let index = index as i32;
            let angle = hash_unit(index, 1, salt) * TAU;
            let reach = SPLASH_MIN_REACH + SPLASH_REACH_SPAN * hash_unit(index, 2, salt);
            let size = hash_unit(index, 3, salt);
            Droplet {
                x: angle.cos() * reach,
                y: angle.sin() * reach,
                radius: (SPLASH_MIN_RADIUS + SPLASH_RADIUS_SPAN * size * size) * (1.0 - reach * 0.5),
                cos: angle.cos(),
                sin: angle.sin(),
            }
        })
        .collect()
}

fn ink_splash_tip(variant: u32) -> MaskTip {
    let salt = salted(SPLASH_SALT, variant);
    let drops = splash_drops(salt);
    texture(|x, y| {
        let core = edge(ragged_reach(x, y, SPLASH_CORE_RADIUS, SPLASH_CORE_RAGGEDNESS, SPLASH_CORE_FREQUENCY, salt));
        let droplets = drops.iter().map(|droplet| {
            let (dx, dy) = (x - droplet.x, y - droplet.y);
            let along = (dx * droplet.cos + dy * droplet.sin) / SPLASH_ELONGATION;
            let across = dy * droplet.cos - dx * droplet.sin;
            edge(droplet.radius - along.hypot(across))
        });
        droplets.fold(core, f32::max)
    })
}

fn sea_sponge_tip(variant: u32) -> MaskTip {
    let salt = salted(SPONGE_SALT, variant);
    texture(|x, y| {
        let size = SPONGE_SIZE_FLOOR + fbm(x * SPONGE_SIZE_FREQUENCY, y * SPONGE_SIZE_FREQUENCY, FINE_OCTAVES, salt + 2);
        let (coarse, _) = worley(x * SPONGE_FREQUENCY, y * SPONGE_FREQUENCY, salt);
        let (fine, _) = worley(x * SPONGE_FREQUENCY * SPONGE_FINE_RATIO, y * SPONGE_FREQUENCY * SPONGE_FINE_RATIO, salt + 3);
        let bridges = smooth_step(SPONGE_PORE_FROM * size, SPONGE_PORE_TO * size, coarse) * smooth_step(SPONGE_PORE_FROM * size, SPONGE_PORE_TO * size, fine);
        bridges * edge(ragged_reach(x, y, SPONGE_REACH, SPONGE_RAGGEDNESS, SPONGE_EDGE_FREQUENCY, salt + 1))
    })
}

fn smoke_tip(variant: u32) -> MaskTip {
    let salt = salted(SMOKE_SALT, variant);
    texture(|x, y| {
        let cloud = warped_fbm(x * SMOKE_FREQUENCY, y * SMOKE_FREQUENCY, DETAIL_OCTAVES, SMOKE_WARP, salt);
        smooth_step(SMOKE_FROM, SMOKE_TO, cloud) * smooth_step(1.0, SMOKE_CORE, x.hypot(y))
    })
}

fn grunge_tip(variant: u32) -> MaskTip {
    let salt = salted(GRUNGE_SALT, variant);
    texture(|x, y| {
        let blotch_level = warped_fbm(x * GRUNGE_BLOTCH_FREQUENCY, y * GRUNGE_BLOTCH_FREQUENCY, DETAIL_OCTAVES, GRUNGE_WARP, salt);
        let blotch = smooth_step(GRUNGE_BLOTCH_FROM, GRUNGE_BLOTCH_TO, blotch_level) * GRUNGE_BLOTCH_STRENGTH;
        let speck_level = fbm(x * GRUNGE_SPECK_FREQUENCY, y * GRUNGE_SPECK_FREQUENCY, FINE_OCTAVES, salt + 1);
        let speck = smooth_step(GRUNGE_SPECK_FLOOR, GRUNGE_SPECK_FLOOR + GRUNGE_SPECK_SHARPNESS, speck_level);
        blotch.max(speck) * smooth_step(1.0, GRUNGE_VIGNETTE_CORE, x.hypot(y))
    })
}

fn grass_tip(variant: u32) -> MaskTip {
    let salt = salted(GRASS_SALT, variant);
    let blades: Vec<(f32, f32, f32)> = (0..GRASS_BLADES)
        .map(|index| {
            let index = index as i32;
            let base_x = (hash_unit(index, 1, salt) * 2.0 - 1.0) * GRASS_FIELD_REACH;
            let height = GRASS_MIN_HEIGHT + GRASS_HEIGHT_SPAN * hash_unit(index, 2, salt);
            let lean = (hash_unit(index, 3, salt) * 2.0 - 1.0) * GRASS_LEAN;
            (base_x, height, lean)
        })
        .collect();
    texture(|x, y| {
        blades
            .iter()
            .map(|&(base_x, height, lean)| {
                let progress = (GRASS_BASE - y) / height;
                if !(0.0..=1.0).contains(&progress) {
                    return 0.0;
                }
                let centre = base_x + lean * progress * progress;
                edge(GRASS_BLADE_HALF_WIDTH * (1.0 - progress) - (x - centre).abs())
            })
            .fold(0.0, f32::max)
    })
}

fn cracks_tip(variant: u32) -> MaskTip {
    let salt = salted(CRACK_SALT, variant);
    texture(|x, y| {
        let warp_x = (fbm(x * CRACK_WARP_FREQUENCY, y * CRACK_WARP_FREQUENCY, FINE_OCTAVES, salt + 1) - 0.5) * CRACK_WARP_STRENGTH;
        let warp_y = (fbm(x * CRACK_WARP_FREQUENCY, y * CRACK_WARP_FREQUENCY, FINE_OCTAVES, salt + 2) - 0.5) * CRACK_WARP_STRENGTH;
        let (nearest, second) = worley(x * CRACK_FREQUENCY + warp_x, y * CRACK_FREQUENCY + warp_y, salt);
        (1.0 - smooth_step(0.0, CRACK_WIDTH, second - nearest)) * edge(CRACK_REACH - x.hypot(y))
    })
}
