use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use rayon::prelude::*;

use crate::document::Layer;
use crate::raster::Raster;

use super::bytes::{Reader, unzigzag, write_varint, zigzag};
use super::layout::Parsed;
use super::tile::WriteMode;
use super::{ChqError, EMPTY_CELL, TileGrid, TileStore, decode_cell, decode_layer, layer_cells, put_cell, read_cells, write_cells};

const KIND_EMPTY: u8 = 0;
const KIND_LINK: u8 = 1;
const KIND_GRID: u8 = 2;
const KIND_DELTA: u8 = 3;
const DELTA_SAME: u64 = 0;
const DELTA_EMPTY: u64 = 1;
const DELTA_ID_BASE: u64 = 2;

type Cells = Rc<Vec<u32>>;

struct LayerEncoder<'a> {
    grid: TileGrid,
    mode: WriteMode,
    store: &'a mut TileStore,
    by_pointer: HashMap<usize, usize>,
    by_cells: HashMap<Cells, usize>,
    cells_of_frame: Vec<Cells>,
}

impl LayerEncoder<'_> {
    fn encode(&mut self, out: &mut Vec<u8>, layer: &Layer, frame: usize) -> Result<(), ChqError> {
        let cel = layer.cel(frame);
        let pointer = Arc::as_ptr(cel) as usize;
        let cells = match self.by_pointer.get(&pointer) {
            Some(&source) => {
                write_link(out, frame - source);
                Rc::clone(&self.cells_of_frame[source])
            }
            None => self.encode_new(out, layer, frame)?,
        };
        self.by_pointer.insert(pointer, frame);
        self.by_cells.insert(Rc::clone(&cells), frame);
        self.cells_of_frame.push(cells);
        Ok(())
    }

    fn encode_new(&mut self, out: &mut Vec<u8>, layer: &Layer, frame: usize) -> Result<Cells, ChqError> {
        let cel = layer.cel(frame);
        if cel.is_transparent() {
            out.push(KIND_EMPTY);
            return Ok(Rc::new(vec![EMPTY_CELL; self.grid.cells()]));
        }
        let previous = frame.checked_sub(1).map(|before| (layer.cel(before).as_ref(), self.cells_of_frame[before].as_slice()));
        let cells = Rc::new(layer_cells(cel, previous, self.grid, self.mode, self.store)?);
        match self.by_cells.get(&cells) {
            Some(&source) => write_link(out, frame - source),
            None => self.write_cel(out, &cells, frame),
        }
        Ok(cells)
    }

    fn write_cel(&self, out: &mut Vec<u8>, cells: &[u32], frame: usize) {
        let mut grid_payload = Vec::new();
        write_cells(&mut grid_payload, cells);
        let delta_payload = frame.checked_sub(1).map(|before| delta_payload(cells, &self.cells_of_frame[before]));
        match delta_payload {
            Some(delta) if delta.len() < grid_payload.len() => {
                out.push(KIND_DELTA);
                out.extend(delta);
            }
            _ => {
                out.push(KIND_GRID);
                out.extend(grid_payload);
            }
        }
    }
}

fn write_link(out: &mut Vec<u8>, distance: usize) {
    out.push(KIND_LINK);
    write_varint(out, distance as u64);
}

fn delta_payload(cells: &[u32], previous: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut last_id = 0i64;
    for (&cell, &before) in cells.iter().zip(previous) {
        if cell == before {
            write_varint(&mut out, DELTA_SAME);
        } else if cell == EMPTY_CELL {
            write_varint(&mut out, DELTA_EMPTY);
        } else {
            write_varint(&mut out, DELTA_ID_BASE + zigzag(i64::from(cell) - last_id));
            last_id = i64::from(cell);
        }
    }
    out
}

pub(super) fn write(layers: &[Layer], grid: TileGrid, mode: WriteMode, store: &mut TileStore) -> Result<Vec<u8>, ChqError> {
    let mut out = Vec::new();
    for layer in layers {
        let mut encoder = LayerEncoder { grid, mode, store: &mut *store, by_pointer: HashMap::new(), by_cells: HashMap::new(), cells_of_frame: Vec::new() };
        for frame in 0..layer.frame_count() {
            encoder.encode(&mut out, layer, frame)?;
        }
    }
    Ok(out)
}

fn read_delta(reader: &mut Reader, previous: &[u32]) -> Result<Vec<u32>, ChqError> {
    let mut last_id = 0i64;
    let mut cells = Vec::with_capacity(previous.len());
    for &before in previous {
        let token = reader.varint()?;
        cells.push(match token {
            DELTA_SAME => before,
            DELTA_EMPTY => EMPTY_CELL,
            _ => {
                last_id = last_id.checked_add(unzigzag(token - DELTA_ID_BASE)).ok_or(ChqError::Invalid("номер тайла вне допустимого"))?;
                u32::try_from(last_id).ok().filter(|&id| id != EMPTY_CELL).ok_or(ChqError::Invalid("номер тайла вне допустимого"))?
            }
        });
    }
    Ok(cells)
}

fn patched(file: &Parsed, grid: TileGrid, previous: &Raster, previous_cells: &[u32], cells: &[u32]) -> Result<Raster, ChqError> {
    let changed: Vec<usize> = (0..cells.len()).filter(|&position| cells[position] != previous_cells[position]).collect();
    let tiles = changed.par_iter().map(|&position| decode_cell(file, grid, position, cells[position])).collect::<Result<Vec<_>, _>>()?;
    let mut raster = previous.clone();
    for (&position, tile) in changed.iter().zip(&tiles) {
        put_cell(&mut raster, grid, position, tile.as_deref());
    }
    Ok(raster)
}

struct LayerDecoder<'a, 'file> {
    file: &'a Parsed<'file>,
    grid: TileGrid,
    blank: Arc<Raster>,
    cels: Vec<Arc<Raster>>,
    cells_of_frame: Vec<Cells>,
}

impl LayerDecoder<'_, '_> {
    fn decode(&mut self, reader: &mut Reader) -> Result<(), ChqError> {
        let frame = self.cels.len();
        let (cel, cells) = match reader.u8()? {
            KIND_EMPTY => (Arc::clone(&self.blank), Rc::new(vec![EMPTY_CELL; self.grid.cells()])),
            KIND_LINK => {
                let distance = usize::try_from(reader.varint()?).ok().filter(|&distance| (1..=frame).contains(&distance)).ok_or(ChqError::Invalid("ссылка на несуществующий кадр"))?;
                (Arc::clone(&self.cels[frame - distance]), Rc::clone(&self.cells_of_frame[frame - distance]))
            }
            KIND_GRID => {
                let cells = read_cells(reader, self.grid.cells())?;
                (Arc::new(decode_layer(self.file, self.grid, &cells)?), Rc::new(cells))
            }
            KIND_DELTA => {
                let before = frame.checked_sub(1).ok_or(ChqError::Invalid("у первого кадра нет предыдущего"))?;
                let cells = read_delta(reader, &self.cells_of_frame[before])?;
                (Arc::new(patched(self.file, self.grid, &self.cels[before], &self.cells_of_frame[before], &cells)?), Rc::new(cells))
            }
            _ => return Err(ChqError::Invalid("неизвестный вид ячейки")),
        };
        self.cels.push(cel);
        self.cells_of_frame.push(cells);
        Ok(())
    }
}

pub(super) fn read(reader: &mut Reader, file: &Parsed, grid: TileGrid, layers: usize, frames: usize) -> Result<Vec<Vec<Arc<Raster>>>, ChqError> {
    let blank = Arc::new(Raster::transparent(grid.width, grid.height));
    let columns = (0..layers)
        .map(|_| {
            let mut decoder = LayerDecoder { file, grid, blank: Arc::clone(&blank), cels: Vec::with_capacity(frames), cells_of_frame: Vec::with_capacity(frames) };
            for _ in 0..frames {
                decoder.decode(reader)?;
            }
            Ok(decoder.cels)
        })
        .collect::<Result<_, ChqError>>()?;
    if !reader.is_empty() {
        return Err(ChqError::Invalid("лишние данные в сетке ячеек"));
    }
    Ok(columns)
}

#[cfg(test)]
mod tests {
    use crate::document::Document;
    use crate::geom::IRect;
    use crate::history::HistoryKind;
    use crate::raster::Rgba;

    use super::super::{decode_document, encode_document};
    use super::*;

    const SIDE: u32 = 200;
    const BACKGROUND: Rgba = [30, 60, 90, 255];

    fn paint(document: &mut Document, area: IRect, color: Rgba) {
        document.modify_active_layer(HistoryKind::Fill, area, |_, _, _, _| color);
    }

    fn animation() -> Document {
        let mut document = Document::new(SIDE, SIDE, BACKGROUND, "фон".into()).unwrap();
        document.add_layer("персонаж".into());
        paint(&mut document, IRect::from_size(0, 0, 10, 10), [255, 0, 0, 255]);
        document.duplicate_frame();
        paint(&mut document, IRect::from_size(70, 0, 10, 10), [0, 255, 0, 255]);
        document.duplicate_frame();
        document.add_empty_frame();
        document.duplicate_frame();
        document.set_current_frame(1);
        document.duplicate_frame();
        document.set_frame_duration(0, 40);
        document
    }

    fn assert_same_animation(a: &Document, b: &Document) {
        assert_eq!(a.frames(), b.frames());
        assert_eq!(a.layers().len(), b.layers().len());
        for (left, right) in a.layers().iter().zip(b.layers()) {
            assert_eq!(left.frame_count(), right.frame_count());
            for frame in 0..left.frame_count() {
                assert_eq!(left.cel(frame), right.cel(frame), "кадр {frame} слоя {}", left.name);
            }
        }
    }

    #[test]
    fn animation_round_trips_in_both_modes_with_every_cel_kind() {
        for mode in [WriteMode::Fast, WriteMode::Small] {
            let original = animation();
            assert!(original.frame_count() > 4);
            let restored = decode_document(&encode_document(&original, mode, false).unwrap()).unwrap();
            assert_same_animation(&original, &restored);
        }
    }

    #[test]
    fn linked_and_empty_cels_stay_shared_after_loading() {
        let mut document = Document::new(SIDE, SIDE, BACKGROUND, "фон".into()).unwrap();
        document.duplicate_frame();
        paint(&mut document, IRect::from_size(5, 5, 20, 20), [255, 0, 0, 255]);
        document.add_empty_frame();
        document.duplicate_frame();
        let restored = decode_document(&encode_document(&document, WriteMode::Fast, false).unwrap()).unwrap();
        let layer = &restored.layers()[0];
        assert_ne!(layer.cel(0), layer.cel(1));
        assert!(layer.cel(2).is_transparent());
        assert!(Arc::ptr_eq(layer.cel(2), layer.cel(3)));
    }

    #[test]
    fn unchanged_frames_cost_almost_nothing_in_the_file() {
        let still = Document::new(SIDE, SIDE, BACKGROUND, "фон".into()).unwrap();
        let mut long = Document::new(SIDE, SIDE, BACKGROUND, "фон".into()).unwrap();
        for _ in 0..200 {
            long.duplicate_frame();
        }
        let single = encode_document(&still, WriteMode::Fast, false).unwrap().len();
        let many = encode_document(&long, WriteMode::Fast, false).unwrap().len();
        assert!(many < single + 1000, "{many} против {single}");
    }

    #[test]
    fn history_with_frames_restores_and_undoes_to_the_start() {
        let mut original = animation();
        let mut restored = decode_document(&encode_document(&original, WriteMode::Fast, true).unwrap()).unwrap();
        assert_eq!(restored.history().cursor(), original.history().cursor());
        original.jump_to(0);
        restored.jump_to(0);
        assert_eq!(restored.frame_count(), 1);
        assert_same_animation(&original, &restored);
    }

    #[test]
    fn broken_animation_payloads_are_errors_not_panics() {
        let bytes = encode_document(&animation(), WriteMode::Fast, false).unwrap();
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
    fn single_frame_documents_keep_the_plain_cell_chunk() {
        let bytes = encode_document(&Document::new(70, 70, BACKGROUND, "x".into()).unwrap(), WriteMode::Fast, false).unwrap();
        let file = super::super::layout::parse(&bytes).unwrap();
        assert!(file.chunk(super::super::CELLS_TAG).is_some());
        assert!(file.chunk(super::super::ANIMATED_CELLS_TAG).is_none());
    }
}
