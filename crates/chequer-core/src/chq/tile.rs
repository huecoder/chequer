use std::collections::HashMap;
use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use xxhash_rust::xxh3::Xxh3;

use super::ChqError;
use super::bytes::Reader;

pub const BYTES_PER_PIXEL: usize = 4;
pub const MAX_TILE_SIDE: u32 = 256;
pub const ENTRY_SIZE: usize = 44;
const RGB_BYTES: usize = 3;
const INDEXED_MAX_COLORS: usize = 256;
const OPAQUE: u8 = 255;
const GREEN: usize = 1;
const RED: usize = 0;
const BLUE: usize = 2;
const FLAG_SUBTRACT_GREEN: u8 = 1;
const ZSTD_LEVEL: i32 = 6;
const DEFLATE_LEVEL: u32 = 6;
const PALETTE_SIZE_BYTES: usize = 1;

macro_rules! byte_enum {
    ($name:ident, $what:literal, { $($variant:ident = $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum $name {
            $($variant = $value),+
        }

        impl TryFrom<u8> for $name {
            type Error = ChqError;

            fn try_from(byte: u8) -> Result<Self, ChqError> {
                match byte {
                    $($value => Ok(Self::$variant),)+
                    _ => Err(ChqError::Invalid($what)),
                }
            }
        }
    };
}

byte_enum!(TileKind, "неизвестный вид тайла", { Solid = 1, Indexed = 2, Rgb = 3, Rgba = 4 });
byte_enum!(Codec, "неизвестный кодек", { Raw = 0, Lz4 = 1, Deflate = 2, Zstd = 3 });
byte_enum!(Predictor, "неизвестный предиктор", { None = 0, Sub = 1, Up = 2, Avg = 3, Paeth = 4 });

const ALL_PREDICTORS: [Predictor; 5] = [Predictor::None, Predictor::Sub, Predictor::Up, Predictor::Avg, Predictor::Paeth];
const FAST_PREDICTORS: [Predictor; 1] = [Predictor::Sub];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteMode {
    Fast,
    Small,
}

impl WriteMode {
    fn codec(self) -> Codec {
        match self {
            Self::Fast => Codec::Lz4,
            Self::Small => Codec::Zstd,
        }
    }

    fn predictors(self) -> &'static [Predictor] {
        match self {
            Self::Fast => &FAST_PREDICTORS,
            Self::Small => &ALL_PREDICTORS,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TileEntry {
    pub kind: TileKind,
    pub codec: Codec,
    pub predictor: Predictor,
    pub flags: u8,
    pub width: u16,
    pub height: u16,
    pub offset: u64,
    pub length: u32,
    pub crc: u32,
    pub color: [u8; 4],
    pub hash: u128,
}

impl TileEntry {
    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&[self.kind as u8, self.codec as u8, self.predictor as u8, self.flags]);
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&self.offset.to_le_bytes());
        out.extend_from_slice(&self.length.to_le_bytes());
        out.extend_from_slice(&self.crc.to_le_bytes());
        out.extend_from_slice(&self.color);
        out.extend_from_slice(&self.hash.to_le_bytes());
    }

    pub fn read(reader: &mut Reader) -> Result<Self, ChqError> {
        let entry = Self {
            kind: reader.u8()?.try_into()?,
            codec: reader.u8()?.try_into()?,
            predictor: reader.u8()?.try_into()?,
            flags: reader.u8()?,
            width: reader.u16()?,
            height: reader.u16()?,
            offset: reader.u64()?,
            length: reader.u32()?,
            crc: reader.u32()?,
            color: reader.take(BYTES_PER_PIXEL)?.try_into().map_err(|_| ChqError::Invalid("цвет тайла"))?,
            hash: reader.u128()?,
        };
        let side_ok = |side: u16| (1..=MAX_TILE_SIDE).contains(&u32::from(side));
        if !side_ok(entry.width) || !side_ok(entry.height) {
            return Err(ChqError::Invalid("размер тайла вне допустимого"));
        }
        Ok(entry)
    }

    fn pixel_count(&self) -> usize {
        usize::from(self.width) * usize::from(self.height)
    }
}

pub struct TilePixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub hash: u128,
}

impl TilePixels {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        let mut hasher = Xxh3::new();
        hasher.update(&width.to_le_bytes());
        hasher.update(&height.to_le_bytes());
        hasher.update(&rgba);
        Self { width, height, rgba, hash: hasher.digest128() }
    }

    pub fn is_transparent(&self) -> bool {
        self.rgba.iter().all(|&byte| byte == 0)
    }

    fn entry(&self, kind: TileKind, color: [u8; 4]) -> TileEntry {
        TileEntry {
            kind,
            codec: Codec::Raw,
            predictor: Predictor::None,
            flags: 0,
            width: self.width as u16,
            height: self.height as u16,
            offset: 0,
            length: 0,
            crc: 0,
            color,
            hash: self.hash,
        }
    }
}

pub struct EncodedTile {
    pub entry: TileEntry,
    pub blob: Vec<u8>,
}

struct Palette {
    colors: Vec<[u8; 4]>,
    indices: Vec<u8>,
}

enum Shape {
    Solid([u8; 4]),
    Indexed(Palette),
    Rgb,
    Rgba,
}

fn pixels(rgba: &[u8]) -> impl Iterator<Item = [u8; 4]> + '_ {
    rgba.chunks_exact(BYTES_PER_PIXEL).map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
}

fn palette_of(rgba: &[u8]) -> Option<Palette> {
    let mut lookup: HashMap<[u8; 4], u8> = HashMap::new();
    let mut colors = Vec::new();
    let mut indices = Vec::with_capacity(rgba.len() / BYTES_PER_PIXEL);
    for color in pixels(rgba) {
        let index = match lookup.get(&color) {
            Some(&index) => index,
            None => {
                if colors.len() == INDEXED_MAX_COLORS {
                    return None;
                }
                let index = colors.len() as u8;
                colors.push(color);
                lookup.insert(color, index);
                index
            }
        };
        indices.push(index);
    }
    Some(Palette { colors, indices })
}

fn classify(rgba: &[u8]) -> Shape {
    let first = pixels(rgba).next().unwrap_or_default();
    if pixels(rgba).all(|color| color == first) {
        return Shape::Solid(first);
    }
    if let Some(palette) = palette_of(rgba) {
        return Shape::Indexed(palette);
    }
    if pixels(rgba).all(|color| color[BYTES_PER_PIXEL - 1] == OPAQUE) { Shape::Rgb } else { Shape::Rgba }
}

fn subtract_green(body: &mut [u8], bytes_per_pixel: usize) {
    for pixel in body.chunks_exact_mut(bytes_per_pixel) {
        pixel[RED] = pixel[RED].wrapping_sub(pixel[GREEN]);
        pixel[BLUE] = pixel[BLUE].wrapping_sub(pixel[GREEN]);
    }
}

fn add_green(body: &mut [u8], bytes_per_pixel: usize) {
    for pixel in body.chunks_exact_mut(bytes_per_pixel) {
        pixel[RED] = pixel[RED].wrapping_add(pixel[GREEN]);
        pixel[BLUE] = pixel[BLUE].wrapping_add(pixel[GREEN]);
    }
}

fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let estimate = i16::from(left) + i16::from(up) - i16::from(up_left);
    let distance = |neighbour: u8| (estimate - i16::from(neighbour)).abs();
    let (to_left, to_up, to_up_left) = (distance(left), distance(up), distance(up_left));
    if to_left <= to_up && to_left <= to_up_left {
        left
    } else if to_up <= to_up_left {
        up
    } else {
        up_left
    }
}

fn predict(predictor: Predictor, left: u8, up: u8, up_left: u8) -> u8 {
    match predictor {
        Predictor::None => 0,
        Predictor::Sub => left,
        Predictor::Up => up,
        Predictor::Avg => ((u16::from(left) + u16::from(up)) / 2) as u8,
        Predictor::Paeth => paeth(left, up, up_left),
    }
}

fn neighbours(body: &[u8], row_bytes: usize, bytes_per_pixel: usize, position: usize) -> (u8, u8, u8) {
    let has_left = position % row_bytes >= bytes_per_pixel;
    let has_up = position >= row_bytes;
    let left = if has_left { body[position - bytes_per_pixel] } else { 0 };
    let up = if has_up { body[position - row_bytes] } else { 0 };
    let up_left = if has_left && has_up { body[position - row_bytes - bytes_per_pixel] } else { 0 };
    (left, up, up_left)
}

fn filter(body: &[u8], row_bytes: usize, bytes_per_pixel: usize, predictor: Predictor) -> Vec<u8> {
    (0..body.len())
        .map(|position| {
            let (left, up, up_left) = neighbours(body, row_bytes, bytes_per_pixel, position);
            body[position].wrapping_sub(predict(predictor, left, up, up_left))
        })
        .collect()
}

fn unfilter(body: &mut [u8], row_bytes: usize, bytes_per_pixel: usize, predictor: Predictor) {
    for position in 0..body.len() {
        let (left, up, up_left) = neighbours(body, row_bytes, bytes_per_pixel, position);
        body[position] = body[position].wrapping_add(predict(predictor, left, up, up_left));
    }
}

fn residual_cost(filtered: &[u8]) -> u64 {
    filtered.iter().map(|&byte| u64::from((byte as i8).unsigned_abs())).sum()
}

fn best_filter(body: &[u8], row_bytes: usize, bytes_per_pixel: usize, mode: WriteMode) -> (Predictor, Vec<u8>) {
    mode.predictors()
        .iter()
        .map(|&predictor| (predictor, filter(body, row_bytes, bytes_per_pixel, predictor)))
        .min_by_key(|(_, filtered)| residual_cost(filtered))
        .unwrap_or((Predictor::None, body.to_vec()))
}

pub fn compress(codec: Codec, bytes: &[u8]) -> Result<Vec<u8>, ChqError> {
    match codec {
        Codec::Raw => Ok(bytes.to_vec()),
        Codec::Lz4 => Ok(lz4_flex::block::compress(bytes)),
        Codec::Deflate => {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(DEFLATE_LEVEL));
            encoder.write_all(bytes)?;
            Ok(encoder.finish()?)
        }
        Codec::Zstd => Ok(zstd::bulk::compress(bytes, ZSTD_LEVEL)?),
    }
}

pub fn decompress(codec: Codec, data: &[u8], size_limit: usize) -> Result<Vec<u8>, ChqError> {
    let bytes = match codec {
        Codec::Raw => data.to_vec(),
        Codec::Lz4 => lz4_flex::block::decompress(data, size_limit).map_err(|_| ChqError::Invalid("ошибка распаковки lz4"))?,
        Codec::Deflate => {
            let mut bytes = Vec::new();
            DeflateDecoder::new(data).take(size_limit as u64 + 1).read_to_end(&mut bytes)?;
            bytes
        }
        Codec::Zstd => zstd::bulk::decompress(data, size_limit)?,
    };
    if bytes.len() > size_limit {
        return Err(ChqError::Invalid("распакованные данные больше заявленного"));
    }
    Ok(bytes)
}

fn seal(mut entry: TileEntry, body: Vec<u8>, mode: WriteMode) -> Result<EncodedTile, ChqError> {
    let packed = compress(mode.codec(), &body)?;
    let (codec, blob) = if packed.len() < body.len() { (mode.codec(), packed) } else { (Codec::Raw, body) };
    entry.codec = codec;
    entry.length = blob.len() as u32;
    entry.crc = crc32fast::hash(&blob);
    Ok(EncodedTile { entry, blob })
}

fn pixel_body(rgba: &[u8], bytes_per_pixel: usize) -> Vec<u8> {
    let mut body: Vec<u8> = if bytes_per_pixel == RGB_BYTES {
        rgba.chunks_exact(BYTES_PER_PIXEL).flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]).collect()
    } else {
        rgba.to_vec()
    };
    subtract_green(&mut body, bytes_per_pixel);
    body
}

fn encode_pixels(tile: &TilePixels, kind: TileKind, bytes_per_pixel: usize, mode: WriteMode) -> Result<EncodedTile, ChqError> {
    let body = pixel_body(&tile.rgba, bytes_per_pixel);
    let (predictor, filtered) = best_filter(&body, tile.width as usize * bytes_per_pixel, bytes_per_pixel, mode);
    let mut entry = tile.entry(kind, [0; 4]);
    entry.predictor = predictor;
    entry.flags = FLAG_SUBTRACT_GREEN;
    seal(entry, filtered, mode)
}

pub fn encode_tile(tile: &TilePixels, mode: WriteMode) -> Result<EncodedTile, ChqError> {
    match classify(&tile.rgba) {
        Shape::Solid(color) => Ok(EncodedTile { entry: tile.entry(TileKind::Solid, color), blob: Vec::new() }),
        Shape::Indexed(palette) => {
            let mut body = vec![(palette.colors.len() - 1) as u8];
            body.extend(palette.colors.iter().flatten());
            body.extend(&palette.indices);
            seal(tile.entry(TileKind::Indexed, [0; 4]), body, mode)
        }
        Shape::Rgb => encode_pixels(tile, TileKind::Rgb, RGB_BYTES, mode),
        Shape::Rgba => encode_pixels(tile, TileKind::Rgba, BYTES_PER_PIXEL, mode),
    }
}

fn decode_indexed(entry: &TileEntry, blob: &[u8]) -> Result<Vec<u8>, ChqError> {
    let pixel_count = entry.pixel_count();
    let limit = PALETTE_SIZE_BYTES + INDEXED_MAX_COLORS * BYTES_PER_PIXEL + pixel_count;
    let body = decompress(entry.codec, blob, limit)?;
    let (&last_index, rest) = body.split_first().ok_or(ChqError::Invalid("пустой тайл"))?;
    let palette_bytes = (usize::from(last_index) + 1) * BYTES_PER_PIXEL;
    if rest.len() != palette_bytes + pixel_count {
        return Err(ChqError::Invalid("размер индексного тайла не совпадает"));
    }
    let (palette, indices) = rest.split_at(palette_bytes);
    let mut rgba = Vec::with_capacity(pixel_count * BYTES_PER_PIXEL);
    for &index in indices {
        let start = usize::from(index) * BYTES_PER_PIXEL;
        rgba.extend_from_slice(palette.get(start..start + BYTES_PER_PIXEL).ok_or(ChqError::Invalid("индекс вне палитры"))?);
    }
    Ok(rgba)
}

fn decode_pixels(entry: &TileEntry, blob: &[u8], bytes_per_pixel: usize) -> Result<Vec<u8>, ChqError> {
    let mut body = decompress(entry.codec, blob, entry.pixel_count() * bytes_per_pixel)?;
    if body.len() != entry.pixel_count() * bytes_per_pixel {
        return Err(ChqError::Invalid("размер тайла не совпадает"));
    }
    unfilter(&mut body, usize::from(entry.width) * bytes_per_pixel, bytes_per_pixel, entry.predictor);
    if entry.flags & FLAG_SUBTRACT_GREEN != 0 {
        add_green(&mut body, bytes_per_pixel);
    }
    if bytes_per_pixel == BYTES_PER_PIXEL {
        return Ok(body);
    }
    Ok(body.chunks_exact(RGB_BYTES).flat_map(|pixel| [pixel[0], pixel[1], pixel[2], OPAQUE]).collect())
}

pub fn decode_tile(entry: &TileEntry, blob: &[u8]) -> Result<Vec<u8>, ChqError> {
    if crc32fast::hash(blob) != entry.crc {
        return Err(ChqError::Invalid("контрольная сумма тайла не совпала"));
    }
    match entry.kind {
        TileKind::Solid => Ok(entry.color.iter().copied().cycle().take(entry.pixel_count() * BYTES_PER_PIXEL).collect()),
        TileKind::Indexed => decode_indexed(entry, blob),
        TileKind::Rgb => decode_pixels(entry, blob, RGB_BYTES),
        TileKind::Rgba => decode_pixels(entry, blob, BYTES_PER_PIXEL),
    }
}
