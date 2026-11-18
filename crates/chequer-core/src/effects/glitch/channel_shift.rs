use super::{EDGES, merge_channels, signed_unit};
use crate::effects::params::{choice, int, seed};
use crate::effects::util::{map_area, texel};
use crate::effects::{Category, EffectDef, Job, ids};
use crate::raster::Raster;

const MAX_OFFSET: i32 = 500;
const MAX_JITTER: i32 = 200;
const MAX_JITTER_ROWS: i32 = 200;
const CHANNEL_OFFSETS: [(&str, &str); 3] = [("red_x", "red_y"), ("green_x", "green_y"), ("blue_x", "blue_y")];

pub const DEF: EffectDef = EffectDef {
    id: ids::CHANNEL_SHIFT,
    category: Category::Glitch,
    params: &[
        int("red_x", -MAX_OFFSET, MAX_OFFSET, 8),
        int("red_y", -MAX_OFFSET, MAX_OFFSET, 0),
        int("green_x", -MAX_OFFSET, MAX_OFFSET, 0),
        int("green_y", -MAX_OFFSET, MAX_OFFSET, 0),
        int("blue_x", -MAX_OFFSET, MAX_OFFSET, -8),
        int("blue_y", -MAX_OFFSET, MAX_OFFSET, 0),
        int("row_jitter", 0, MAX_JITTER, 0),
        int("row_jitter_size", 1, MAX_JITTER_ROWS, 8),
        choice("edge", EDGES, 0),
        seed("seed"),
    ],
    render,
};

fn render(job: &Job) -> Option<Raster> {
    let offsets = CHANNEL_OFFSETS.map(|(x, y)| (job.params.int(x), job.params.int(y)));
    let jitter = job.params.int("row_jitter") as f32;
    let band_rows = job.params.int("row_jitter_size");
    let seed = job.params.seed("seed");
    let edge = job.params.edge("edge");
    map_area(job.area, job.cancel, |x, y| {
        let band = y.div_euclid(band_rows);
        let sample = |channel: usize| {
            let (dx, dy) = offsets[channel];
            let wobble = (signed_unit(seed, band, channel as i32) * jitter).round() as i32;
            texel(job.source, x - dx - wobble, y - dy, edge).unwrap_or_default()
        };
        merge_channels(sample(0), sample(1), sample(2))
    })
}
