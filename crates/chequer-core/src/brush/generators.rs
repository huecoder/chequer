use std::f32::consts::TAU;

use super::{BrushError, MaskTip, TipGenerator, TipParam, TipValues, hash_unit, rasterize, soft_edge, value_noise};

const BRISTLE_FIELD_REACH: f32 = 0.88;
const BRISTLE_RADIUS_SPREAD: f32 = 0.04;
const BRISTLE_MIN_ALPHA: f32 = 0.55;
const COMB_SPAN: f32 = 0.95;

pub(super) static BUILTIN: [&TipGenerator; 4] = [&BRISTLES, &COMB, &SPLIT, &NOISE];

static BRISTLES: TipGenerator = TipGenerator {
    id: "bristles",
    params: &[TipParam { key: "count", default: 90.0, range: (1.0, 400.0) }, TipParam { key: "radius", default: 0.03, range: (0.005, 0.3) }],
    render: render_bristles,
};

static COMB: TipGenerator = TipGenerator {
    id: "comb",
    params: &[TipParam { key: "teeth", default: 7.0, range: (2.0, 24.0) }, TipParam { key: "fill", default: 0.55, range: (0.1, 0.95) }, TipParam { key: "reach", default: 0.9, range: (0.2, 1.0) }],
    render: render_comb,
};

static SPLIT: TipGenerator = TipGenerator {
    id: "split",
    params: &[TipParam { key: "gap", default: 0.45, range: (0.1, 0.9) }, TipParam { key: "width", default: 0.09, range: (0.02, 0.4) }, TipParam { key: "reach", default: 0.9, range: (0.3, 1.0) }],
    render: render_split,
};

static NOISE: TipGenerator = TipGenerator {
    id: "noise",
    params: &[
        TipParam { key: "frequency", default: 6.0, range: (1.0, 24.0) },
        TipParam { key: "threshold", default: 0.45, range: (0.0, 0.9) },
        TipParam { key: "contrast", default: 8.0, range: (1.0, 20.0) },
        TipParam { key: "reach", default: 0.95, range: (0.3, 1.0) },
    ],
    render: render_noise,
};

struct Bristle {
    x: f32,
    y: f32,
    radius: f32,
    alpha: f32,
}

fn render_bristles(values: &TipValues, seed: u32, side: u32) -> Result<MaskTip, BrushError> {
    let (count, min_radius) = (values.get("count").round() as u32, values.get("radius"));
    let bristles: Vec<Bristle> = (0..count)
        .map(|index| {
            let (angle, distance) = (hash_unit(index as i32, 1, seed) * TAU, hash_unit(index as i32, 2, seed).sqrt() * BRISTLE_FIELD_REACH);
            Bristle {
                x: angle.cos() * distance,
                y: angle.sin() * distance,
                radius: min_radius + BRISTLE_RADIUS_SPREAD * hash_unit(index as i32, 3, seed),
                alpha: BRISTLE_MIN_ALPHA + (1.0 - BRISTLE_MIN_ALPHA) * hash_unit(index as i32, 4, seed),
            }
        })
        .collect();
    rasterize(side, |x, y| bristles.iter().map(|bristle| soft_edge(bristle.radius - (x - bristle.x).hypot(y - bristle.y), side) * bristle.alpha).fold(0.0, f32::max))
}

fn render_comb(values: &TipValues, _seed: u32, side: u32) -> Result<MaskTip, BrushError> {
    let (teeth, fill, reach) = (values.get("teeth").round(), values.get("fill"), values.get("reach"));
    let period = COMB_SPAN * 2.0 / teeth;
    rasterize(side, |x, y| {
        let phase = (x + COMB_SPAN).rem_euclid(period) - period / 2.0;
        let inside = (fill * period / 2.0 - phase.abs()).min(COMB_SPAN - x.abs()).min(reach - y.abs());
        soft_edge(inside, side)
    })
}

fn render_split(values: &TipValues, _seed: u32, side: u32) -> Result<MaskTip, BrushError> {
    let (gap, width, reach) = (values.get("gap"), values.get("width"), values.get("reach"));
    let straight_length = (reach - width).max(0.0);
    rasterize(side, |x, y| {
        let beyond_ends = (y.abs() - straight_length).max(0.0);
        let distance = (x.abs() - gap).hypot(beyond_ends);
        soft_edge(width - distance, side)
    })
}

fn render_noise(values: &TipValues, seed: u32, side: u32) -> Result<MaskTip, BrushError> {
    let (frequency, threshold, contrast, reach) = (values.get("frequency"), values.get("threshold"), values.get("contrast"), values.get("reach"));
    rasterize(side, |x, y| ((value_noise(x * frequency, y * frequency, seed) - threshold) * contrast).clamp(0.0, 1.0) * soft_edge(reach - x.hypot(y), side))
}
