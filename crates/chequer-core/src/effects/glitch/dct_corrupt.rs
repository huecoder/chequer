use crate::effects::params::{choice, int, seed};
use crate::effects::util::{Cancel, clamp_channel, hash, hash_unit, par_rows, render_rows};
use crate::effects::{Category, EffectDef, Job, ids};
use crate::raster::{BYTES_PER_PIXEL, Raster};

use super::signed_unit;

const BLOCK: usize = 8;
const BLOCK_AREA: usize = BLOCK * BLOCK;
const SUBSAMPLINGS: &[&str] = &["chroma_full", "chroma_half", "chroma_quarter"];
const SUBSAMPLE_FACTORS: [(usize, usize); 3] = [(1, 1), (2, 2), (4, 1)];
const PERCENT: f32 = 100.0;
const SAMPLE_CENTER: f32 = 128.0;
const DC_SPAN: f32 = 64.0;
const RING_SPAN: f32 = 4.0;
const SWAP_SALT: u32 = 11;
const ZERO_SALT: u32 = 12;
const SCRAMBLE_SALT: u32 = 13;
const RING_SALT: u32 = 14;
const DRIFT_SALT: u32 = 15;
const DRIFT_SIZE_SALT: u32 = 16;
const SWAP_X_SALT: u32 = 17;
const SWAP_Y_SALT: u32 = 18;
const PAIR_SALT: u32 = 100;
const RING_GAIN_SALT: u32 = 19;

const LUMA_TABLE: [u8; BLOCK_AREA] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92, 49, 64, 78, 87,
    103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];
const CHROMA_TABLE: [u8; BLOCK_AREA] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99, 47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];
const ZIGZAG: [usize; BLOCK_AREA] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39,
    46, 53, 60, 61, 54, 47, 55, 62, 63,
];

pub const DEF: EffectDef = EffectDef {
    id: ids::DCT_CORRUPT,
    category: Category::Glitch,
    params: &[
        int("quality", 1, 100, 40),
        int("quality_mismatch", -50, 50, 0),
        choice("subsampling", SUBSAMPLINGS, 1),
        int("dc_drift", 0, 100, 25),
        int("color_drift", 0, 100, 25),
        int("drift_size", 1, 100, 40),
        int("zero_blocks", 0, 100, 10),
        int("zero_from", 1, 63, 6),
        int("scramble", 0, 100, 10),
        int("scramble_swaps", 1, 32, 6),
        int("ringing", 0, 100, 10),
        int("block_swap", 0, 100, 8),
        int("swap_radius", 1, 64, 6),
        seed("seed"),
    ],
    render,
};

type Block = [f32; BLOCK_AREA];

#[derive(Clone, Copy)]
enum PlaneKind {
    Luma,
    BlueDifference,
    RedDifference,
}

impl PlaneKind {
    fn table_index(self) -> usize {
        usize::from(!matches!(self, PlaneKind::Luma))
    }

    fn salt(self) -> u32 {
        match self {
            PlaneKind::Luma => 0,
            PlaneKind::BlueDifference => 0x9e37,
            PlaneKind::RedDifference => 0x79b9,
        }
    }
}

struct Corruption {
    seed: u32,
    dc_drift: f32,
    color_drift: f32,
    drift_size: f32,
    zero_blocks: f32,
    zero_from: usize,
    scramble: f32,
    scramble_swaps: u32,
    ringing: f32,
    block_swap: f32,
    swap_radius: i32,
}

struct Basis([[f32; BLOCK]; BLOCK]);

impl Basis {
    fn new() -> Basis {
        let mut rows = [[0.0; BLOCK]; BLOCK];
        for (frequency, row) in rows.iter_mut().enumerate() {
            let scale = if frequency == 0 { (1.0 / BLOCK as f32).sqrt() } else { (2.0 / BLOCK as f32).sqrt() };
            for (position, value) in row.iter_mut().enumerate() {
                *value = scale * ((2 * position + 1) as f32 * frequency as f32 * std::f32::consts::PI / (2 * BLOCK) as f32).cos();
            }
        }
        Basis(rows)
    }

    fn forward(&self, samples: &Block) -> Block {
        let mut across = [0.0; BLOCK_AREA];
        let mut result = [0.0; BLOCK_AREA];
        for y in 0..BLOCK {
            for u in 0..BLOCK {
                across[y * BLOCK + u] = (0..BLOCK).map(|x| self.0[u][x] * samples[y * BLOCK + x]).sum();
            }
        }
        for v in 0..BLOCK {
            for u in 0..BLOCK {
                result[v * BLOCK + u] = (0..BLOCK).map(|y| self.0[v][y] * across[y * BLOCK + u]).sum();
            }
        }
        result
    }

    fn inverse(&self, coefficients: &Block) -> Block {
        let mut across = [0.0; BLOCK_AREA];
        let mut result = [0.0; BLOCK_AREA];
        for v in 0..BLOCK {
            for x in 0..BLOCK {
                across[v * BLOCK + x] = (0..BLOCK).map(|u| self.0[u][x] * coefficients[v * BLOCK + u]).sum();
            }
        }
        for y in 0..BLOCK {
            for x in 0..BLOCK {
                result[y * BLOCK + x] = (0..BLOCK).map(|v| self.0[v][y] * across[v * BLOCK + x]).sum();
            }
        }
        result
    }
}

fn scaled_table(base: &[u8; BLOCK_AREA], quality: i32) -> Block {
    let quality = quality.clamp(1, 100) as f32;
    let scale = if quality < 50.0 { 5000.0 / quality } else { 200.0 - 2.0 * quality };
    base.map(|step| ((f32::from(step) * scale + 50.0) / PERCENT).floor().clamp(1.0, 255.0))
}

struct Plane {
    stride: usize,
    rows: usize,
    data: Vec<f32>,
}

impl Plane {
    fn block(&self, bx: usize, by: usize) -> Block {
        std::array::from_fn(|index| self.data[(by * BLOCK + index / BLOCK) * self.stride + bx * BLOCK + index % BLOCK] - SAMPLE_CENTER)
    }
}

struct Codec {
    basis: Basis,
    encode: [Block; 2],
    decode: [Block; 2],
    corruption: Corruption,
}

impl Codec {
    fn quantized(&self, plane: &Plane, kind: PlaneKind, bx: usize, by: usize) -> Block {
        let table = &self.encode[kind.table_index()];
        let mut coefficients = self.basis.forward(&plane.block(bx, by));
        coefficients.iter_mut().zip(table).for_each(|(value, step)| *value = (*value / step).round());
        coefficients
    }

    fn corrupt(&self, coefficients: &mut Block, plane: &Plane, kind: PlaneKind, bx: usize, by: usize) {
        let settings = &self.corruption;
        let (blocks_x, blocks_y) = (plane.stride / BLOCK, plane.rows / BLOCK);
        let id = (by * blocks_x + bx) as i32;
        let key = settings.seed ^ kind.salt();
        let roll = |salt: u32| hash_unit(key, id, salt as i32) * PERCENT;
        if roll(SWAP_SALT) < settings.block_swap {
            let offset = |salt: u32| (signed_unit(key, id, salt as i32) * settings.swap_radius as f32).round() as i32;
            let neighbor_x = (bx as i32 + offset(SWAP_X_SALT)).clamp(0, blocks_x as i32 - 1) as usize;
            let neighbor_y = (by as i32 + offset(SWAP_Y_SALT)).clamp(0, blocks_y as i32 - 1) as usize;
            *coefficients = self.quantized(plane, kind, neighbor_x, neighbor_y);
        }
        if roll(ZERO_SALT) < settings.zero_blocks {
            ZIGZAG[settings.zero_from..].iter().for_each(|&position| coefficients[position] = 0.0);
        }
        if roll(SCRAMBLE_SALT) < settings.scramble {
            for swap in 0..settings.scramble_swaps {
                let pick = |side: u32| ZIGZAG[hash(key, id, (PAIR_SALT + swap * 2 + side) as i32) as usize % BLOCK_AREA];
                coefficients.swap(pick(0), pick(1));
            }
        }
        if roll(RING_SALT) < settings.ringing {
            let gain = 1.0 + RING_SPAN * hash_unit(key, id, RING_GAIN_SALT as i32);
            coefficients[1..].iter_mut().for_each(|value| *value *= gain);
        }
        let drift = if matches!(kind, PlaneKind::Luma) { settings.dc_drift } else { settings.color_drift };
        if roll(DRIFT_SALT) < drift {
            coefficients[0] += signed_unit(key, id, DRIFT_SIZE_SALT as i32) * settings.drift_size / PERCENT * DC_SPAN;
        }
    }

    fn reconstruct(&self, plane: &Plane, kind: PlaneKind, cancel: &Cancel) -> Option<Plane> {
        let mut data = vec![0.0; plane.data.len()];
        let decode = &self.decode[kind.table_index()];
        let finished = par_rows(&mut data, plane.stride * BLOCK, cancel, |by, rows| {
            for bx in 0..plane.stride / BLOCK {
                let mut coefficients = self.quantized(plane, kind, bx, by);
                self.corrupt(&mut coefficients, plane, kind, bx, by);
                coefficients.iter_mut().zip(decode).for_each(|(value, step)| *value *= step);
                let samples = self.basis.inverse(&coefficients);
                for (index, sample) in samples.iter().enumerate() {
                    rows[(index / BLOCK) * plane.stride + bx * BLOCK + index % BLOCK] = sample + SAMPLE_CENTER;
                }
            }
        });
        finished.then_some(Plane { stride: plane.stride, rows: plane.rows, data })
    }
}

fn to_ycc([red, green, blue]: [f32; 3]) -> [f32; 3] {
    [
        0.299 * red + 0.587 * green + 0.114 * blue,
        SAMPLE_CENTER - 0.168_736 * red - 0.331_264 * green + 0.5 * blue,
        SAMPLE_CENTER + 0.5 * red - 0.418_688 * green - 0.081_312 * blue,
    ]
}

fn split_planes(region: &Raster, padded: (usize, usize), factor: (usize, usize)) -> [Plane; 3] {
    let (width, height) = (region.width() as usize, region.height() as usize);
    let ycc_at = |x: usize, y: usize| {
        let pixel = region.get(x.min(width - 1) as u32, y.min(height - 1) as u32);
        to_ycc([pixel[0], pixel[1], pixel[2]].map(f32::from))
    };
    let luma = Plane { stride: padded.0, rows: padded.1, data: (0..padded.0 * padded.1).map(|index| ycc_at(index % padded.0, index / padded.0)[0]).collect() };
    let chroma = |channel: usize| {
        let (stride, rows) = (padded.0 / factor.0, padded.1 / factor.1);
        let data = (0..stride * rows)
            .map(|index| {
                let (cx, cy) = ((index % stride) * factor.0, (index / stride) * factor.1);
                let total: f32 = (0..factor.1).flat_map(|dy| (0..factor.0).map(move |dx| (dx, dy))).map(|(dx, dy)| ycc_at(cx + dx, cy + dy)[channel]).sum();
                total / (factor.0 * factor.1) as f32
            })
            .collect();
        Plane { stride, rows, data }
    };
    [luma, chroma(1), chroma(2)]
}

fn render(job: &Job) -> Option<Raster> {
    let region = job.source.crop(job.area);
    let params = job.params;
    let factor = SUBSAMPLE_FACTORS[params.choice("subsampling")];
    let round_up = |value: usize, multiple: usize| value.div_ceil(multiple) * multiple;
    let (width, height) = (region.width() as usize, region.height() as usize);
    let padded = (round_up(width, BLOCK * factor.0), round_up(height, BLOCK * factor.1));
    let quality = params.int("quality");
    let decode_quality = quality + params.int("quality_mismatch");
    let percent = |key: &str| params.int(key) as f32;
    let codec = Codec {
        basis: Basis::new(),
        encode: [scaled_table(&LUMA_TABLE, quality), scaled_table(&CHROMA_TABLE, quality)],
        decode: [scaled_table(&LUMA_TABLE, decode_quality), scaled_table(&CHROMA_TABLE, decode_quality)],
        corruption: Corruption {
            seed: params.seed("seed"),
            dc_drift: percent("dc_drift"),
            color_drift: percent("color_drift"),
            drift_size: percent("drift_size"),
            zero_blocks: percent("zero_blocks"),
            zero_from: params.int("zero_from") as usize,
            scramble: percent("scramble"),
            scramble_swaps: params.int("scramble_swaps") as u32,
            ringing: percent("ringing"),
            block_swap: percent("block_swap"),
            swap_radius: params.int("swap_radius"),
        },
    };
    let [luma, blue, red] = split_planes(&region, padded, factor);
    let luma = codec.reconstruct(&luma, PlaneKind::Luma, job.cancel)?;
    let blue = codec.reconstruct(&blue, PlaneKind::BlueDifference, job.cancel)?;
    let red = codec.reconstruct(&red, PlaneKind::RedDifference, job.cancel)?;
    render_rows(width as u32, height as u32, job.cancel, |y, row| {
        let y = y as usize;
        let source = region.row_bytes(y as u32, 0, width as u32);
        for (x, pixel) in row.chunks_exact_mut(BYTES_PER_PIXEL).enumerate() {
            let chroma_at = (y / factor.1) * blue.stride + x / factor.0;
            let (luma_value, cb, cr) = (luma.data[y * luma.stride + x], blue.data[chroma_at] - SAMPLE_CENTER, red.data[chroma_at] - SAMPLE_CENTER);
            pixel[0] = clamp_channel(luma_value + 1.402 * cr);
            pixel[1] = clamp_channel(luma_value - 0.344_136 * cb - 0.714_136 * cr);
            pixel[2] = clamp_channel(luma_value + 1.772 * cb);
            pixel[3] = source[x * BYTES_PER_PIXEL + 3];
        }
    })
}
