
use super::params::{choice, float, int, angle, center, flag};
use super::util::{
    Cancel, Edge, Plane, bilinear, box_blur, center_of, clamp_channel, gaussian_blur, map_area, mean, par_rows, percentile_filter, read_region, texel,
};
use super::{Category, EffectDef, Job, ids};
use crate::raster::Raster;

const EDGES: &[&str] = &["clamp", "wrap", "mirror", "transparent"];
const RADIUS_PER_SIGMA: f32 = 2.0;
const MAX_LINE_SAMPLES: f32 = 256.0;
const MIN_LINE_SAMPLES: f32 = 3.0;

pub static DEFS: [EffectDef; 10] = [
    EffectDef { id: ids::BOKEH_BLUR, category: Category::Blurs, params: &[int("radius", 1, 200, 10), float("gamma", 0.5, 4.0, 2.2, 0.1)], render: bokeh },
    EffectDef {
        id: ids::FRAGMENT_BLUR,
        category: Category::Blurs,
        params: &[int("fragments", 2, 50, 4), int("distance", 0, 100, 8), angle("rotation", 0.0, 360.0, 0.0)],
        render: fragment,
    },
    EffectDef { id: ids::GAUSSIAN_BLUR, category: Category::Blurs, params: &[float("radius", 0.0, 200.0, 10.0, 0.5), float("gamma", 0.5, 4.0, 2.2, 0.1)], render: gaussian },
    EffectDef { id: ids::MEDIAN_BLUR, category: Category::Blurs, params: &[int("radius", 1, 200, 10), int("percentile", 0, 100, 50)], render: median },
    EffectDef {
        id: ids::MOTION_BLUR,
        category: Category::Blurs,
        params: &[angle("angle", -180.0, 180.0, 25.0), int("distance", 1, 200, 10), flag("centered", true), choice("edge", EDGES, 0)],
        render: motion,
    },
    EffectDef { id: ids::RADIAL_BLUR, category: Category::Blurs, params: &[angle("angle", 0.0, 360.0, 2.0), center("center"), float("quality", 1.0, 8.0, 2.0, 0.1)], render: radial },
    EffectDef {
        id: ids::SKETCH_BLUR,
        category: Category::Blurs,
        params: &[int("radius", 1, 200, 10), int("percentile", 0, 100, 50), int("smoothness", 1, 10, 3)],
        render: sketch,
    },
    EffectDef { id: ids::SQUARE_BLUR, category: Category::Blurs, params: &[int("radius", 1, 200, 10), float("gamma", 0.5, 4.0, 2.2, 0.1)], render: square },
    EffectDef { id: ids::SURFACE_BLUR, category: Category::Blurs, params: &[int("radius", 1, 100, 6), int("threshold", 1, 100, 15)], render: surface },
    EffectDef { id: ids::ZOOM_BLUR, category: Category::Blurs, params: &[int("distance", 0, 100, 10), center("center"), float("quality", 1.0, 8.0, 2.0, 0.1)], render: zoom },
];

fn gaussian(job: &Job) -> Option<Raster> {
    let (radius, gamma) = (job.params.float("radius"), job.params.float("gamma"));
    let sigma = radius / RADIUS_PER_SIGMA;
    let (region, inner) = read_region(job.source, job.area, (sigma * 3.0).ceil() as i32);
    let blurred = gaussian_blur(Plane::from_raster(job.source, region, gamma), sigma, job.cancel)?;
    Some(blurred.to_raster(inner, gamma))
}

fn square(job: &Job) -> Option<Raster> {
    let (radius, gamma) = (job.params.int("radius"), job.params.float("gamma"));
    let (region, inner) = read_region(job.source, job.area, radius);
    let blurred = box_blur(Plane::from_raster(job.source, region, gamma), radius as usize, job.cancel)?;
    Some(blurred.to_raster(inner, gamma))
}

fn bokeh(job: &Job) -> Option<Raster> {
    let (radius, gamma) = (job.params.int("radius"), job.params.float("gamma"));
    let (region, inner) = read_region(job.source, job.area, radius);
    let blurred = disc_blur(&Plane::from_raster(job.source, region, gamma), radius, job.cancel)?;
    Some(blurred.to_raster(inner, gamma))
}

const CHANNELS: usize = 4;

fn disc_blur(plane: &Plane, radius: i32, cancel: &Cancel) -> Option<Plane> {
    let (width, height) = (plane.width, plane.height);
    let prefix_len = (width + 1) * CHANNELS;
    let mut prefix = vec![0.0f64; prefix_len * height];
    for (y, row) in prefix.chunks_exact_mut(prefix_len).enumerate() {
        for x in 0..width {
            for c in 0..CHANNELS {
                row[(x + 1) * CHANNELS + c] = row[x * CHANNELS + c] + f64::from(plane.data[(y * width + x) * CHANNELS + c]);
            }
        }
    }
    let half_widths: Vec<i32> = (-radius..=radius).map(|dy| ((radius * radius - dy * dy) as f32 + 0.5).sqrt().floor() as i32).collect();
    let mut data = vec![0.0f32; plane.data.len()];
    let done = par_rows(&mut data, width * CHANNELS, cancel, |y, out| {
        for x in 0..width {
            let mut sum = [0.0f64; CHANNELS];
            let mut count = 0.0f64;
            for (offset, &half) in half_widths.iter().enumerate() {
                let row = y as i32 + offset as i32 - radius;
                if row < 0 || row >= height as i32 {
                    continue;
                }
                let left = (x as i32 - half).max(0) as usize;
                let right = ((x as i32 + half) as usize).min(width - 1);
                let base = row as usize * prefix_len;
                for c in 0..CHANNELS {
                    sum[c] += prefix[base + (right + 1) * CHANNELS + c] - prefix[base + left * CHANNELS + c];
                }
                count += (right + 1 - left) as f64;
            }
            for c in 0..CHANNELS {
                out[x * CHANNELS + c] = (sum[c] / count) as f32;
            }
        }
    });
    done.then_some(Plane { width, height, data })
}

fn median(job: &Job) -> Option<Raster> {
    percentile_filter(job.source, job.area, job.params.int("radius"), job.params.int("percentile") as f32, job.cancel)
}

fn sketch(job: &Job) -> Option<Raster> {
    let (radius, smoothness) = (job.params.int("radius"), job.params.int("smoothness"));
    let picked = percentile_filter(job.source, job.area, radius, job.params.int("percentile") as f32, job.cancel)?;
    let sigma = smoothness as f32 / RADIUS_PER_SIGMA;
    let blurred = gaussian_blur(Plane::from_raster(&picked, picked.bounds(), 1.0), sigma, job.cancel)?;
    Some(blurred.to_raster(picked.bounds(), 1.0))
}

fn motion(job: &Job) -> Option<Raster> {
    let direction = job.params.float("angle").to_radians();
    let distance = job.params.int("distance") as f32;
    let (edge, centered) = (job.params.edge("edge"), job.params.bool("centered"));
    let (step_x, step_y) = (direction.cos(), -direction.sin());
    let samples = (distance.ceil() as i32).max(2);
    let start = if centered { -distance / 2.0 } else { 0.0 };
    map_area(job.area, job.cancel, |x, y| {
        mean((0..samples).map(|i| {
            let along = start + distance * i as f32 / (samples - 1) as f32;
            bilinear(job.source, x as f32 + step_x * along, y as f32 + step_y * along, edge)
        }))
    })
}

fn fragment(job: &Job) -> Option<Raster> {
    let count = job.params.int("fragments");
    let distance = job.params.int("distance") as f32;
    let rotation = job.params.float("rotation").to_radians();
    let offsets: Vec<(f32, f32)> = (0..count)
        .map(|i| {
            let theta = rotation + std::f32::consts::TAU * i as f32 / count as f32;
            (theta.cos() * distance, -theta.sin() * distance)
        })
        .collect();
    map_area(job.area, job.cancel, |x, y| mean(offsets.iter().map(|&(dx, dy)| bilinear(job.source, x as f32 + dx, y as f32 + dy, Edge::Clamp))))
}

fn path_samples(length: f32, quality: f32) -> i32 {
    (length * quality / 2.0).clamp(MIN_LINE_SAMPLES, MAX_LINE_SAMPLES).ceil() as i32
}

fn half_diagonal(area: crate::geom::IRect) -> f32 {
    let (w, h) = (area.width() as f32, area.height() as f32);
    (w * w + h * h).sqrt() / 2.0
}

fn radial(job: &Job) -> Option<Raster> {
    let total = job.params.float("angle").to_radians();
    let (cx, cy) = center_of(job.area, job.params.point("center"));
    let samples = path_samples(total * half_diagonal(job.area), job.params.float("quality"));
    map_area(job.area, job.cancel, |x, y| {
        let (dx, dy) = (x as f32 - cx, y as f32 - cy);
        mean((0..samples).map(|i| {
            let theta = (i as f32 / (samples - 1) as f32 - 0.5) * total;
            let (sin, cos) = theta.sin_cos();
            bilinear(job.source, cx + dx * cos - dy * sin, cy + dx * sin + dy * cos, Edge::Clamp)
        }))
    })
}

fn zoom(job: &Job) -> Option<Raster> {
    let amount = job.params.int("distance") as f32 / 100.0;
    let (cx, cy) = center_of(job.area, job.params.point("center"));
    let samples = path_samples(amount * half_diagonal(job.area), job.params.float("quality"));
    map_area(job.area, job.cancel, |x, y| {
        let (dx, dy) = (x as f32 - cx, y as f32 - cy);
        mean((0..samples).map(|i| {
            let scale = 1.0 - amount * i as f32 / (samples - 1) as f32;
            bilinear(job.source, cx + dx * scale, cy + dy * scale, Edge::Clamp)
        }))
    })
}

fn surface(job: &Job) -> Option<Raster> {
    let radius = job.params.int("radius");
    let tolerance = job.params.int("threshold") as f32 * 2.55;
    let reach_squared = (radius * radius) as f32;
    map_area(job.area, job.cancel, |x, y| {
        let centre = job.source.get(x as u32, y as u32);
        let (mut sum, mut total) = ([0.0f32; CHANNELS], 0.0f32);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if (dx * dx + dy * dy) as f32 > reach_squared {
                    continue;
                }
                let Some(pixel) = texel(job.source, x + dx, y + dy, Edge::Clamp) else { continue };
                let difference: f32 = (0..3).map(|c| (f32::from(pixel[c]) - f32::from(centre[c])).abs()).sum::<f32>() / 3.0;
                let weight = (1.0 - difference / tolerance).max(0.0);
                for c in 0..CHANNELS {
                    sum[c] += weight * f32::from(pixel[c]);
                }
                total += weight;
            }
        }
        sum.map(|s| clamp_channel(s / total))
    })
}
