
use super::params::int;
use super::util::{Edge, Plane, clamp_channel, gaussian_blur, intensity, map_area, mix, render_rows, texel};
use super::{Category, EffectDef, Job, ids};
use crate::raster::Raster;

const MAX_LEVEL: f32 = 255.0;
const LEVELS_PER_PERCENT: f32 = 2.55;
const INK_GAIN_BASE: f32 = 0.5;
const INK_GAIN_PER_UNIT: f32 = 1.0 / 33.0;
const WHITE: [u8; 4] = [255, 255, 255, 255];
const BUCKETS: usize = 256;

pub static DEFS: [EffectDef; 3] = [
    EffectDef { id: ids::INK_SKETCH, category: Category::Artistic, params: &[int("ink_outline", 0, 99, 50), int("coloring", 0, 100, 50)], render: ink_sketch },
    EffectDef { id: ids::OIL_PAINTING, category: Category::Artistic, params: &[int("brush_size", 1, 8, 3), int("coarseness", 3, 255, 50)], render: oil_painting },
    EffectDef { id: ids::PENCIL_SKETCH, category: Category::Artistic, params: &[int("tip_size", 1, 20, 2), int("range", -20, 20, 0)], render: pencil_sketch },
];

fn ink_sketch(job: &Job) -> Option<Raster> {
    let gain = INK_GAIN_BASE + job.params.int("ink_outline") as f32 * INK_GAIN_PER_UNIT;
    let coloring = job.params.int("coloring") as f32 / 100.0;
    let level = |x: i32, y: i32| f32::from(intensity(texel(job.source, x, y, Edge::Clamp).unwrap_or_default()));
    map_area(job.area, job.cancel, |x, y| {
        let gx = level(x + 1, y - 1) + 2.0 * level(x + 1, y) + level(x + 1, y + 1) - level(x - 1, y - 1) - 2.0 * level(x - 1, y) - level(x - 1, y + 1);
        let gy = level(x - 1, y + 1) + 2.0 * level(x, y + 1) + level(x + 1, y + 1) - level(x - 1, y - 1) - 2.0 * level(x, y - 1) - level(x + 1, y - 1);
        let ink = ((gx * gx + gy * gy).sqrt() * gain / MAX_LEVEL).clamp(0.0, 1.0);
        let pixel = job.source.get(x as u32, y as u32);
        let paper = mix(WHITE, pixel, coloring);
        [clamp_channel(f32::from(paper[0]) * (1.0 - ink)), clamp_channel(f32::from(paper[1]) * (1.0 - ink)), clamp_channel(f32::from(paper[2]) * (1.0 - ink)), pixel[3]]
    })
}

fn oil_painting(job: &Job) -> Option<Raster> {
    let radius = job.params.int("brush_size");
    let buckets = job.params.int("coarseness") as usize;
    let bucket_of = |x: i32, y: i32| {
        let pixel = texel(job.source, x, y, Edge::Clamp).unwrap_or_default();
        (usize::from(intensity(pixel)) * (buckets - 1) / (BUCKETS - 1), pixel)
    };
    render_rows(job.area.width() as u32, job.area.height() as u32, job.cancel, |row, out| {
        let y = job.area.y0 + row as i32;
        let mut counts = [0u16; BUCKETS];
        let mut sums = [[0u32; 3]; BUCKETS];
        for (column, target) in out.chunks_exact_mut(4).enumerate() {
            let x = job.area.x0 + column as i32;
            let window = || (-radius..=radius).flat_map(move |dy| (-radius..=radius).map(move |dx| (x + dx, y + dy)));
            let mut best = 0usize;
            for (wx, wy) in window() {
                let (bucket, pixel) = bucket_of(wx, wy);
                counts[bucket] += 1;
                for c in 0..3 {
                    sums[bucket][c] += u32::from(pixel[c]);
                }
                if counts[bucket] > counts[best] {
                    best = bucket;
                }
            }
            let count = u32::from(counts[best]);
            let alpha = job.source.get(x as u32, y as u32)[3];
            target.copy_from_slice(&[(sums[best][0] / count) as u8, (sums[best][1] / count) as u8, (sums[best][2] / count) as u8, alpha]);
            for (wx, wy) in window() {
                let (bucket, _) = bucket_of(wx, wy);
                counts[bucket] = 0;
                sums[bucket] = [0; 3];
            }
        }
    })
}

fn pencil_sketch(job: &Job) -> Option<Raster> {
    let sigma = job.params.int("tip_size") as f32;
    let shift = job.params.int("range") as f32 * LEVELS_PER_PERCENT;
    let (region, inner) = super::util::read_region(job.source, job.area, (sigma * 3.0).ceil() as i32);
    let inverted = map_area(region, job.cancel, |x, y| {
        let level = 255 - intensity(job.source.get(x as u32, y as u32));
        [level, level, level, 255]
    })?;
    let blurred = gaussian_blur(Plane::from_raster(&inverted, inverted.bounds(), 1.0), sigma, job.cancel)?.to_raster(inner, 1.0);
    map_area(job.area, job.cancel, |x, y| {
        let source = job.source.get(x as u32, y as u32);
        let gray = f32::from(intensity(source));
        let blur = f32::from(blurred.get((x - job.area.x0) as u32, (y - job.area.y0) as u32)[0]);
        let dodged = (gray * MAX_LEVEL / (MAX_LEVEL - blur + 1.0)).min(MAX_LEVEL);
        let level = clamp_channel(dodged + shift);
        [level, level, level, source[3]]
    })
}
