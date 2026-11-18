use std::collections::HashMap;
use std::io::Read;

use flate2::read::DeflateDecoder;
use serde::Deserialize;

use crate::blend::BlendMode;
use crate::container::{Chunk, Container, Tag};
use crate::document::{Document, Layer};
use crate::raster::Raster;

use super::layout::{FORMAT_MAJOR, MAGIC};
use super::{BYTES_PER_PIXEL, ChqError, EMPTY_CELL, MAX_LAYERS, TileGrid};

const MANIFEST_TAG: Tag = *b"MNFT";
const TILE_TAG: Tag = *b"TILE";
const KNOWN_TAGS: [Tag; 2] = [MANIFEST_TAG, TILE_TAG];
const MIN_TILE_SIDE: u32 = 8;
const MAX_TILE_SIDE: u32 = 1024;
const KIND_SOLID: u8 = 0;
const KIND_DEFLATED: u8 = 1;

#[derive(Deserialize)]
struct Manifest {
    width: u32,
    height: u32,
    tile_side: u32,
    active: usize,
    layers: Vec<LayerEntry>,
}

#[derive(Deserialize)]
struct LayerEntry {
    name: String,
    visible: bool,
    opacity: u8,
    blend: BlendMode,
    #[serde(default)]
    masked: bool,
    #[serde(default)]
    collapsed: bool,
    grid: Vec<u32>,
}

fn decode_tile(payload: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ChqError> {
    let expected = width as usize * height as usize * BYTES_PER_PIXEL;
    let (kind, body) = payload.split_first().ok_or(ChqError::Invalid("пустой тайл"))?;
    match *kind {
        KIND_SOLID if body.len() == BYTES_PER_PIXEL => Ok(body.iter().copied().cycle().take(expected).collect()),
        KIND_DEFLATED => {
            let mut bytes = Vec::with_capacity(expected);
            DeflateDecoder::new(body).take(expected as u64 + 1).read_to_end(&mut bytes)?;
            if bytes.len() != expected {
                return Err(ChqError::Invalid("размер тайла не совпадает"));
            }
            let row_bytes = width as usize * BYTES_PER_PIXEL;
            for row in bytes.chunks_exact_mut(row_bytes) {
                for index in BYTES_PER_PIXEL..row_bytes {
                    row[index] = row[index].wrapping_add(row[index - BYTES_PER_PIXEL]);
                }
            }
            Ok(bytes)
        }
        _ => Err(ChqError::Invalid("неизвестный вид тайла")),
    }
}

pub fn decode_document(bytes: &[u8]) -> Result<Document, ChqError> {
    let container = Container::decode(bytes, MAGIC, FORMAT_MAJOR)?;
    container.reject_unknown_critical(&KNOWN_TAGS)?;
    let manifest_chunk = container.chunks.iter().find(|chunk| chunk.tag == MANIFEST_TAG).ok_or(ChqError::Invalid("нет манифеста"))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_chunk.payload)?;
    if !(MIN_TILE_SIDE..=MAX_TILE_SIDE).contains(&manifest.tile_side) || manifest.layers.is_empty() || manifest.layers.len() > MAX_LAYERS {
        return Err(ChqError::Invalid("параметры манифеста вне допустимого"));
    }
    crate::document::check_size(manifest.width, manifest.height)?;
    let grid = TileGrid::new(manifest.width, manifest.height, manifest.tile_side);
    let tiles: Vec<&Chunk> = container.chunks.iter().filter(|chunk| chunk.tag == TILE_TAG).collect();
    let mut decoded: HashMap<u32, Vec<u8>> = HashMap::new();
    let mut layers = Vec::with_capacity(manifest.layers.len());
    for entry in manifest.layers {
        if entry.grid.len() != grid.cells() {
            return Err(ChqError::Invalid("сетка слоя не совпадает с размером"));
        }
        let mut raster = Raster::transparent(grid.width, grid.height);
        for (position, &cell) in entry.grid.iter().enumerate() {
            if cell == EMPTY_CELL {
                continue;
            }
            let (x0, y0, x1, y1) = grid.bounds_at(position);
            let payload = &tiles.get(cell as usize - 1).ok_or(ChqError::Invalid("ссылка на несуществующий тайл"))?.payload;
            let tile = match decoded.get(&cell) {
                Some(tile) => tile,
                None => decoded.entry(cell).or_insert(decode_tile(payload, x1 - x0, y1 - y0)?),
            };
            if tile.len() != (x1 - x0) as usize * (y1 - y0) as usize * BYTES_PER_PIXEL {
                return Err(ChqError::Invalid("тайл другого размера"));
            }
            let row_bytes = (x1 - x0) as usize * BYTES_PER_PIXEL;
            for (line, source) in (y0..y1).zip(tile.chunks_exact(row_bytes)) {
                raster.row_bytes_mut(line, x0, x1).copy_from_slice(source);
            }
        }
        let mut layer = Layer::new(entry.name, raster);
        layer.visible = entry.visible;
        layer.opacity = entry.opacity;
        layer.blend = entry.blend;
        layer.masked = entry.masked;
        layer.collapsed = entry.collapsed;
        layers.push(layer);
    }
    Ok(Document::from_layers(layers, manifest.active)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::Container;

    #[test]
    fn version_0_1_file_with_solid_tile_still_opens() {
        let manifest = br#"{"width":8,"height":8,"tile_side":64,"active":0,"layers":[{"name":"a","visible":true,"opacity":255,"blend":"Normal","grid":[1]}]}"#;
        let mut container = Container::new(FORMAT_MAJOR, 1);
        container.chunks.push(Chunk::critical(MANIFEST_TAG, manifest.to_vec()));
        container.chunks.push(Chunk::critical(TILE_TAG, vec![KIND_SOLID, 1, 2, 3, 4]));
        let document = decode_document(&container.encode(MAGIC)).unwrap();
        assert_eq!(document.layers()[0].raster().get(7, 7), [1, 2, 3, 4]);
    }
}
