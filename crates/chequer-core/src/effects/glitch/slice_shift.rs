use super::{EDGES, merge_channels};
use crate::effects::params::{choice, int, seed};
use crate::effects::util::{Rng, map_area, texel};
use crate::effects::{Category, EffectDef, Job, ids};
use crate::raster::Raster;

const DIRECTIONS: &[&str] = &["horizontal", "vertical"];
const VERTICAL: usize = 1;
const MAX_SLICE_SIZE: i32 = 2000;
const MAX_SHIFT: i32 = 2000;
const MAX_SPLIT: i32 = 200;
const PERCENT: f32 = 100.0;

pub const DEF: EffectDef = EffectDef {
    id: ids::SLICE_SHIFT,
    category: Category::Glitch,
    params: &[
        choice("direction", DIRECTIONS, 0),
        int("min_size", 1, MAX_SLICE_SIZE, 4),
        int("max_size", 1, MAX_SLICE_SIZE, 64),
        int("probability", 0, 100, 40),
        int("max_shift", 0, MAX_SHIFT, 80),
        int("color_split", 0, MAX_SPLIT, 4),
        choice("edge", EDGES, 1),
        seed("seed"),
    ],
    render,
};

#[derive(Clone, Copy, Default)]
struct Slice {
    shift: i32,
    split: i32,
}

struct SliceSettings {
    min_size: i32,
    max_size: i32,
    probability: f32,
    max_shift: f32,
    color_split: f32,
}

fn slice_table(extent: usize, settings: &SliceSettings, seed: u32) -> Vec<Slice> {
    let mut rng = Rng::new(seed);
    let mut table = Vec::with_capacity(extent);
    while table.len() < extent {
        let spread = (settings.max_size - settings.min_size).max(0) + 1;
        let size = settings.min_size + (rng.next_unit() * spread as f32) as i32;
        let moved = rng.next_unit() * PERCENT < settings.probability;
        let slice = if moved {
            Slice {
                shift: ((rng.next_unit() * 2.0 - 1.0) * settings.max_shift).round() as i32,
                split: (rng.next_unit() * settings.color_split).round() as i32,
            }
        } else {
            Slice::default()
        };
        table.extend(std::iter::repeat_n(slice, size as usize));
    }
    table.truncate(extent);
    table
}

fn render(job: &Job) -> Option<Raster> {
    let params = job.params;
    let vertical = params.choice("direction") == VERTICAL;
    let extent = if vertical { job.source.width() } else { job.source.height() } as usize;
    let settings = SliceSettings {
        min_size: params.int("min_size"),
        max_size: params.int("max_size"),
        probability: params.int("probability") as f32,
        max_shift: params.int("max_shift") as f32,
        color_split: params.int("color_split") as f32,
    };
    let table = slice_table(extent, &settings, params.seed("seed"));
    let edge = params.edge("edge");
    map_area(job.area, job.cancel, |x, y| {
        let (along, across) = if vertical { (y, x) } else { (x, y) };
        let slice = table[across as usize];
        let sample = |split: i32| {
            let position = along - slice.shift + split;
            let (sx, sy) = if vertical { (across, position) } else { (position, across) };
            texel(job.source, sx, sy, edge).unwrap_or_default()
        };
        merge_channels(sample(slice.split), sample(0), sample(-slice.split))
    })
}
