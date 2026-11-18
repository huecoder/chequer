use crate::effects::params::{choice, flag, int, seed};
use crate::effects::util::{hash, hash_unit, par_rows};
use crate::effects::{Category, EffectDef, Job, ids};
use crate::raster::{BYTES_PER_PIXEL, Raster};

const OPERATIONS: &[&str] = &["xor", "rotate_bits", "add", "swap_channels", "crush", "stutter", "reverse"];
const BYTE_OPERATIONS: &[&str] = &["xor", "rotate_bits", "add", "crush"];
const CHANNEL_CHOICES: &[&str] = &["channel_all", "channel_red", "channel_green", "channel_blue"];
const ALL_COLOR_CHANNELS: [usize; 3] = [0, 1, 2];
const MAX_CHUNK_PIXELS: i32 = 65536;
const PERCENT: f32 = 100.0;
const BITS_PER_BYTE: u32 = 8;
const STUTTER_REACH: u32 = 8;
const PERMUTATIONS: [[usize; 3]; 6] = [[0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0], [0, 1, 2]];
const AFFECT_SALT: i32 = 1;
const VALUE_SALT: i32 = 2;
const PERMUTATION_SALT: i32 = 3;
const STUTTER_SALT: i32 = 4;

pub const DEF: EffectDef = EffectDef {
    id: ids::DATABEND,
    category: Category::Glitch,
    params: &[
        choice("operation", OPERATIONS, 0),
        int("chunk", 1, MAX_CHUNK_PIXELS, 256),
        int("amount", 0, 100, 30),
        int("value", 0, 255, 85).only_if(&[("operation", BYTE_OPERATIONS)]),
        flag("random_value", true).only_if(&[("operation", BYTE_OPERATIONS)]),
        choice("channels", CHANNEL_CHOICES, 0).only_if(&[("operation", BYTE_OPERATIONS)]),
        seed("seed"),
    ],
    render,
};

fn affected_channels(choice: usize) -> &'static [usize] {
    static SINGLE: [[usize; 1]; 3] = [[0], [1], [2]];
    match choice {
        0 => &ALL_COLOR_CHANNELS,
        other => &SINGLE[other - 1],
    }
}

fn map_bytes(piece: &mut [u8], channels: &[usize], f: impl Fn(u8) -> u8) {
    for pixel in piece.chunks_exact_mut(BYTES_PER_PIXEL) {
        for &channel in channels {
            pixel[channel] = f(pixel[channel]);
        }
    }
}

fn swap_channels(piece: &mut [u8], permutation: [usize; 3]) {
    for pixel in piece.chunks_exact_mut(BYTES_PER_PIXEL) {
        let original = [pixel[0], pixel[1], pixel[2]];
        for (slot, source) in pixel.iter_mut().zip(permutation) {
            *slot = original[source];
        }
    }
}

fn reverse_pixels(piece: &mut [u8]) {
    let pixels = piece.len() / BYTES_PER_PIXEL;
    for front in 0..pixels / 2 {
        let back = pixels - 1 - front;
        for byte in 0..BYTES_PER_PIXEL {
            piece.swap(front * BYTES_PER_PIXEL + byte, back * BYTES_PER_PIXEL + byte);
        }
    }
}

fn render(job: &Job) -> Option<Raster> {
    let region = job.source.crop(job.area);
    let original = region.as_bytes();
    let params = job.params;
    let (operation, seed, amount, fixed_value) = (params.choice_key("operation"), params.seed("seed"), params.int("amount") as f32, params.int("value") as u8);
    let random_value = params.bool("random_value");
    let channels = affected_channels(params.choice("channels"));
    let chunk_bytes = params.int("chunk") as usize * BYTES_PER_PIXEL;
    let mut data = original.to_vec();
    let finished = par_rows(&mut data, chunk_bytes, job.cancel, |index, piece| {
        let chunk = index as i32;
        if hash_unit(seed, chunk, AFFECT_SALT) * PERCENT >= amount {
            return;
        }
        let value = if random_value { hash(seed, chunk, VALUE_SALT) as u8 } else { fixed_value };
        match operation {
            "xor" => map_bytes(piece, channels, |byte| byte ^ value),
            "rotate_bits" => map_bytes(piece, channels, |byte| byte.rotate_left(u32::from(value) % (BITS_PER_BYTE - 1) + 1)),
            "add" => map_bytes(piece, channels, |byte| byte.wrapping_add(value)),
            "crush" => {
                let dropped = u32::from(value) % (BITS_PER_BYTE - 1) + 1;
                map_bytes(piece, channels, |byte| byte >> dropped << dropped);
            }
            "swap_channels" => swap_channels(piece, PERMUTATIONS[hash(seed, chunk, PERMUTATION_SALT) as usize % PERMUTATIONS.len()]),
            "stutter" => {
                let back = (hash(seed, chunk, STUTTER_SALT) % STUTTER_REACH) as usize + 1;
                if let Some(earlier) = index.checked_sub(back) {
                    piece.copy_from_slice(&original[earlier * chunk_bytes..earlier * chunk_bytes + piece.len()]);
                }
            }
            "reverse" => reverse_pixels(piece),
            other => panic!("неизвестная операция датабендинга «{other}»"),
        }
    });
    if !finished {
        return None;
    }
    Raster::from_rgba(region.width(), region.height(), data).ok()
}
