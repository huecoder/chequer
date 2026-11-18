use std::f32::consts::FRAC_PI_2;

use super::{BrushError, MaskTip, TipGenerator, TipParam, TipValues, hash_unit, rasterize, soft_edge};

const HEX_ROW_HEIGHT: f32 = 0.866_025_4;
const HEX_NEIGHBORS: [(i32, i32); 6] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, -1), (-1, 1)];
const FROZEN_MASS: f32 = 1.0;
const SEED_MASS: f32 = 1.0;
const GAMMA_UNIT: f32 = 1e-3;
const GROWTH_STEPS_PER_CELL: usize = 14;
const EDGE_STOP_MARGIN: i32 = 3;
const FLAKE_FILL: f32 = 0.96;
const ICE_EDGE: (f32, f32) = (0.45, 0.8);
const BETA_NOISE_SALT: u32 = 0x51;
const VAPOR_NOISE_STRENGTH: f32 = 0.5;

const FROST_STEM_REACH: f32 = 0.86;
const FROST_STEM_WIDTH: f32 = 0.05;
const FROST_TIP_TAPER: f32 = 0.25;
const FROST_SIDE_LENGTH_SHARE: f32 = 0.4;
const FROST_SIDE_WIDTH_SHARE: f32 = 0.7;
const FROST_LENGTH_FALLOFF: f32 = 0.7;
const FROST_MIN_LENGTH: f32 = 0.05;
const FROST_MIN_SEGMENTS: usize = 3;
const FROST_SEGMENT_SHARE: f32 = 0.66;
const FROST_JITTER_SHARE: f32 = 0.35;
const FROST_SALT_STRIDE: i32 = 7;

pub(super) static BUILTIN: [&TipGenerator; 2] = [&SNOWFLAKE, &FROST];

static SNOWFLAKE: TipGenerator = TipGenerator {
    id: "snowflake",
    params: &[
        TipParam { key: "vapor", default: 0.4, range: (0.25, 0.95) },
        TipParam { key: "diffusion", default: 1.0, range: (0.4, 2.0) },
        TipParam { key: "sticking", default: 1.0, range: (0.01, 10.0) },
        TipParam { key: "unevenness", default: 0.1, range: (0.0, 0.5) },
        TipParam { key: "detail", default: 64.0, range: (30.0, 140.0) },
    ],
    render: render_snowflake,
};

static FROST: TipGenerator = TipGenerator {
    id: "frost",
    params: &[
        TipParam { key: "branches", default: 14.0, range: (4.0, 24.0) },
        TipParam { key: "depth", default: 2.0, range: (1.0, 4.0) },
        TipParam { key: "spread", default: 0.95, range: (0.4, 1.4) },
        TipParam { key: "curl", default: 0.3, range: (0.0, 1.0) },
    ],
    render: render_frost,
};

struct Lattice {
    radius: i32,
    width: usize,
    mass: Vec<f32>,
}

impl Lattice {
    fn index(&self, q: i32, r: i32) -> usize {
        (r + self.radius) as usize * self.width + (q + self.radius) as usize
    }

    fn inside(&self, q: i32, r: i32) -> bool {
        q.abs().max(r.abs()).max((q + r).abs()) <= self.radius
    }

    fn frozen(&self, q: i32, r: i32) -> bool {
        self.inside(q, r) && self.mass[self.index(q, r)] >= FROZEN_MASS
    }

    fn receptive(&self, q: i32, r: i32) -> bool {
        self.frozen(q, r) || HEX_NEIGHBORS.iter().any(|&(step_q, step_r)| self.frozen(q + step_q, r + step_r))
    }

    fn ice(&self, q: i32, r: i32) -> f32 {
        if self.inside(q, r) && self.receptive(q, r) { self.mass[self.index(q, r)].min(FROZEN_MASS) } else { 0.0 }
    }

    fn reached_edge(&self) -> bool {
        let limit = self.radius - EDGE_STOP_MARGIN;
        (-self.radius..=self.radius).any(|r| (-self.radius..=self.radius).any(|q| self.inside(q, r) && q.abs().max(r.abs()).max((q + r).abs()) >= limit && self.frozen(q, r)))
    }

    fn frozen_extent(&self) -> f32 {
        let mut extent = 1.0f32;
        for r in -self.radius..=self.radius {
            for q in -self.radius..=self.radius {
                if self.frozen(q, r) {
                    let (x, y) = hex_position(q, r);
                    extent = extent.max(x.hypot(y));
                }
            }
        }
        extent
    }

    fn step(&mut self, diffusion: f32, sticking: f32, vapor_at: &impl Fn(i32, i32) -> f32) {
        let mut ice_vapor = vec![0.0; self.mass.len()];
        let mut ice_water = vec![0.0; self.mass.len()];
        for r in -self.radius..=self.radius {
            for q in -self.radius..=self.radius {
                if !self.inside(q, r) {
                    continue;
                }
                let index = self.index(q, r);
                let mass = self.mass[index];
                if self.receptive(q, r) {
                    ice_water[index] = mass + sticking;
                } else {
                    ice_vapor[index] = mass;
                }
            }
        }
        let mut next = self.mass.clone();
        for r in -self.radius..=self.radius {
            for q in -self.radius..=self.radius {
                if !self.inside(q, r) {
                    continue;
                }
                let index = self.index(q, r);
                if q.abs().max(r.abs()).max((q + r).abs()) == self.radius {
                    next[index] = vapor_at(q, r);
                    continue;
                }
                let own = ice_vapor[index];
                let neighbor_mean = HEX_NEIGHBORS.iter().map(|&(step_q, step_r)| if self.inside(q + step_q, r + step_r) { ice_vapor[self.index(q + step_q, r + step_r)] } else { vapor_at(q, r) }).sum::<f32>() / HEX_NEIGHBORS.len() as f32;
                next[index] = ice_water[index] + own + diffusion / 2.0 * (neighbor_mean - own);
            }
        }
        self.mass = next;
    }
}

fn hex_position(q: i32, r: i32) -> (f32, f32) {
    (q as f32 + r as f32 / 2.0, r as f32 * HEX_ROW_HEIGHT)
}

fn grow_crystal(vapor: f32, diffusion: f32, sticking: f32, unevenness: f32, radius: i32, seed: u32) -> Lattice {
    let width = (2 * radius + 1) as usize;
    let vapor_at = |q: i32, r: i32| (vapor * (1.0 + unevenness * VAPOR_NOISE_STRENGTH * (hash_unit(q, r, seed ^ BETA_NOISE_SALT) - 0.5))).clamp(0.0, 1.0);
    let mut lattice = Lattice { radius, width, mass: vec![0.0; width * width] };
    for r in -radius..=radius {
        for q in -radius..=radius {
            if lattice.inside(q, r) {
                let index = lattice.index(q, r);
                lattice.mass[index] = vapor_at(q, r);
            }
        }
    }
    let center = lattice.index(0, 0);
    lattice.mass[center] = SEED_MASS;
    for _ in 0..radius as usize * GROWTH_STEPS_PER_CELL {
        lattice.step(diffusion, sticking, &vapor_at);
        if lattice.reached_edge() {
            break;
        }
    }
    lattice
}

fn ice_at(lattice: &Lattice, x: f32, y: f32) -> f32 {
    let row = y / HEX_ROW_HEIGHT;
    let column = x - row / 2.0;
    let (base_q, base_r) = (column.floor(), row.floor());
    let (fraction_q, fraction_r) = (column - base_q, row - base_r);
    let sample = |step_q: i32, step_r: i32| lattice.ice(base_q as i32 + step_q, base_r as i32 + step_r);
    let near = sample(0, 0) + (sample(1, 0) - sample(0, 0)) * fraction_q;
    let far = sample(0, 1) + (sample(1, 1) - sample(0, 1)) * fraction_q;
    near + (far - near) * fraction_r
}

fn render_snowflake(values: &TipValues, seed: u32, side: u32) -> Result<MaskTip, BrushError> {
    let radius = values.get("detail").round() as i32;
    let lattice = grow_crystal(values.get("vapor"), values.get("diffusion"), values.get("sticking") * GAMMA_UNIT, values.get("unevenness"), radius, seed);
    let scale = lattice.frozen_extent() / FLAKE_FILL;
    rasterize(side, |x, y| {
        let ice = ice_at(&lattice, x * scale, y * scale);
        ((ice - ICE_EDGE.0) / (ICE_EDGE.1 - ICE_EDGE.0)).clamp(0.0, 1.0)
    })
}

struct Feather<'a> {
    side: u32,
    seed: u32,
    counter: i32,
    spread: f32,
    curl: f32,
    pixels: &'a mut [f32],
}

impl Feather<'_> {
    fn random(&mut self) -> f32 {
        self.counter += FROST_SALT_STRIDE;
        hash_unit(self.counter, self.counter / 3, self.seed)
    }

    fn capsule(&mut self, from: (f32, f32), to: (f32, f32), widths: (f32, f32)) {
        let side = self.side as f32;
        let to_pixels = |value: f32| (value + 1.0) / 2.0 * side;
        let reach = (widths.0.max(widths.1) * side / 2.0).ceil() as i32 + 2;
        let (low_x, high_x) = (to_pixels(from.0.min(to.0)) as i32 - reach, to_pixels(from.0.max(to.0)) as i32 + reach);
        let (low_y, high_y) = (to_pixels(from.1.min(to.1)) as i32 - reach, to_pixels(from.1.max(to.1)) as i32 + reach);
        let (edge_x, edge_y) = (to.0 - from.0, to.1 - from.1);
        let length_sq = (edge_x * edge_x + edge_y * edge_y).max(f32::EPSILON);
        for pixel_y in low_y.max(0)..=high_y.min(self.side as i32 - 1) {
            for pixel_x in low_x.max(0)..=high_x.min(self.side as i32 - 1) {
                let (x, y) = ((pixel_x as f32 + 0.5) / side * 2.0 - 1.0, (pixel_y as f32 + 0.5) / side * 2.0 - 1.0);
                let along = (((x - from.0) * edge_x + (y - from.1) * edge_y) / length_sq).clamp(0.0, 1.0);
                let distance = (x - from.0 - edge_x * along).hypot(y - from.1 - edge_y * along);
                let half_width = (widths.0 + (widths.1 - widths.0) * along) / 2.0;
                let ice = soft_edge(half_width - distance, self.side);
                let slot = &mut self.pixels[(pixel_y as u32 * self.side + pixel_x as u32) as usize];
                *slot = slot.max(ice);
            }
        }
    }

    fn grow(&mut self, start: (f32, f32), heading: f32, length: f32, width: f32, depth: u32, segments: usize) {
        let step = length / segments as f32;
        let (mut at, mut direction) = (start, heading);
        for segment in 0..segments {
            let taper = |fraction: f32| width * (1.0 - (1.0 - FROST_TIP_TAPER) * fraction);
            let (from_fraction, to_fraction) = (segment as f32 / segments as f32, (segment + 1) as f32 / segments as f32);
            direction += (self.random() - 0.5) * self.curl * FROST_JITTER_SHARE;
            let next = (at.0 + direction.cos() * step, at.1 + direction.sin() * step);
            self.capsule(at, next, (taper(from_fraction), taper(to_fraction)));
            at = next;
            if depth > 0 && segment > 0 {
                for sign in [1.0, -1.0] {
                    let side_length = length * FROST_SIDE_LENGTH_SHARE * (std::f32::consts::PI * to_fraction).sin().powf(FROST_LENGTH_FALLOFF) * (0.8 + 0.4 * self.random());
                    if side_length >= FROST_MIN_LENGTH {
                        let side_heading = direction + sign * (self.spread + (self.random() - 0.5) * self.curl);
                        let side_segments = ((segments as f32 * FROST_SEGMENT_SHARE) as usize).max(FROST_MIN_SEGMENTS);
                        self.grow(at, side_heading, side_length, width * FROST_SIDE_WIDTH_SHARE, depth - 1, side_segments);
                    }
                }
            }
        }
    }
}

fn render_frost(values: &TipValues, seed: u32, side: u32) -> Result<MaskTip, BrushError> {
    let mut pixels = vec![0.0; (side * side) as usize];
    let mut feather = Feather { side, seed, counter: 0, spread: values.get("spread"), curl: values.get("curl"), pixels: &mut pixels };
    feather.grow((0.0, FROST_STEM_REACH), -FRAC_PI_2, FROST_STEM_REACH * 2.0, FROST_STEM_WIDTH, values.get("depth").round() as u32, values.get("branches").round() as usize);
    let bytes = pixels.iter().map(|ice| (ice.clamp(0.0, 1.0) * f32::from(u8::MAX)).round() as u8).collect();
    MaskTip::new(side, side, bytes)
}
