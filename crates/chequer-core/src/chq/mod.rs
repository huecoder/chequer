mod bytes;
mod cels;
mod history;
mod layout;
mod legacy;
mod tile;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::blend::BlendMode;
use crate::container::{Chunk, ContainerError, Tag};
use crate::document::{Document, DocumentError, DocumentPalette, Layer, Resample};
use crate::frames::{Frame, MAX_FRAMES};
use crate::raster::{Raster, RasterError};

use bytes::{Reader, unzigzag, write_varint, zigzag};
use tile::{BYTES_PER_PIXEL, EncodedTile, TileEntry, TilePixels, decode_tile, encode_tile};

pub use tile::WriteMode;

pub const FILE_EXTENSION: &str = "chq";

const DOC_TAG: Tag = *b"DOC ";
const LAYERS_TAG: Tag = *b"LAYR";
const CELLS_TAG: Tag = *b"CELS";
const ANIMATED_CELLS_TAG: Tag = *b"ACEL";
const THUMBNAIL_TAG: Tag = *b"THUM";
const META_TAG: Tag = *b"META";
const PALETTE_TAG: Tag = *b"PAL ";
const HISTORY_TAG: Tag = *b"HIST";
const KNOWN_TAGS: [Tag; 8] = [DOC_TAG, LAYERS_TAG, CELLS_TAG, ANIMATED_CELLS_TAG, THUMBNAIL_TAG, META_TAG, PALETTE_TAG, HISTORY_TAG];
const DEFAULT_TILE_LOG2: u8 = 6;
const MAX_LAYERS: usize = 4096;
const EMPTY_CELL: u32 = 0;
const THUMBNAIL_SIDE: u32 = 256;
const MAX_PALETTE_COLORS: usize = 256;
const PALETTE_CHANNELS: usize = 3;
const META_APP_KEY: &str = "app";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const TEMP_SUFFIX: &str = ".tmp";

#[derive(Debug)]
pub enum ChqError {
    Container(ContainerError),
    Json(serde_json::Error),
    Document(DocumentError),
    Raster(RasterError),
    Io(std::io::Error),
    Invalid(&'static str),
    Unsupported(&'static str),
}

impl fmt::Display for ChqError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Container(e) => write!(f, "{e}"),
            Self::Json(e) => write!(f, "{e}"),
            Self::Document(e) => write!(f, "{e}"),
            Self::Raster(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Invalid(what) => write!(f, "файл .chq повреждён: {what}"),
            Self::Unsupported(what) => write!(f, "эта версия Chequer не умеет: {what}"),
        }
    }
}

impl std::error::Error for ChqError {}

impl From<ContainerError> for ChqError {
    fn from(error: ContainerError) -> Self {
        Self::Container(error)
    }
}

impl From<serde_json::Error> for ChqError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<DocumentError> for ChqError {
    fn from(error: DocumentError) -> Self {
        Self::Document(error)
    }
}

impl From<RasterError> for ChqError {
    fn from(error: RasterError) -> Self {
        Self::Raster(error)
    }
}

impl From<std::io::Error> for ChqError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum PixelMode {
    Rgba8,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ColorSpace {
    Srgb,
}

#[derive(Serialize, Deserialize)]
struct FrameInfo {
    duration_ms: u32,
}

#[derive(Serialize, Deserialize)]
struct DocInfo {
    width: u32,
    height: u32,
    active: usize,
    frames: Vec<FrameInfo>,
    mode: PixelMode,
    color_space: ColorSpace,
}

#[derive(Serialize, Deserialize)]
struct LayerEntry {
    name: String,
    visible: bool,
    opacity: u8,
    blend: BlendMode,
    masked: bool,
    collapsed: bool,
}

impl LayerEntry {
    fn of(layer: &Layer) -> Self {
        Self { name: layer.name.clone(), visible: layer.visible, opacity: layer.opacity, blend: layer.blend, masked: layer.masked, collapsed: layer.collapsed }
    }

    fn layer(&self, cels: Vec<Arc<Raster>>) -> Layer {
        let mut layer = Layer::with_cels(self.name.clone(), cels);
        layer.visible = self.visible;
        layer.opacity = self.opacity;
        layer.blend = self.blend;
        layer.masked = self.masked;
        layer.collapsed = self.collapsed;
        layer
    }
}

#[derive(Clone, Copy)]
struct TileGrid {
    side: u32,
    columns: u32,
    rows: u32,
    width: u32,
    height: u32,
}

impl TileGrid {
    fn new(width: u32, height: u32, side: u32) -> Self {
        Self { side, columns: width.div_ceil(side), rows: height.div_ceil(side), width, height }
    }

    fn cells(&self) -> usize {
        self.columns as usize * self.rows as usize
    }

    fn bounds_at(&self, position: usize) -> (u32, u32, u32, u32) {
        let (column, row) = (position as u32 % self.columns, position as u32 / self.columns);
        let (x0, y0) = (column * self.side, row * self.side);
        (x0, y0, (x0 + self.side).min(self.width), (y0 + self.side).min(self.height))
    }
}

#[derive(Default)]
struct TileStore {
    ids: HashMap<u128, u32>,
    tiles: Vec<EncodedTile>,
}

fn read_tile(raster: &Raster, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> TilePixels {
    let mut rgba = Vec::with_capacity((x1 - x0) as usize * (y1 - y0) as usize * BYTES_PER_PIXEL);
    for y in y0..y1 {
        rgba.extend_from_slice(raster.row_bytes(y, x0, x1));
    }
    TilePixels::new(x1 - x0, y1 - y0, rgba)
}

enum Slot {
    Empty,
    Reused(u32),
    Fresh(TilePixels),
}

fn tile_matches(raster: &Raster, previous: &Raster, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> bool {
    (y0..y1).all(|y| raster.row_bytes(y, x0, x1) == previous.row_bytes(y, x0, x1))
}

fn layer_cells(raster: &Raster, previous: Option<(&Raster, &[u32])>, grid: TileGrid, mode: WriteMode, store: &mut TileStore) -> Result<Vec<u32>, ChqError> {
    let slots: Vec<Slot> = (0..grid.cells())
        .into_par_iter()
        .map(|position| {
            let bounds = grid.bounds_at(position);
            if let Some((before, before_cells)) = previous
                && tile_matches(raster, before, bounds)
            {
                return Slot::Reused(before_cells[position]);
            }
            let tile = read_tile(raster, bounds);
            if tile.is_transparent() { Slot::Empty } else { Slot::Fresh(tile) }
        })
        .collect();
    let mut fresh = Vec::new();
    let mut cells = Vec::with_capacity(slots.len());
    for slot in slots {
        let tile = match slot {
            Slot::Empty => {
                cells.push(EMPTY_CELL);
                continue;
            }
            Slot::Reused(id) => {
                cells.push(id);
                continue;
            }
            Slot::Fresh(tile) => tile,
        };
        let next_id = (store.tiles.len() + fresh.len() + 1) as u32;
        let hash = tile.hash;
        cells.push(*store.ids.entry(hash).or_insert_with(|| {
            fresh.push(tile);
            next_id
        }));
    }
    let encoded = fresh.par_iter().map(|tile| encode_tile(tile, mode)).collect::<Result<Vec<_>, _>>()?;
    store.tiles.extend(encoded);
    Ok(cells)
}

fn write_cells(out: &mut Vec<u8>, cells: &[u32]) {
    let mut last_id = 0i64;
    for &cell in cells {
        if cell == EMPTY_CELL {
            write_varint(out, 0);
            continue;
        }
        write_varint(out, 1 + zigzag(i64::from(cell) - last_id));
        last_id = i64::from(cell);
    }
}

fn read_cells(reader: &mut Reader, count: usize) -> Result<Vec<u32>, ChqError> {
    let mut last_id = 0i64;
    let mut cells = Vec::with_capacity(count);
    for _ in 0..count {
        let token = reader.varint()?;
        if token == 0 {
            cells.push(EMPTY_CELL);
            continue;
        }
        last_id = last_id.checked_add(unzigzag(token - 1)).ok_or(ChqError::Invalid("номер тайла вне допустимого"))?;
        cells.push(u32::try_from(last_id).ok().filter(|&id| id != EMPTY_CELL).ok_or(ChqError::Invalid("номер тайла вне допустимого"))?);
    }
    Ok(cells)
}

fn thumbnail_payload(document: &Document, mode: WriteMode) -> Result<Vec<u8>, ChqError> {
    let flat = document.flattened(document.bounds());
    let scale = f64::from(THUMBNAIL_SIDE) / f64::from(flat.width().max(flat.height()));
    let small = if scale < 1.0 {
        let fit = |side: u32| ((f64::from(side) * scale).round() as u32).max(1);
        flat.resized(fit(flat.width()), fit(flat.height()), Resample::Bilinear)
    } else {
        flat
    };
    let encoded = encode_tile(&TilePixels::new(small.width(), small.height(), small.into_bytes()), mode)?;
    let mut payload = Vec::new();
    encoded.entry.write(&mut payload);
    payload.extend_from_slice(&encoded.blob);
    Ok(payload)
}

fn palette_payload(palette: &DocumentPalette) -> Vec<u8> {
    let mut payload = Vec::new();
    write_varint(&mut payload, palette.name.len() as u64);
    payload.extend_from_slice(palette.name.as_bytes());
    let colors = &palette.colors[..palette.colors.len().min(MAX_PALETTE_COLORS)];
    write_varint(&mut payload, colors.len() as u64);
    payload.extend(colors.iter().flatten());
    payload
}

fn read_palette(payload: &[u8]) -> Result<DocumentPalette, ChqError> {
    let mut reader = Reader::new(payload);
    let name_length = usize::try_from(reader.varint()?).map_err(|_| ChqError::Invalid("имя палитры слишком длинное"))?;
    let name = String::from_utf8(reader.take(name_length)?.to_vec()).map_err(|_| ChqError::Invalid("имя палитры не в UTF-8"))?;
    let count = usize::try_from(reader.varint()?).ok().filter(|&count| count <= MAX_PALETTE_COLORS).ok_or(ChqError::Invalid("в палитре слишком много цветов"))?;
    let colors = (0..count).map(|_| reader.take(PALETTE_CHANNELS).map(|rgb| [rgb[0], rgb[1], rgb[2]])).collect::<Result<_, _>>()?;
    Ok(DocumentPalette { name, colors })
}

pub fn encode_document(document: &Document, mode: WriteMode, keep_history: bool) -> Result<Vec<u8>, ChqError> {
    let grid = TileGrid::new(document.width(), document.height(), 1 << DEFAULT_TILE_LOG2);
    let mut store = TileStore::default();
    let cells_chunk = if document.frame_count() > 1 {
        Chunk::critical(ANIMATED_CELLS_TAG, cels::write(document.layers(), grid, mode, &mut store)?)
    } else {
        let mut cells = Vec::new();
        for layer in document.layers() {
            write_cells(&mut cells, &layer_cells(layer.raster(), None, grid, mode, &mut store)?);
        }
        Chunk::critical(CELLS_TAG, cells)
    };
    let layers: Vec<LayerEntry> = document.layers().iter().map(LayerEntry::of).collect();
    let frames = document.frames().iter().map(|frame| FrameInfo { duration_ms: frame.duration_ms }).collect();
    let info = DocInfo { width: grid.width, height: grid.height, active: document.active_index(), frames, mode: PixelMode::Rgba8, color_space: ColorSpace::Srgb };
    let meta = BTreeMap::from([(META_APP_KEY, APP_VERSION)]);
    let mut chunks = vec![
        Chunk::critical(DOC_TAG, serde_json::to_vec(&info)?),
        Chunk::critical(LAYERS_TAG, serde_json::to_vec(&layers)?),
        cells_chunk,
        Chunk::new(THUMBNAIL_TAG, thumbnail_payload(document, mode)?),
        Chunk::new(META_TAG, serde_json::to_vec(&meta)?),
    ];
    if let Some(palette) = &document.palette {
        chunks.push(Chunk::new(PALETTE_TAG, palette_payload(palette)));
    }
    if keep_history && !document.history().is_empty() {
        chunks.push(Chunk::new(HISTORY_TAG, history::write(document.history(), mode, &mut store)?));
    }
    layout::assemble(DEFAULT_TILE_LOG2, &mut store.tiles, &layout::write_chunks(&chunks))
}

fn decode_cell(file: &layout::Parsed, grid: TileGrid, position: usize, cell: u32) -> Result<Option<Vec<u8>>, ChqError> {
    if cell == EMPTY_CELL {
        return Ok(None);
    }
    let entry = file.entries.get(cell as usize - 1).ok_or(ChqError::Invalid("ссылка на несуществующий тайл"))?;
    let (x0, y0, x1, y1) = grid.bounds_at(position);
    if u32::from(entry.width) != x1 - x0 || u32::from(entry.height) != y1 - y0 {
        return Err(ChqError::Invalid("тайл другого размера"));
    }
    decode_tile(entry, file.blob(entry)?).map(Some)
}

fn put_cell(raster: &mut Raster, grid: TileGrid, position: usize, pixels: Option<&[u8]>) {
    let (x0, y0, x1, y1) = grid.bounds_at(position);
    let row_bytes = (x1 - x0) as usize * BYTES_PER_PIXEL;
    for (index, line) in (y0..y1).enumerate() {
        let target = raster.row_bytes_mut(line, x0, x1);
        match pixels {
            Some(pixels) => target.copy_from_slice(&pixels[index * row_bytes..(index + 1) * row_bytes]),
            None => target.fill(0),
        }
    }
}

fn decode_layer(file: &layout::Parsed, grid: TileGrid, cells: &[u32]) -> Result<Raster, ChqError> {
    let tiles = cells
        .par_iter()
        .enumerate()
        .map(|(position, &cell)| decode_cell(file, grid, position, cell))
        .collect::<Result<Vec<_>, ChqError>>()?;
    let mut raster = Raster::transparent(grid.width, grid.height);
    for (position, tile) in tiles.iter().enumerate() {
        if tile.is_some() {
            put_cell(&mut raster, grid, position, tile.as_deref());
        }
    }
    Ok(raster)
}

fn required_chunk<'a>(file: &'a layout::Parsed, tag: Tag) -> Result<&'a [u8], ChqError> {
    file.chunk(tag).ok_or(ChqError::Invalid("нет обязательного блока"))
}

fn read_single_frame_cels(file: &layout::Parsed, grid: TileGrid, layers: usize, frames: usize) -> Result<Vec<Vec<Arc<Raster>>>, ChqError> {
    if frames != 1 {
        return Err(ChqError::Invalid("нет сетки ячеек анимации"));
    }
    let mut reader = Reader::new(required_chunk(file, CELLS_TAG)?);
    let columns = (0..layers)
        .map(|_| Ok(vec![Arc::new(decode_layer(file, grid, &read_cells(&mut reader, grid.cells())?)?)]))
        .collect::<Result<_, ChqError>>()?;
    if !reader.is_empty() {
        return Err(ChqError::Invalid("лишние данные в сетке тайлов"));
    }
    Ok(columns)
}

pub fn decode_document(bytes: &[u8]) -> Result<Document, ChqError> {
    if layout::is_legacy(bytes) {
        return legacy::decode_document(bytes);
    }
    let file = layout::parse(bytes)?;
    file.reject_unknown_critical(&KNOWN_TAGS)?;
    let info: DocInfo = serde_json::from_slice(required_chunk(&file, DOC_TAG)?)?;
    let entries: Vec<LayerEntry> = serde_json::from_slice(required_chunk(&file, LAYERS_TAG)?)?;
    if entries.is_empty() || entries.len() > MAX_LAYERS {
        return Err(ChqError::Invalid("число слоёв вне допустимого"));
    }
    if info.frames.is_empty() || info.frames.len() > MAX_FRAMES {
        return Err(ChqError::Invalid("число кадров вне допустимого"));
    }
    crate::document::check_size(info.width, info.height)?;
    let grid = TileGrid::new(info.width, info.height, 1 << file.tile_log2);
    let columns = match file.chunk(ANIMATED_CELLS_TAG) {
        Some(payload) => cels::read(&mut Reader::new(payload), &file, grid, entries.len(), info.frames.len())?,
        None => read_single_frame_cels(&file, grid, entries.len(), info.frames.len())?,
    };
    let layers = entries.iter().zip(columns).map(|(entry, cels)| entry.layer(cels)).collect();
    let frames = info.frames.iter().map(|frame| Frame::clamped(frame.duration_ms)).collect();
    let mut document = Document::from_frames(layers, frames, info.active)?;
    document.palette = file.chunk(PALETTE_TAG).map(read_palette).transpose()?;
    let readable_history = file.chunk(HISTORY_TAG).and_then(|payload| history::read(&file, payload).ok());
    document.history = readable_history.unwrap_or_default();
    Ok(document)
}

pub fn decode_thumbnail(bytes: &[u8]) -> Result<Option<Raster>, ChqError> {
    if layout::is_legacy(bytes) {
        return Ok(None);
    }
    let file = layout::parse(bytes)?;
    let Some(payload) = file.chunk(THUMBNAIL_TAG) else {
        return Ok(None);
    };
    let mut reader = Reader::new(payload);
    let entry = TileEntry::read(&mut reader)?;
    let blob = reader.take(entry.length as usize)?;
    let rgba = decode_tile(&entry, blob)?;
    Ok(Some(Raster::from_rgba(u32::from(entry.width), u32::from(entry.height), rgba)?))
}

pub fn save_document(path: &Path, document: &Document, mode: WriteMode, keep_history: bool) -> Result<(), ChqError> {
    let bytes = encode_document(document, mode, keep_history)?;
    let mut temp_name = OsString::from(path.file_name().ok_or(ChqError::Invalid("у пути нет имени файла"))?);
    temp_name.push(TEMP_SUFFIX);
    let temp = path.with_file_name(temp_name);
    let written = File::create(&temp).and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all())).and_then(|()| fs::rename(&temp, path));
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    Ok(written?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::IPoint;

    fn sample() -> Document {
        let mut raster = Raster::filled(150, 70, [10, 20, 30, 255]);
        for x in 5..120 {
            raster.set(x, 40, [(x * 2) as u8, 7, 200, 180]);
        }
        for y in 0..70 {
            for x in 0..150 {
                if (x / 10 + y / 10) % 2 == 0 {
                    raster.set(x, y, [(x * 7) as u8, (y * 11) as u8, (x * y) as u8, 255]);
                }
            }
        }
        let bottom = Layer::new("фон".into(), raster);
        let mut top = Layer::new("верх".into(), Raster::transparent(150, 70));
        top.raster_mut().set(149, 69, [1, 2, 3, 4]);
        top.opacity = 128;
        top.blend = BlendMode::Multiply;
        top.visible = false;
        Document::from_layers(vec![bottom, top], 1).unwrap()
    }

    #[test]
    fn document_round_trips_pixels_and_layer_props_in_both_modes() {
        for mode in [WriteMode::Fast, WriteMode::Small] {
            let original = sample();
            let restored = decode_document(&encode_document(&original, mode, false).unwrap()).unwrap();
            assert_eq!(restored.layers().len(), 2);
            for (a, b) in original.layers().iter().zip(restored.layers()) {
                assert_eq!(a.raster(), b.raster());
                assert_eq!((&a.name, a.visible, a.opacity, a.blend), (&b.name, b.visible, b.opacity, b.blend));
            }
            assert!(restored.bounds().contains(IPoint::new(149, 69)));
        }
    }

    #[test]
    fn every_tile_kind_round_trips() {
        let mut raster = Raster::transparent(256, 64);
        for y in 0..64 {
            for x in 0..64 {
                raster.set(x, y, [(x * 4) as u8, (y * 4) as u8, (x ^ y) as u8, 255]);
                raster.set(64 + x, y, [(x * 4) as u8, (y * 4) as u8, (x ^ y) as u8, (x + y) as u8]);
                raster.set(128 + x, y, [(x % 4) as u8 * 60, 0, (y % 3) as u8 * 90, 255]);
                raster.set(192 + x, y, [5, 6, 7, 255]);
            }
        }
        let original = Document::from_layers(vec![Layer::new("k".into(), raster)], 0).unwrap();
        let restored = decode_document(&encode_document(&original, WriteMode::Small, false).unwrap()).unwrap();
        assert_eq!(original.layers()[0].raster(), restored.layers()[0].raster());
    }

    #[test]
    fn palette_round_trips_and_absent_palette_stays_absent() {
        let mut document = sample();
        assert!(decode_document(&encode_document(&document, WriteMode::Fast, false).unwrap()).unwrap().palette.is_none());
        let palette = DocumentPalette { name: "пиксель".into(), colors: vec![[1, 2, 3], [250, 251, 252]] };
        document.palette = Some(palette.clone());
        assert_eq!(decode_document(&encode_document(&document, WriteMode::Fast, false).unwrap()).unwrap().palette, Some(palette));
    }

    #[test]
    fn identical_tiles_are_stored_once() {
        let document = Document::new(640, 640, [9, 9, 9, 255], "x".into()).unwrap();
        assert!(encode_document(&document, WriteMode::Small, false).unwrap().len() < 1500);
    }

    #[test]
    fn thumbnail_fits_the_limit_and_keeps_aspect() {
        let document = Document::new(1000, 500, [200, 10, 10, 255], "x".into()).unwrap();
        let thumbnail = decode_thumbnail(&encode_document(&document, WriteMode::Fast, false).unwrap()).unwrap().unwrap();
        assert_eq!((thumbnail.width(), thumbnail.height()), (256, 128));
        assert_eq!(thumbnail.get(10, 10), [200, 10, 10, 255]);
    }

    #[test]
    fn damaged_or_truncated_files_are_errors_not_panics() {
        let bytes = encode_document(&Document::new(70, 70, [3, 4, 5, 255], "x".into()).unwrap(), WriteMode::Small, false).unwrap();
        for cut in 0..bytes.len() {
            assert!(decode_document(&bytes[..cut]).is_err());
        }
        for position in 0..bytes.len() {
            let mut damaged = bytes.clone();
            damaged[position] ^= 0xFF;
            let _ = decode_document(&damaged);
        }
    }

    #[test]
    fn save_replaces_the_file_without_leaving_the_temp_one() {
        let dir = std::env::temp_dir().join(format!("chq-save-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.chq");
        let document = sample();
        save_document(&path, &document, WriteMode::Small, false).unwrap();
        save_document(&path, &document, WriteMode::Small, false).unwrap();
        assert!(decode_document(&fs::read(&path).unwrap()).is_ok());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
