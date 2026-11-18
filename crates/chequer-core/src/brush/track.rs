use serde::{Deserialize, Serialize};

use super::marks::{MAX_MARKS, Mark, render_marks};
use super::Texture;

pub const MAX_TRACK_LAYERS: usize = 8;
pub const MAX_PITCH_STEPS: usize = 16;
pub const PITCH_RANGE: (f32, f32) = (0.02, 20.0);
pub const PITCH_STEP_RANGE: (f32, f32) = (0.2, 5.0);
const NEUTRAL_PITCH_STEP: f32 = 1.0;
const MIN_TILE_LENGTH: f32 = 1e-4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerSpec {
    pub pitch: f32,
    pub pitches: Vec<f32>,
    pub amount: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<Mark>,
}

impl Default for LayerSpec {
    fn default() -> Self {
        Self { pitch: 1.0, pitches: Vec::new(), amount: 1.0, marks: Vec::new() }
    }
}

impl LayerSpec {
    pub fn clamped(self) -> Self {
        Self {
            pitch: self.pitch.clamp(PITCH_RANGE.0, PITCH_RANGE.1),
            pitches: self.pitches.into_iter().take(MAX_PITCH_STEPS).map(|step| step.clamp(PITCH_STEP_RANGE.0, PITCH_STEP_RANGE.1)).collect(),
            amount: self.amount.clamp(0.0, 1.0),
            marks: self.marks.into_iter().take(MAX_MARKS).map(Mark::clamped).collect(),
        }
    }

    fn tile_fraction(&self, tiles: f32) -> f32 {
        if self.pitches.is_empty() {
            return tiles.rem_euclid(NEUTRAL_PITCH_STEP);
        }
        let cycle: f32 = self.pitches.iter().sum();
        let mut remaining = tiles.rem_euclid(cycle);
        for &step in &self.pitches {
            if remaining < step {
                return remaining / step.max(MIN_TILE_LENGTH);
            }
            remaining -= step;
        }
        0.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackLayer {
    pub spec: LayerSpec,
    pub texture: Texture,
}

impl TrackLayer {
    pub fn marked(spec: LayerSpec) -> Self {
        let texture = render_marks(spec.pitch, &spec.marks);
        Self { spec, texture }
    }

    pub fn is_marked(&self) -> bool {
        !self.spec.marks.is_empty()
    }

    pub fn redraw_marks(&mut self) {
        if self.is_marked() {
            self.texture = render_marks(self.spec.pitch, &self.spec.marks);
        }
    }

    fn value(&self, across: f32, along: f32, unit: f32) -> f32 {
        let fraction = self.spec.tile_fraction(along / (self.spec.pitch * unit));
        let texel = self.texture.profile(across, 1.0 - fraction);
        1.0 - self.spec.amount * (1.0 - texel)
    }
}

#[derive(Clone, Copy)]
pub struct TrackSampler<'a> {
    layers: &'a [TrackLayer],
    unit: f32,
}

impl<'a> TrackSampler<'a> {
    pub fn new(layers: &'a [TrackLayer], unit: f32) -> Self {
        Self { layers, unit }
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    pub fn value(&self, across: f32, along: f32) -> f32 {
        self.layers.iter().fold(1.0, |paint, layer| paint * layer.value(across, along, self.unit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stripes() -> Texture {
        Texture::new(1, 4, vec![255, 255, 0, 0]).unwrap()
    }

    fn layer(spec: LayerSpec) -> TrackLayer {
        TrackLayer { spec, texture: stripes() }
    }

    #[test]
    fn tile_repeats_every_pitch_and_the_image_top_is_the_forward_end() {
        let layers = [layer(LayerSpec { pitch: 1.0, ..LayerSpec::default() })];
        let track = TrackSampler::new(&layers, 10.0);
        assert!(track.value(0.5, 9.0) > 0.8);
        assert!(track.value(0.5, 1.0) < 0.2);
        assert!((track.value(0.5, 1.0) - track.value(0.5, 11.0)).abs() < 1e-4);
    }

    #[test]
    fn pitch_sequence_stretches_each_repeat_in_turn() {
        let layers = [layer(LayerSpec { pitch: 1.0, pitches: vec![1.0, 2.0], ..LayerSpec::default() })];
        let track = TrackSampler::new(&layers, 10.0);
        assert!((track.value(0.5, 15.0) - track.value(0.5, 2.5)).abs() < 1e-4);
        assert!((track.value(0.5, 5.0) - track.value(0.5, 35.0)).abs() < 1e-4);
    }

    #[test]
    fn layers_multiply_and_amount_fades_a_layer() {
        let faint = LayerSpec { amount: 0.5, ..LayerSpec::default() };
        let layers = [layer(faint.clone()), layer(faint)];
        let track = TrackSampler::new(&layers, 10.0);
        assert!((track.value(0.5, 3.0) - 0.25).abs() < 1e-4);
    }

    #[test]
    fn hostile_spec_is_clamped() {
        let spec = LayerSpec { pitch: 0.0, pitches: vec![0.0; 100], amount: 9.0, ..LayerSpec::default() }.clamped();
        assert!(spec.pitch >= PITCH_RANGE.0 && spec.pitches.len() == MAX_PITCH_STEPS && spec.amount == 1.0);
        assert!(spec.pitches.iter().all(|&step| step >= PITCH_STEP_RANGE.0));
    }
}
