use std::collections::BTreeMap;

use chequer_core::brush::{Brush, BrushSettings, BrushTip, TipSpec, TipVariant, generate_tip};

use super::classic::brush;

const DECOR_CATEGORY: &str = "decor";
const TEXTURE_CATEGORY: &str = "texture";
const SNOWFLAKES_ID: &str = "nat.snowflakes";
const FROST_ID: &str = "nat.frost";
const SNOWFLAKE_SIDE: u32 = 384;
const SNOWFLAKE_DETAIL: f32 = 120.0;
const FROST_SIDE: u32 = 256;

const SNOWFLAKE_KINDS: [(f32, f32, f32, u32); 6] = [
    (0.55, 0.2, 0.1, 13),
    (0.6, 0.1, 0.1, 15),
    (0.5, 0.3, 0.1, 11),
    (0.45, 0.5, 0.1, 12),
    (0.7, 2.5, 0.05, 21),
    (0.58, 0.15, 0.1, 17),
];
const FROST_KINDS: [(f32, f32, f32, u32); 4] = [(14.0, 3.0, 0.8, 1), (18.0, 3.0, 0.95, 2), (12.0, 2.0, 0.7, 3), (16.0, 3.0, 0.85, 4)];

fn tip(generator: &str, side: u32, seed: u32, params: &[(&str, f32)]) -> TipVariant {
    let spec = TipSpec { generator: generator.into(), params: params.iter().map(|&(key, value)| (key.to_owned(), value)).collect::<BTreeMap<_, _>>(), seed };
    TipVariant::new(generate_tip(&spec, side).unwrap_or_else(|error| panic!("{generator}: {error}")))
}

pub fn crystal_brushes() -> Vec<Brush> {
    let snowflakes = SNOWFLAKE_KINDS.iter().map(|&(vapor, sticking, unevenness, seed)| tip("snowflake", SNOWFLAKE_SIDE, seed, &[("vapor", vapor), ("sticking", sticking), ("unevenness", unevenness), ("detail", SNOWFLAKE_DETAIL)])).collect();
    let fronds = FROST_KINDS.iter().map(|&(branches, depth, spread, seed)| tip("frost", FROST_SIDE, seed, &[("branches", branches), ("depth", depth), ("spread", spread)])).collect();
    vec![
        brush(
            SNOWFLAKES_ID,
            DECOR_CATEGORY,
            BrushSettings { spacing: 1.1, scatter: 0.9, size_jitter: 0.7, angle_jitter: 1.0, opacity_jitter: 0.25, pressure_size: 0.6, value_jitter: 0.04, hardness: 1.0, ..BrushSettings::default() },
            BrushTip::from_variants(snowflakes),
        ),
        brush(
            FROST_ID,
            TEXTURE_CATEGORY,
            BrushSettings { spacing: 0.12, scatter: 0.5, size_jitter: 0.6, angle_jitter: 1.0, opacity_jitter: 0.35, flow: 0.8, pressure_size: 0.4, pressure_opacity: 0.3, grain: 0.2, hardness: 1.0, ..BrushSettings::default() },
            BrushTip::from_variants(fronds),
        ),
    ]
}
