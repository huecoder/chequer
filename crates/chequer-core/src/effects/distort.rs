
use super::params::{angle, center, choice, flag, float, int, seed};
use super::util::{
    Edge, bicubic, bilinear, center_of, fractal_noise, hash_unit, map_area, map_supersampled, mean, render_rows, texel, window_extreme,
};
use super::{Category, EffectDef, Job, ids};
use crate::raster::{Raster, Rgba};

const EDGES: &[&str] = &["clamp", "wrap", "mirror", "transparent"];
const POLAR_EDGES: &[&str] = &["clamp", "mirror", "wrap"];
const MORPH_MODES: &[&str] = &["erode", "dilate"];
const SCALE_UP: &[&str] = &["nearest", "bilinear", "bicubic"];
const CELL_Y_SALT: u32 = 0x9e37_79b9;
const BULGE_UNITS: f32 = 200.0;
const TWIST_TURNS_PER_100: f32 = 1.0;
const CURVATURE_UNITS: f32 = 20.0;
const NOISE_OCTAVE_STEP: i32 = 15;
const Y_NOISE_SEED_SHIFT: u32 = 97;

pub static DEFS: [EffectDef; 10] = [
    EffectDef {
        id: ids::CHROMATIC_ABERRATION,
        category: Category::Distort,
        params: &[int("distance", 0, 100, 4), angle("angle", -180.0, 180.0, 0.0)],
        render: chromatic_aberration,
    },
    EffectDef {
        id: ids::BULGE,
        category: Category::Distort,
        params: &[float("amount", -200.0, 100.0, 45.0, 1.0), center("center"), choice("edge", EDGES, 0), int("quality", 1, 5, 2)],
        render: bulge,
    },
    EffectDef { id: ids::CRYSTALIZE, category: Category::Distort, params: &[int("cell_size", 2, 100, 10), int("quality", 1, 5, 2), seed("seed")], render: crystalize },
    EffectDef {
        id: ids::DENTS,
        category: Category::Distort,
        params: &[
            int("scale", 1, 200, 25),
            int("refraction", 0, 200, 50),
            int("detail", 0, 100, 10),
            int("turbulence", 0, 100, 10),
            choice("edge", EDGES, 0),
            int("quality", 1, 5, 2),
            seed("seed"),
        ],
        render: dents,
    },
    EffectDef {
        id: ids::FROSTED_GLASS,
        category: Category::Distort,
        params: &[int("max_scatter", 0, 200, 3), int("min_scatter", 0, 200, 0), int("diffusion", 1, 8, 2), int("smoothness", 1, 5, 2), seed("seed")],
        render: frosted_glass,
    },
    EffectDef {
        id: ids::MORPHOLOGY,
        category: Category::Distort,
        params: &[choice("mode", MORPH_MODES, 0), int("width", 1, 100, 3), int("height", 1, 100, 3), flag("linked", true)],
        render: morphology,
    },
    EffectDef { id: ids::PIXELATE, category: Category::Distort, params: &[int("cell_size", 1, 100, 2), choice("scale_up", SCALE_UP, 0)], render: pixelate },
    EffectDef {
        id: ids::POLAR_INVERSION,
        category: Category::Distort,
        params: &[float("amount", -4.0, 4.0, 1.0, 0.05), center("center"), choice("edge", POLAR_EDGES, 2), int("quality", 1, 5, 2)],
        render: polar_inversion,
    },
    EffectDef {
        id: ids::TILE_REFLECTION,
        category: Category::Distort,
        params: &[angle("angle", -180.0, 180.0, 30.0), int("tile_size", 1, 800, 40), int("curvature", -100, 100, 8), int("quality", 1, 5, 2), choice("edge", EDGES, 0)],
        render: tile_reflection,
    },
    EffectDef {
        id: ids::TWIST,
        category: Category::Distort,
        params: &[float("amount", -200.0, 200.0, 30.0, 1.0), float("size", 0.01, 2.0, 1.0, 0.01), center("center"), int("quality", 1, 5, 2)],
        render: twist,
    },
];

fn half_extent(job: &Job) -> (f32, f32) {
    (job.area.width() as f32 / 2.0, job.area.height() as f32 / 2.0)
}

fn bulge(job: &Job) -> Option<Raster> {
    let amount = job.params.float("amount") / BULGE_UNITS;
    let (cx, cy) = center_of(job.area, job.params.point("center"));
    let (half_w, half_h) = half_extent(job);
    let reach = half_w.min(half_h);
    let edge = job.params.edge("edge");
    map_supersampled(job.area, job.params.int("quality"), job.cancel, |x, y| {
        let (u, v) = (x - cx, y - cy);
        let falloff = 1.0 - (u * u + v * v).sqrt() / reach;
        if falloff <= 0.0 {
            return bilinear(job.source, x, y, edge);
        }
        let scale = 1.0 - amount * falloff * falloff;
        bilinear(job.source, cx + u * scale, cy + v * scale, edge)
    })
}

fn twist(job: &Job) -> Option<Raster> {
    let amount = job.params.float("amount") / 100.0 * TWIST_TURNS_PER_100 * std::f32::consts::TAU;
    let (cx, cy) = center_of(job.area, job.params.point("center"));
    let (half_w, half_h) = half_extent(job);
    let reach = half_w.min(half_h) * job.params.float("size");
    map_supersampled(job.area, job.params.int("quality"), job.cancel, |x, y| {
        let (u, v) = (x - cx, y - cy);
        let falloff = 1.0 - (u * u + v * v).sqrt() / reach;
        if falloff <= 0.0 {
            return bilinear(job.source, x, y, Edge::Clamp);
        }
        let (sin, cos) = (amount * falloff * falloff).sin_cos();
        bilinear(job.source, cx + u * cos - v * sin, cy + u * sin + v * cos, Edge::Clamp)
    })
}

fn polar_inversion(job: &Job) -> Option<Raster> {
    let amount = job.params.float("amount");
    let (cx, cy) = center_of(job.area, job.params.point("center"));
    let (half_w, half_h) = half_extent(job);
    let circle_squared = half_w.max(half_h).powi(2);
    let edge = job.params.edge("edge");
    map_supersampled(job.area, job.params.int("quality"), job.cancel, |x, y| {
        let (u, v) = (x - cx, y - cy);
        let magnitude = (u * u + v * v).max(f32::EPSILON);
        let factor = 1.0 + (circle_squared / magnitude - 1.0) * amount;
        bilinear(job.source, cx + u * factor, cy + v * factor, edge)
    })
}

fn tile_reflection(job: &Job) -> Option<Raster> {
    let theta = job.params.float("angle").to_radians();
    let (sin, cos) = theta.sin_cos();
    let tile = job.params.int("tile_size") as f32;
    let curvature = job.params.int("curvature") as f32 / CURVATURE_UNITS;
    let (cx, cy) = center_of(job.area, (0.0, 0.0));
    let edge = job.params.edge("edge");
    map_supersampled(job.area, job.params.int("quality"), job.cancel, |x, y| {
        let (dx, dy) = (x - cx, y - cy);
        let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
        let local = |value: f32| (value + tile / 2.0).rem_euclid(tile) - tile / 2.0;
        let (local_u, local_v) = (local(u), local(v));
        let rho_squared = ((local_u * local_u + local_v * local_v) / (tile * tile / 4.0)).min(1.0);
        let factor = 1.0 - curvature * (1.0 - rho_squared);
        let (source_u, source_v) = (u - local_u + local_u * factor, v - local_v + local_v * factor);
        bilinear(job.source, cx + source_u * cos - source_v * sin, cy + source_u * sin + source_v * cos, edge)
    })
}

fn dents(job: &Job) -> Option<Raster> {
    let scale = job.params.int("scale") as f32;
    let refraction = job.params.int("refraction") as f32;
    let octaves = 1 + (job.params.int("detail") / NOISE_OCTAVE_STEP) as u32;
    let persistence = 0.3 + job.params.int("turbulence") as f32 / 200.0;
    let (noise_seed, edge) = (job.params.seed("seed"), job.params.edge("edge"));
    map_supersampled(job.area, job.params.int("quality"), job.cancel, |x, y| {
        let shift = |seed: u32| (fractal_noise(seed, x / scale, y / scale, octaves, persistence) - 0.5) * refraction;
        bilinear(job.source, x + shift(noise_seed), y + shift(noise_seed.wrapping_add(Y_NOISE_SEED_SHIFT)), edge)
    })
}

fn crystalize(job: &Job) -> Option<Raster> {
    let size = job.params.int("cell_size") as f32;
    let cell_seed = job.params.seed("seed");
    let site = |column: i32, row: i32| {
        ((column as f32 + hash_unit(cell_seed, column, row)) * size, (row as f32 + hash_unit(cell_seed ^ CELL_Y_SALT, column, row)) * size)
    };
    map_supersampled(job.area, job.params.int("quality"), job.cancel, |x, y| {
        let (column, row) = ((x / size).floor() as i32, (y / size).floor() as i32);
        let mut nearest = (f32::MAX, (x, y));
        for j in row - 1..=row + 1 {
            for i in column - 1..=column + 1 {
                let (sx, sy) = site(i, j);
                let distance = (sx - x).powi(2) + (sy - y).powi(2);
                if distance < nearest.0 {
                    nearest = (distance, (sx, sy));
                }
            }
        }
        texel(job.source, nearest.1.0.floor() as i32, nearest.1.1.floor() as i32, Edge::Clamp).unwrap_or_default()
    })
}

fn frosted_glass(job: &Job) -> Option<Raster> {
    let (low, high) = {
        let (a, b) = (job.params.int("min_scatter") as f32, job.params.int("max_scatter") as f32);
        (a.min(b), a.max(b))
    };
    let samples = job.params.int("diffusion") * job.params.int("smoothness");
    let glass_seed = job.params.seed("seed");
    map_area(job.area, job.cancel, |x, y| {
        mean((0..samples).map(|k| {
            let radius = low + (high - low) * hash_unit(glass_seed.wrapping_add(k as u32), x, y);
            let theta = std::f32::consts::TAU * hash_unit(glass_seed.wrapping_add(k as u32 + 1_000), y, x);
            let at = (x + (radius * theta.cos()).round() as i32, y + (radius * theta.sin()).round() as i32);
            texel(job.source, at.0, at.1, Edge::Clamp).unwrap_or_default()
        }))
    })
}

fn morphology(job: &Job) -> Option<Raster> {
    let width = job.params.int("width");
    let height = if job.params.bool("linked") { width } else { job.params.int("height") };
    window_extreme(job.source, job.area, (width / 2, height / 2), job.params.choice_key("mode") == "dilate", job.cancel)
}

fn pixelate(job: &Job) -> Option<Raster> {
    let size = job.params.int("cell_size");
    let first_cell = (job.area.x0.div_euclid(size) - 1, job.area.y0.div_euclid(size) - 1);
    let last_cell = (job.area.x1.div_euclid(size) + 1, job.area.y1.div_euclid(size) + 1);
    let cells = render_rows((last_cell.0 - first_cell.0 + 1) as u32, (last_cell.1 - first_cell.1 + 1) as u32, job.cancel, |row, out| {
        let top = (first_cell.1 + row as i32) * size;
        for (column, pixel) in out.chunks_exact_mut(4).enumerate() {
            let left = (first_cell.0 + column as i32) * size;
            pixel.copy_from_slice(&cell_mean(job.source, left, top, size));
        }
    })?;
    let method = job.params.choice("scale_up");
    map_area(job.area, job.cancel, |x, y| {
        let position = |value: i32, first: i32| (value as f32 + 0.5) / size as f32 - 0.5 - first as f32;
        let (u, v) = (position(x, first_cell.0), position(y, first_cell.1));
        match method {
            0 => cells.get(u.round() as u32, v.round() as u32),
            1 => bilinear(&cells, u, v, Edge::Clamp),
            _ => bicubic(&cells, u, v, Edge::Clamp),
        }
    })
}

fn cell_mean(source: &Raster, left: i32, top: i32, size: i32) -> Rgba {
    let bounds = source.bounds();
    let cell = crate::geom::IRect::new(left, top, left + size, top + size).intersect(bounds);
    if cell.is_empty() {
        return texel(source, left, top, Edge::Clamp).unwrap_or_default();
    }
    mean((cell.y0..cell.y1).flat_map(|y| (cell.x0..cell.x1).map(move |x| (x, y))).map(|(x, y)| source.get(x as u32, y as u32)))
}

fn chromatic_aberration(job: &Job) -> Option<Raster> {
    let distance = job.params.int("distance");
    let radians = job.params.float("angle").to_radians();
    let (dx, dy) = ((radians.cos() * distance as f32).round() as i32, (radians.sin() * distance as f32).round() as i32);
    let shifted = |x: i32, y: i32, sign: i32| texel(job.source, x + dx * sign, y + dy * sign, Edge::Clamp).unwrap_or_default();
    map_area(job.area, job.cancel, |x, y| {
        let (red, green, blue) = (shifted(x, y, 1), job.source.get(x as u32, y as u32), shifted(x, y, -1));
        [red[0], green[1], blue[2], red[3].max(green[3]).max(blue[3])]
    })
}
