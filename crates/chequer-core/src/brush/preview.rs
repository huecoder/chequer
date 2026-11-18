use std::sync::Arc;

use super::Brush;
use crate::blend::BlendMode;
use crate::draw::{Stroke, StrokeKind, StrokeParams, StrokeTarget};
use crate::edit::PixelEdit;
use crate::geom::FPoint;
use crate::raster::{Raster, Rgba};

const PREVIEW_SEED: u64 = 0x5EED;
const PREVIEW_STEPS: u32 = 64;
const PREVIEW_STEP_MS: f64 = 8.0;
const HORIZONTAL_PADDING_RATIO: f32 = 0.12;
const WAVE_AMPLITUDE_RATIO: f32 = 0.22;
const WAVE_TURNS: f32 = 1.0;
const MAX_SIZE_RATIO: f32 = 0.55;
const MAX_DAB_SIZE_RATIO: f32 = 0.9;
const PREVIEW_PRESSURE: f32 = 1.0;

pub struct PreviewStyle {
    pub size: f32,
    pub color: Rgba,
}

fn preview_params(brush: &Arc<Brush>, style: &PreviewStyle, max_size: f32) -> StrokeParams {
    StrokeParams {
        kind: StrokeKind::Brush,
        color: style.color,
        size: style.size.min(max_size),
        antialias: true,
        blend: BlendMode::Normal,
        pixel_perfect: false,
        smooth: false,
        wrap: false,
        brush: Arc::clone(brush),
        seed: PREVIEW_SEED,
        pressure: PREVIEW_PRESSURE,
        tilt: None,
    }
}

pub fn stroke_preview(brush: &Arc<Brush>, style: &PreviewStyle, width: u32, height: u32) -> Raster {
    let mut raster = Raster::transparent(width, height);
    let mut edit = PixelEdit::new(0, 0, &raster);
    let mut target = StrokeTarget { raster: &mut raster, edit: &mut edit, selection: None };
    let dab_zone = height as f32;
    let dab_center = FPoint::new(dab_zone / 2.0, height as f32 / 2.0);
    Stroke::begin(preview_params(brush, style, height as f32 * MAX_DAB_SIZE_RATIO), dab_center, &mut target);
    let wave_width = (width as f32 - dab_zone).max(1.0);
    let (padding, middle, amplitude) = (wave_width * HORIZONTAL_PADDING_RATIO, height as f32 / 2.0, height as f32 * WAVE_AMPLITUDE_RATIO);
    let point_at = |step: u32| {
        let progress = step as f32 / PREVIEW_STEPS as f32;
        FPoint::new(dab_zone + padding + (wave_width - 2.0 * padding) * progress, middle + amplitude * (progress * WAVE_TURNS * std::f32::consts::TAU).sin())
    };
    let (mut stroke, _) = Stroke::begin(preview_params(brush, style, height as f32 * MAX_SIZE_RATIO), point_at(0), &mut target);
    for step in 1..=PREVIEW_STEPS {
        stroke.set_time_ms(f64::from(step) * PREVIEW_STEP_MS);
        stroke.extend(point_at(step), &mut target);
    }
    stroke.flush(point_at(PREVIEW_STEPS), &mut target);
    raster
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_brush_preview_has_ink_in_the_middle_and_margin_stays_clear() {
        let style = PreviewStyle { size: 6.0, color: [0, 0, 0, 255] };
        let raster = stroke_preview(&Arc::new(Brush::round()), &style, 100, 30);
        assert!((0..30).any(|y| raster.get(50, y)[3] > 0));
        assert!((0..30).all(|y| raster.get(1, y)[3] == 0));
    }
}
