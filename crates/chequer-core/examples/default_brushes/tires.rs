use std::f32::consts::TAU;

use chequer_core::brush::{Brush, BrushEngine, BrushSettings, BrushTextures, BrushTip, LayerSpec, Texture, TrackLayer, hash_unit};

use super::classic::brush;
use super::texture::smooth_step;

const TRACKS_CATEGORY: &str = "tracks";
const WINTER_TIRE_ID: &str = "trk.tire_winter";
const SUMMER_TIRE_ID: &str = "trk.tire_summer";
const ACROSS_DEGREES: f32 = 90.0;
const FLAT_NIB_ROUNDNESS: f32 = 0.05;

const TREAD_WIDTH_TEXELS: u32 = 384;
const PROFILE_WIDTH_TEXELS: u32 = 256;
const NOISE_TILE_TEXELS: u32 = 256;
const GRIT_TILE_TEXELS: u32 = 128;

const SHOULDER_ROUNDING: f32 = 0.045;
const CONTACT_CENTER_BIAS: f32 = 0.14;

const WINTER_PITCH: f32 = 0.3125;
const WINTER_PITCH_STEPS: [f32; 6] = [1.0, 1.18, 0.86, 1.12, 0.92, 0.9];
const WINTER_GROOVE_AT: f32 = 0.19;
const WINTER_GROOVE_WAVE: f32 = 0.009;
const WINTER_GROOVE_HALF: f32 = 0.02;
const WINTER_RIB_HALF: f32 = 0.045;
const WINTER_CENTER_SLOPE: f32 = 1.0;
const WINTER_CENTER_HALF: f32 = 0.017;
const WINTER_SHOULDER_SLOPE: f32 = 0.55;
const WINTER_SHOULDER_PHASE: f32 = 0.5;
const WINTER_SHOULDER_HALF: f32 = 0.024;
const WINTER_SIPE_HALF: f32 = 0.004;
const WINTER_SIPE_ZIG: f32 = 0.006;
const WINTER_SIPE_ZIG_PERIOD: f32 = 0.05;
const WINTER_SIPE_DEPTH: f32 = 0.85;

const SUMMER_PITCH: f32 = 0.28;
const SUMMER_PITCH_STEPS: [f32; 5] = [1.0, 1.14, 0.88, 1.08, 0.93];
const SUMMER_GROOVES: [(f32, f32); 4] = [(0.2, 0.015), (0.4, 0.02), (0.6, 0.02), (0.8, 0.015)];
const SUMMER_SLOT_FROM: f32 = 0.32;
const SUMMER_SLOT_SLOPE: f32 = 0.22;
const SUMMER_SLOT_HALF: f32 = 0.024;
const SUMMER_SIPE_FROM: f32 = 0.115;
const SUMMER_SIPE_TO: f32 = 0.265;
const SUMMER_SIPE_SLOPE: f32 = 0.7;
const SUMMER_SIPE_HALF: f32 = 0.0045;
const SUMMER_SIPE_PER_PITCH: f32 = 2.0;
const SUMMER_NOTCH_FROM: f32 = 0.03;
const SUMMER_NOTCH_TO: f32 = 0.075;
const SUMMER_NOTCH_HALF: f32 = 0.006;

const WEAR_PITCH: f32 = 5.0;
const WEAR_AMOUNT: f32 = 0.45;
const WEAR_CELLS_ACROSS: f32 = 4.0;
const WEAR_OCTAVES: u32 = 3;
const WEAR_THRESHOLD: (f32, f32) = (0.45, 0.8);
const WEAR_SALT: u32 = 31;
const GRIT_PITCH: f32 = 0.9;
const GRIT_PITCH_STEPS: [f32; 3] = [1.0, 1.37, 0.73];
const GRIT_AMOUNT: f32 = 0.45;
const GRIT_CELLS_ACROSS: f32 = 40.0;
const GRIT_THRESHOLD: (f32, f32) = (0.8, 0.92);
const GRIT_SALT: u32 = 57;

const SUMMER_WEAR_SALT: u32 = 77;
const SUMMER_GRIT_SALT: u32 = 91;

struct Tile {
    width: u32,
    height: u32,
    pitch: f32,
}

impl Tile {
    fn for_pitch(width: u32, pitch: f32) -> Self {
        Self { width, height: (width as f32 * pitch).round() as u32, pitch }
    }

    fn render(&self, coverage: impl Fn(f32, f32) -> f32) -> Texture {
        let pixels = (0..self.width * self.height)
            .map(|index| {
                let (column, row) = (index % self.width, index / self.width);
                let across = (column as f32 + 0.5) / self.width as f32;
                let along = (1.0 - (row as f32 + 0.5) / self.height as f32) * self.pitch;
                (coverage(across, along).clamp(0.0, 1.0) * u8::MAX as f32).round() as u8
            })
            .collect();
        Texture::new(self.width, self.height, pixels).expect("tread tile is within texture limits")
    }

    fn ramp(&self, inside: f32) -> f32 {
        (0.5 + inside * self.width as f32).clamp(0.0, 1.0)
    }

    fn band(&self, distance: f32, half_width: f32) -> f32 {
        self.ramp(half_width - distance.abs())
    }

    fn slanted(&self, across_offset: f32, along: f32, slope: f32, phase: f32, repeats: f32) -> f32 {
        let period = self.pitch / repeats;
        let behind = along - phase * self.pitch + slope * across_offset;
        wrapped(behind, period) / slope.hypot(1.0)
    }
}

fn wrapped(value: f32, period: f32) -> f32 {
    (value + period / 2.0).rem_euclid(period) - period / 2.0
}

fn winter_coverage(tile: &Tile, across: f32, along: f32) -> f32 {
    let offset = (across - 0.5).abs();
    let groove_at = WINTER_GROOVE_AT + WINTER_GROOVE_WAVE * (TAU * along / tile.pitch).sin();
    let main_groove = tile.band(offset - groove_at, WINTER_GROOVE_HALF);
    let center_zone = tile.ramp(offset - WINTER_RIB_HALF).min(tile.ramp(groove_at - offset));
    let shoulder_zone = tile.ramp(offset - groove_at);
    let center_groove = tile.band(tile.slanted(offset, along, WINTER_CENTER_SLOPE, 0.0, 1.0), WINTER_CENTER_HALF) * center_zone;
    let shoulder_groove = tile.band(tile.slanted(offset - groove_at, along, WINTER_SHOULDER_SLOPE, WINTER_SHOULDER_PHASE, 1.0), WINTER_SHOULDER_HALF) * shoulder_zone;
    let zig = WINTER_SIPE_ZIG * (TAU * offset / WINTER_SIPE_ZIG_PERIOD).sin();
    let center_sipe = tile.band(tile.slanted(offset, along + zig, WINTER_CENTER_SLOPE, 0.5, 1.0), WINTER_SIPE_HALF) * center_zone;
    let shoulder_sipe = tile.band(tile.slanted(offset - groove_at, along + zig, WINTER_SHOULDER_SLOPE, 0.0, 1.0), WINTER_SIPE_HALF) * shoulder_zone;
    let sipes = center_sipe.max(shoulder_sipe) * WINTER_SIPE_DEPTH;
    1.0 - main_groove.max(center_groove).max(shoulder_groove).max(sipes)
}

fn summer_coverage(tile: &Tile, across: f32, along: f32) -> f32 {
    let offset = (across - 0.5).abs();
    let longitudinal = SUMMER_GROOVES.iter().map(|&(at, half)| tile.band(across - at, half)).fold(0.0, f32::max);
    let shoulder_zone = tile.ramp(offset - SUMMER_SLOT_FROM);
    let slot = tile.band(tile.slanted(offset - SUMMER_SLOT_FROM, along, SUMMER_SLOT_SLOPE, 0.3, 1.0), SUMMER_SLOT_HALF) * shoulder_zone;
    let rib_zone = tile.ramp(offset - SUMMER_SIPE_FROM).min(tile.ramp(SUMMER_SIPE_TO - offset));
    let sipe = tile.band(tile.slanted(offset, along, SUMMER_SIPE_SLOPE, 0.0, SUMMER_SIPE_PER_PITCH), SUMMER_SIPE_HALF) * rib_zone;
    let notch_zone = tile.ramp(offset - SUMMER_NOTCH_FROM).min(tile.ramp(SUMMER_NOTCH_TO - offset));
    let side = if across < 0.5 { 0.0 } else { 0.5 };
    let notch = tile.band(wrapped(along - side * tile.pitch, tile.pitch), SUMMER_NOTCH_HALF) * notch_zone;
    1.0 - longitudinal.max(slot).max(sipe).max(notch)
}

fn periodic_noise(x: f32, y: f32, period: i32, salt: u32) -> f32 {
    let (cell_x, cell_y) = (x.floor(), y.floor());
    let smooth = |fraction: f32| fraction * fraction * (3.0 - 2.0 * fraction);
    let (blend_x, blend_y) = (smooth(x - cell_x), smooth(y - cell_y));
    let corner = |step_x: i32, step_y: i32| hash_unit(cell_x as i32 + step_x, (cell_y as i32 + step_y).rem_euclid(period), salt);
    let lerp = |from: f32, to: f32, amount: f32| from + (to - from) * amount;
    lerp(lerp(corner(0, 0), corner(1, 0), blend_x), lerp(corner(0, 1), corner(1, 1), blend_x), blend_y)
}

fn periodic_fbm(x: f32, y: f32, period: i32, octaves: u32, salt: u32) -> f32 {
    let (mut sum, mut weight, mut amplitude) = (0.0, 0.0, 1.0);
    for octave in 0..octaves {
        let scale = (1 << octave) as f32;
        sum += amplitude * periodic_noise(x * scale, y * scale, period << octave, salt + octave);
        weight += amplitude;
        amplitude /= 2.0;
    }
    sum / weight
}

fn contact_layer() -> TrackLayer {
    let pixels = (0..PROFILE_WIDTH_TEXELS)
        .map(|column| {
            let across = (column as f32 + 0.5) / PROFILE_WIDTH_TEXELS as f32;
            let offset = (across - 0.5).abs() * 2.0;
            let rounded = smooth_step(0.0, SHOULDER_ROUNDING, across.min(1.0 - across));
            let pressure = 1.0 - CONTACT_CENTER_BIAS * offset.powi(3);
            ((rounded * pressure).clamp(0.0, 1.0) * u8::MAX as f32).round() as u8
        })
        .collect();
    TrackLayer { spec: LayerSpec::default(), texture: Texture::new(PROFILE_WIDTH_TEXELS, 1, pixels).expect("profile fits texture limits") }
}

fn wear_layer(salt: u32) -> TrackLayer {
    let tile = Tile { width: NOISE_TILE_TEXELS, height: NOISE_TILE_TEXELS, pitch: WEAR_PITCH };
    let cells_along = (WEAR_CELLS_ACROSS * WEAR_PITCH) as i32;
    let texture = tile.render(|across, along| 1.0 - smooth_step(WEAR_THRESHOLD.0, WEAR_THRESHOLD.1, periodic_fbm(across * WEAR_CELLS_ACROSS, along * WEAR_CELLS_ACROSS, cells_along, WEAR_OCTAVES, salt)));
    TrackLayer { spec: LayerSpec { pitch: WEAR_PITCH, amount: WEAR_AMOUNT, ..LayerSpec::default() }, texture }
}

fn grit_layer(salt: u32) -> TrackLayer {
    let tile = Tile { width: GRIT_TILE_TEXELS, height: GRIT_TILE_TEXELS, pitch: GRIT_PITCH };
    let cells_along = (GRIT_CELLS_ACROSS * GRIT_PITCH) as i32;
    let texture = tile.render(|across, along| 1.0 - smooth_step(GRIT_THRESHOLD.0, GRIT_THRESHOLD.1, periodic_noise(across * GRIT_CELLS_ACROSS, along * GRIT_CELLS_ACROSS, cells_along, salt)));
    TrackLayer { spec: LayerSpec { pitch: GRIT_PITCH, pitches: GRIT_PITCH_STEPS.to_vec(), amount: GRIT_AMOUNT, ..LayerSpec::default() }, texture }
}

fn tread_layer(pitch: f32, steps: &[f32], coverage: fn(&Tile, f32, f32) -> f32) -> TrackLayer {
    let tile = Tile::for_pitch(TREAD_WIDTH_TEXELS, pitch);
    let texture = tile.render(|across, along| coverage(&tile, across, along));
    TrackLayer { spec: LayerSpec { pitch, pitches: steps.to_vec(), ..LayerSpec::default() }, texture }
}

fn tire(id: &str, layers: Vec<TrackLayer>) -> Brush {
    let settings = BrushSettings { engine: BrushEngine::Ribbon, angle: ACROSS_DEGREES, follow_direction: true, roundness: FLAT_NIB_ROUNDNESS, pressure_size: 0.0, ..BrushSettings::default() };
    let mut tire = brush(id, TRACKS_CATEGORY, settings, BrushTip::Round);
    tire.textures = BrushTextures { layers, ..BrushTextures::default() };
    tire
}

pub fn tire_brushes() -> Vec<Brush> {
    vec![
        tire(
            WINTER_TIRE_ID,
            vec![contact_layer(), tread_layer(WINTER_PITCH, &WINTER_PITCH_STEPS, winter_coverage), wear_layer(WEAR_SALT), grit_layer(GRIT_SALT)],
        ),
        tire(
            SUMMER_TIRE_ID,
            vec![contact_layer(), tread_layer(SUMMER_PITCH, &SUMMER_PITCH_STEPS, summer_coverage), wear_layer(SUMMER_WEAR_SALT), grit_layer(SUMMER_GRIT_SALT)],
        ),
    ]
}
