use std::f32::consts::{FRAC_PI_2, PI, TAU};

use chequer_core::brush::{Brush, BrushSettings, BrushTip, MaskTip, hash_unit};

use super::classic::brush;
use super::shapes::{convex_inside, pixel_mask, rotated, texel_edge, unit_edge, unit_mask};
use super::texture::{fbm, salted, smooth_step, variant_set};

const DECOR_CATEGORY: &str = "decor";
const RAINDROPS_ID: &str = "nat.raindrops";
const STARS_ID: &str = "nat.starglints";
const BLOSSOMS_ID: &str = "nat.blossoms";
const HEARTS_ID: &str = "dec.hearts";
const CONFETTI_ID: &str = "dec.confetti";
const GLITTER_ID: &str = "dec.glitter";
const BURST_ID: &str = "cmx.burst";
const SPEED_LINES_ID: &str = "cmx.speed_lines";

const RAIN_SALT: u32 = 401;
const RAIN_RADII: [f32; 6] = [0.42, 0.55, 0.62, 0.7, 0.78, 0.5];
const RAIN_WOBBLE: f32 = 0.16;
const RAIN_TAIL: f32 = 0.55;
const RAIN_INNER_ALPHA: f32 = 0.2;
const RAIN_RIM_SHARE: f32 = 0.24;
const RAIN_HIGHLIGHT: (f32, f32, f32) = (-0.36, -0.34, 0.16);
const RAIN_CAUSTIC: (f32, f32, f32) = (0.26, 0.34, 0.3);

const STAR_CORE_RADIUS: f32 = 0.09;
const STAR_HALO_RADIUS: f32 = 0.38;
const STAR_HALO_ALPHA: f32 = 0.4;
const STAR_SPIKE_WIDTH: f32 = 0.014;
const STAR_SPIKE_BASE: f32 = 0.0025;
const STAR_SPIKE_FALLOFF: f32 = 1.7;
const STAR_KINDS: [(&[(f32, f32)], f32); 5] = [
    (&[(0.0, 0.96), (FRAC_PI_2, 0.96)], 0.0),
    (&[(0.0, 0.92), (PI / 3.0, 0.72), (2.0 * PI / 3.0, 0.72)], 0.0),
    (&[(0.0, 0.96), (FRAC_PI_2, 0.96), (PI / 4.0, 0.46), (3.0 * PI / 4.0, 0.46)], 0.0),
    (&[], 0.0),
    (&[(0.0, 0.9), (FRAC_PI_2, 0.74)], 0.26),
];

const BLOSSOM_KINDS: [BlossomKind; 5] = [
    BlossomKind { petals: 5, distance: 0.46, length: 0.34, width: 0.34, center: 0.14, dotted: false },
    BlossomKind { petals: 6, distance: 0.5, length: 0.36, width: 0.26, center: 0.13, dotted: false },
    BlossomKind { petals: 8, distance: 0.55, length: 0.38, width: 0.12, center: 0.15, dotted: true },
    BlossomKind { petals: 4, distance: 0.45, length: 0.35, width: 0.35, center: 0.12, dotted: false },
    BlossomKind { petals: 5, distance: 0.5, length: 0.4, width: 0.22, center: 0.1, dotted: true },
];
const BLOSSOM_DOT_GAP: f32 = 0.05;

const HEART_SCALE: f32 = 1.25;
const HEART_CENTER_LIFT: f32 = 0.1;
const HEART_KINDS: [(f32, f32, f32, f32); 4] = [(1.0, 1.0, 0.0, 0.0), (0.92, 1.06, 0.06, -0.1), (1.08, 0.94, -0.05, 0.12), (0.97, 1.02, 0.04, 0.04)];
const HEART_GLOSS: (f32, f32, f32, f32) = (-0.4, -0.34, 0.2, 0.09);
const HEART_GLOSS_TURN: f32 = -0.6;
const HEART_GLOSS_DEPTH: f32 = 0.55;

const CONFETTI_SALT: u32 = 441;
const CONFETTI_KINDS: u32 = 8;
const CONFETTI_CREASE_ALPHA: f32 = 0.8;
const CONFETTI_CURL_AMPLITUDE: f32 = 0.28;
const CONFETTI_CURL_FREQUENCY: f32 = 4.6;

const GLITTER_SALT: u32 = 461;
const GLITTER_KINDS: [(usize, f32); 6] = [(6, 0.82), (5, 0.8), (3, 0.9), (4, 0.82), (4, 0.7), (7, 0.78)];
const GLITTER_RADIUS_SPREAD: f32 = 0.16;
const GLITTER_FACET_FLOOR: f32 = 0.6;

const BURST_SALT: u32 = 481;
const BURST_KINDS: [usize; 3] = [14, 18, 11];
const BURST_OUTER: (f32, f32) = (0.8, 0.98);
const BURST_INNER: (f32, f32) = (0.46, 0.6);
const BURST_TURN_SPREAD: f32 = 0.3;

const SPEED_WIDTH_TEXELS: u32 = 256;
const SPEED_HEIGHT_TEXELS: u32 = 128;
const SPEED_LINES: i32 = 11;
const SPEED_SALT: u32 = 501;
const SPEED_LENGTH: (f32, f32) = (0.35, 1.0);
const SPEED_THICKNESS_TEXELS: (f32, f32) = (1.5, 4.5);
const SPEED_TAPER: f32 = 0.8;

pub fn decor_brushes() -> Vec<Brush> {
    let hard = BrushSettings { hardness: 1.0, pressure_size: 0.3, ..BrushSettings::default() };
    vec![
        brush(RAINDROPS_ID, DECOR_CATEGORY, BrushSettings { spacing: 1.4, scatter: 1.0, size_jitter: 0.6, angle_jitter: 0.03, opacity_jitter: 0.2, ..hard.clone() }, variant_set(RAIN_RADII.len() as u32, raindrop)),
        brush(STARS_ID, DECOR_CATEGORY, BrushSettings { spacing: 1.3, scatter: 1.2, size_jitter: 0.7, angle_jitter: 0.1, opacity_jitter: 0.4, ..hard.clone() }, variant_set(STAR_KINDS.len() as u32, star_glint)),
        brush(BLOSSOMS_ID, DECOR_CATEGORY, BrushSettings { spacing: 1.2, scatter: 0.6, size_jitter: 0.35, angle_jitter: 1.0, ..hard.clone() }, variant_set(BLOSSOM_KINDS.len() as u32, blossom)),
        brush(HEARTS_ID, DECOR_CATEGORY, BrushSettings { spacing: 1.3, scatter: 0.5, size_jitter: 0.35, angle_jitter: 0.06, ..hard.clone() }, variant_set(HEART_KINDS.len() as u32, heart)),
        brush(CONFETTI_ID, DECOR_CATEGORY, BrushSettings { spacing: 0.9, scatter: 1.4, size_jitter: 0.5, angle_jitter: 1.0, hue_jitter: 1.0, ..hard.clone() }, variant_set(CONFETTI_KINDS, confetti)),
        brush(GLITTER_ID, DECOR_CATEGORY, BrushSettings { spacing: 0.35, scatter: 1.8, size_jitter: 0.8, angle_jitter: 1.0, opacity_jitter: 0.35, value_jitter: 0.12, hue_jitter: 0.04, ..hard.clone() }, variant_set(GLITTER_KINDS.len() as u32, glitter)),
        brush(BURST_ID, DECOR_CATEGORY, BrushSettings { spacing: 1.4, angle_jitter: 1.0, size_jitter: 0.2, ..hard.clone() }, variant_set(BURST_KINDS.len() as u32, burst)),
        brush(SPEED_LINES_ID, DECOR_CATEGORY, BrushSettings { spacing: 0.8, follow_direction: true, pressure_size: 0.0, ..hard }, BrushTip::Mask(speed_lines())),
    ]
}

fn raindrop(variant: u32) -> MaskTip {
    let seed = salted(RAIN_SALT, variant);
    let radius = RAIN_RADII[variant as usize];
    let tail = hash_unit(variant as i32, 1, seed) * RAIN_TAIL;
    unit_mask(|x, y| {
        let stretched = if y < 0.0 { y / (1.0 + tail) } else { y };
        let angle = stretched.atan2(x);
        let wobble = (fbm(angle.cos() * 1.6 + 7.0, angle.sin() * 1.6 + 7.0, 3, seed) - 0.5) * RAIN_WOBBLE;
        let depth = radius * (1.0 + wobble) - x.hypot(stretched);
        let rim = 1.0 - smooth_step(0.0, RAIN_RIM_SHARE * radius, depth);
        let body = (RAIN_INNER_ALPHA + (1.0 - RAIN_INNER_ALPHA) * rim) * unit_edge(depth);
        let spot = |(spot_x, spot_y, spot_radius): (f32, f32, f32)| smooth_step(spot_radius * radius, 0.35 * spot_radius * radius, (x - spot_x * radius).hypot(y - spot_y * radius));
        let shining = body * (1.0 - 0.85 * spot(RAIN_HIGHLIGHT));
        shining.max(unit_edge(depth) * 0.7 * spot(RAIN_CAUSTIC))
    })
}

fn star_glint(variant: u32) -> MaskTip {
    let (spikes, tilt) = STAR_KINDS[variant as usize];
    unit_mask(|x, y| {
        let distance = x.hypot(y);
        let core = 1.0 - smooth_step(0.0, STAR_CORE_RADIUS, distance);
        let halo = STAR_HALO_ALPHA * (1.0 - smooth_step(0.0, STAR_HALO_RADIUS, distance)).powi(2);
        let spike = |&(angle, reach): &(f32, f32)| {
            let (along, across) = rotated(x, y, angle + tilt);
            let remaining = 1.0 - along.abs() / reach;
            if remaining <= 0.0 {
                return 0.0;
            }
            let width = STAR_SPIKE_BASE + STAR_SPIKE_WIDTH * remaining.powf(STAR_SPIKE_FALLOFF);
            remaining.powf(STAR_SPIKE_FALLOFF) * (-(across / width).powi(2)).exp()
        };
        spikes.iter().map(spike).fold(core.max(halo), f32::max)
    })
}

struct BlossomKind {
    petals: u32,
    distance: f32,
    length: f32,
    width: f32,
    center: f32,
    dotted: bool,
}

fn blossom(variant: u32) -> MaskTip {
    let kind = &BLOSSOM_KINDS[variant as usize];
    unit_mask(|x, y| {
        let petals = (0..kind.petals)
            .map(|petal| {
                let angle = petal as f32 / kind.petals as f32 * TAU - FRAC_PI_2;
                let (along, across) = rotated(x - angle.cos() * kind.distance, y - angle.sin() * kind.distance, angle);
                unit_edge((1.0 - (along / kind.length).hypot(across / kind.width)) * kind.width.min(kind.length))
            })
            .fold(0.0, f32::max);
        let distance = x.hypot(y);
        if kind.dotted {
            let clear = unit_edge(distance - kind.center - BLOSSOM_DOT_GAP);
            (petals * clear).max(unit_edge(kind.center - distance))
        } else {
            petals * unit_edge(distance - kind.center)
        }
    })
}

fn heart(variant: u32) -> MaskTip {
    let (stretch_x, stretch_y, shear, turn) = HEART_KINDS[variant as usize];
    let (gloss_x, gloss_y, gloss_long, gloss_short) = HEART_GLOSS;
    unit_mask(|x, y| {
        let (turned_x, turned_y) = rotated(x, y, turn);
        let u = (turned_x / stretch_x + shear * turned_y) * HEART_SCALE;
        let w = (-turned_y / stretch_y) * HEART_SCALE + HEART_CENTER_LIFT;
        let (square_sum, cubed) = (u * u + w * w - 1.0, w * w * w);
        let field = square_sum.powi(3) - u * u * cubed;
        let slope_u = 6.0 * u * square_sum.powi(2) - 2.0 * u * cubed;
        let slope_w = 6.0 * w * square_sum.powi(2) - 3.0 * u * u * w * w;
        let inside = -field / slope_u.hypot(slope_w).max(f32::EPSILON) / HEART_SCALE;
        let (along, across) = rotated(x - gloss_x, y - gloss_y, HEART_GLOSS_TURN);
        let gloss = (1.0 - smooth_step(0.6, 1.0, (along / gloss_long).hypot(across / gloss_short))).clamp(0.0, 1.0);
        unit_edge(inside) * (1.0 - HEART_GLOSS_DEPTH * gloss)
    })
}

fn confetti(variant: u32) -> MaskTip {
    let seed = salted(CONFETTI_SALT, variant);
    let turn = hash_unit(variant as i32, 1, seed) * PI;
    let crease = |along: f32, across: f32, alpha: f32| if along + 0.35 * across > 0.1 { alpha * CONFETTI_CREASE_ALPHA } else { alpha };
    unit_mask(|x, y| {
        let (along, across) = rotated(x, y, turn);
        match variant {
            0 => crease(along, across, unit_edge((0.82 - along.abs()).min(0.2 - across.abs()))),
            1 => crease(along, across, unit_edge((0.85 - along.abs()).min(0.12 - across.abs()))),
            2 => unit_edge(0.5 - x.hypot(y)),
            3 => crease(along, across, unit_edge((0.36 - along.abs()).min(0.3 - across.abs()))),
            4 => {
                let vertices: Vec<(f32, f32)> = (0..3).map(|corner| ((corner as f32 / 3.0 * TAU).cos() * 0.62, (corner as f32 / 3.0 * TAU).sin() * 0.62)).collect();
                unit_edge(convex_inside(along, across, &vertices))
            }
            5 => unit_edge((0.12 - (across - CONFETTI_CURL_AMPLITUDE * (along * CONFETTI_CURL_FREQUENCY).sin()).abs()).min(0.85 - along.abs())),
            6 => unit_edge((0.5 - along.abs()).min(0.045 - across.abs())),
            _ => crease(along, across, unit_edge((0.7 - along.abs()).min(0.26 - across.abs()))),
        }
    })
}

fn glitter(variant: u32) -> MaskTip {
    let (corners, radius) = GLITTER_KINDS[variant as usize];
    let seed = salted(GLITTER_SALT, variant);
    let turn = hash_unit(variant as i32, 1, seed) * TAU;
    let vertices: Vec<(f32, f32)> = (0..corners)
        .map(|corner| {
            let angle = turn + corner as f32 / corners as f32 * TAU;
            let reach = radius * (1.0 + (hash_unit(corner as i32, 2, seed) - 0.5) * 2.0 * GLITTER_RADIUS_SPREAD);
            (angle.cos() * reach, angle.sin() * reach)
        })
        .collect();
    unit_mask(|x, y| {
        let facet = ((y.atan2(x) - turn).rem_euclid(TAU) / TAU * corners as f32) as i32;
        let shine = GLITTER_FACET_FLOOR + (1.0 - GLITTER_FACET_FLOOR) * hash_unit(facet, 3, seed);
        unit_edge(convex_inside(x, y, &vertices)) * shine
    })
}

fn burst(variant: u32) -> MaskTip {
    let spikes = BURST_KINDS[variant as usize];
    let seed = salted(BURST_SALT, variant);
    let vertices: Vec<(f32, f32)> = (0..spikes * 2)
        .map(|index| {
            let outer = index % 2 == 0;
            let (low, high) = if outer { BURST_OUTER } else { BURST_INNER };
            let reach = low + (high - low) * hash_unit(index as i32, 1, seed);
            let angle = (index as f32 + (hash_unit(index as i32, 2, seed) - 0.5) * BURST_TURN_SPREAD) / (spikes * 2) as f32 * TAU;
            (angle.cos() * reach, angle.sin() * reach)
        })
        .collect();
    unit_mask(|x, y| {
        let distance = x.hypot(y);
        let angle = y.atan2(x).rem_euclid(TAU);
        let direction = (x / distance.max(f32::EPSILON), y / distance.max(f32::EPSILON));
        let edges = vertices.len();
        let sector = (0..edges)
            .find(|&index| {
                let (from, to) = (vertices[index].1.atan2(vertices[index].0).rem_euclid(TAU), vertices[(index + 1) % edges].1.atan2(vertices[(index + 1) % edges].0).rem_euclid(TAU));
                if from <= to { angle >= from && angle < to } else { angle >= from || angle < to }
            })
            .unwrap_or(edges - 1);
        let (from, to) = (vertices[sector], vertices[(sector + 1) % edges]);
        let edge = (to.0 - from.0, to.1 - from.1);
        let slope = direction.0 * edge.1 - direction.1 * edge.0;
        let boundary = (from.0 * to.1 - from.1 * to.0) / slope;
        unit_edge((boundary - distance) * slope.abs() / edge.0.hypot(edge.1))
    })
}

fn speed_lines() -> MaskTip {
    let (width, height) = (SPEED_WIDTH_TEXELS as f32, SPEED_HEIGHT_TEXELS as f32);
    let lines: Vec<(f32, f32, f32, f32)> = (0..SPEED_LINES)
        .map(|index| {
            let row = (index as f32 + 0.5 + (hash_unit(index, 1, SPEED_SALT) - 0.5) * 0.6) / SPEED_LINES as f32 * height;
            let length = (SPEED_LENGTH.0 + (SPEED_LENGTH.1 - SPEED_LENGTH.0) * hash_unit(index, 2, SPEED_SALT)) * width;
            let head = width * (0.55 + 0.45 * hash_unit(index, 3, SPEED_SALT)).max(length / width);
            let thickness = SPEED_THICKNESS_TEXELS.0 + (SPEED_THICKNESS_TEXELS.1 - SPEED_THICKNESS_TEXELS.0) * hash_unit(index, 4, SPEED_SALT);
            (row, head - length, head, thickness)
        })
        .collect();
    pixel_mask(SPEED_WIDTH_TEXELS, SPEED_HEIGHT_TEXELS, |x, y| {
        lines
            .iter()
            .map(|&(row, tail, head, thickness)| {
                let progress = ((x - tail) / (head - tail)).clamp(0.0, 1.0);
                let half = thickness / 2.0 * progress.powf(SPEED_TAPER);
                texel_edge(half - (y - row).abs()).min(texel_edge(head - x)).min(if x < tail { 0.0 } else { 1.0 })
            })
            .fold(0.0, f32::max)
    })
}
