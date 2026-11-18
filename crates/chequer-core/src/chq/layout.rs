use crate::container::{Chunk, ContainerError, Tag};

use super::ChqError;
use super::bytes::Reader;
use super::tile::{Codec, ENTRY_SIZE, EncodedTile, TileEntry, compress, decompress};

pub const MAGIC: Tag = *b"CHQ\0";
pub const FORMAT_MAJOR: u16 = 0;
pub const FORMAT_MINOR: u16 = 2;
pub const MIN_TILE_LOG2: u8 = 5;
pub const MAX_TILE_LOG2: u8 = 8;
const LEGACY_MINOR: u16 = 1;
const FOOTER_MAGIC: Tag = *b"CHQ!";
const FOOTER_TAG: Tag = *b"FOOT";
const TABLE_TAG: Tag = *b"TABL";
const MANIFEST_TAG: Tag = *b"MNFT";
const TILE_TAG: Tag = *b"TILE";
const MAGIC_SIZE: usize = 4;
const MINOR_OFFSET: usize = 6;
const HEADER_FLAGS: u32 = 0;
const HEADER_RESERVED: [u8; 3] = [0; 3];
const HEADER_SIZE: usize = 16;
const FOOTER_SIZE: usize = 48;
const FOOTER_BODY_SIZE: usize = FOOTER_SIZE - CRC_SIZE - MAGIC_SIZE;
const CRC_SIZE: usize = 4;
const GENERATION_SIZE: usize = 4;
const MINOR_AND_FLAGS_SIZE: usize = 6;
const FIRST_GENERATION: u32 = 1;
const MAX_TILES: usize = 1 << 22;
const MAX_MANIFEST_SIZE: usize = 128 * 1024 * 1024;
const CRITICAL_FLAG: u8 = 1;

struct Span {
    offset: u64,
    length: u32,
    crc: u32,
}

impl Span {
    fn of(offset: usize, bytes: &[u8]) -> Self {
        Self { offset: offset as u64, length: bytes.len() as u32, crc: crc32fast::hash(bytes) }
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.offset.to_le_bytes());
        out.extend_from_slice(&self.length.to_le_bytes());
        out.extend_from_slice(&self.crc.to_le_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ChqError> {
        Ok(Self { offset: reader.u64()?, length: reader.u32()?, crc: reader.u32()? })
    }

    fn bytes<'a>(&self, file: &'a [u8], end: usize) -> Result<&'a [u8], ChqError> {
        let start = usize::try_from(self.offset).ok().filter(|&start| start >= HEADER_SIZE).ok_or(ContainerError::Truncated)?;
        let stop = start.checked_add(self.length as usize).filter(|&stop| stop <= end).ok_or(ContainerError::Truncated)?;
        Ok(&file[start..stop])
    }

    fn verified<'a>(&self, file: &'a [u8], end: usize, tag: Tag) -> Result<&'a [u8], ChqError> {
        let bytes = self.bytes(file, end)?;
        if crc32fast::hash(bytes) != self.crc {
            return Err(ContainerError::Corrupt { tag }.into());
        }
        Ok(bytes)
    }
}

pub fn is_legacy(bytes: &[u8]) -> bool {
    bytes.get(..MAGIC_SIZE) == Some(&MAGIC[..]) && bytes.get(MINOR_OFFSET..MINOR_OFFSET + 2) == Some(&LEGACY_MINOR.to_le_bytes()[..])
}

pub fn write_chunks(chunks: &[Chunk]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in chunks {
        out.extend_from_slice(&chunk.tag);
        out.push(if chunk.critical { CRITICAL_FLAG } else { 0 });
        out.extend_from_slice(&(chunk.payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&chunk.payload);
    }
    out
}

fn read_chunks(bytes: &[u8]) -> Result<Vec<Chunk>, ChqError> {
    let mut reader = Reader::new(bytes);
    let mut chunks = Vec::new();
    while !reader.is_empty() {
        let tag = reader.tag()?;
        let critical = reader.u8()? & CRITICAL_FLAG != 0;
        let length = reader.u32()? as usize;
        chunks.push(Chunk { tag, critical, payload: reader.take(length)?.to_vec() });
    }
    Ok(chunks)
}

pub fn assemble(tile_log2: u8, tiles: &mut [EncodedTile], manifest: &[u8]) -> Result<Vec<u8>, ChqError> {
    if tiles.len() > MAX_TILES {
        return Err(ChqError::Unsupported("больше 4 млн тайлов, например из-за очень длинной истории"));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_MAJOR.to_le_bytes());
    out.extend_from_slice(&FORMAT_MINOR.to_le_bytes());
    out.extend_from_slice(&HEADER_FLAGS.to_le_bytes());
    out.push(tile_log2);
    out.extend_from_slice(&HEADER_RESERVED);
    for tile in tiles.iter_mut().filter(|tile| !tile.blob.is_empty()) {
        tile.entry.offset = out.len() as u64;
        out.extend_from_slice(&tile.blob);
    }
    let mut table = Vec::with_capacity(tiles.len() * ENTRY_SIZE);
    for tile in tiles.iter() {
        tile.entry.write(&mut table);
    }
    let table_packed = compress(Codec::Zstd, &table)?;
    let table_span = Span::of(out.len(), &table_packed);
    out.extend_from_slice(&table_packed);
    let manifest_packed = compress(Codec::Zstd, manifest)?;
    let manifest_span = Span::of(out.len(), &manifest_packed);
    out.extend_from_slice(&manifest_packed);
    let footer_start = out.len();
    manifest_span.write(&mut out);
    table_span.write(&mut out);
    out.extend_from_slice(&(tiles.len() as u32).to_le_bytes());
    out.extend_from_slice(&FIRST_GENERATION.to_le_bytes());
    let footer_crc = crc32fast::hash(&out[footer_start..]);
    out.extend_from_slice(&footer_crc.to_le_bytes());
    out.extend_from_slice(&FOOTER_MAGIC);
    Ok(out)
}

pub struct Parsed<'a> {
    pub tile_log2: u8,
    pub entries: Vec<TileEntry>,
    pub chunks: Vec<Chunk>,
    file: &'a [u8],
    blobs_end: usize,
}

impl Parsed<'_> {
    pub fn blob(&self, entry: &TileEntry) -> Result<&[u8], ChqError> {
        if entry.length == 0 {
            return Ok(&[]);
        }
        Span { offset: entry.offset, length: entry.length, crc: entry.crc }.bytes(self.file, self.blobs_end).map_err(|_| ContainerError::Corrupt { tag: TILE_TAG }.into())
    }

    pub fn chunk(&self, tag: Tag) -> Option<&[u8]> {
        self.chunks.iter().find(|chunk| chunk.tag == tag).map(|chunk| chunk.payload.as_slice())
    }

    pub fn reject_unknown_critical(&self, known: &[Tag]) -> Result<(), ChqError> {
        match self.chunks.iter().find(|chunk| chunk.critical && !known.contains(&chunk.tag)) {
            Some(chunk) => Err(ContainerError::UnknownCritical { tag: chunk.tag }.into()),
            None => Ok(()),
        }
    }
}

pub fn parse(file: &[u8]) -> Result<Parsed<'_>, ChqError> {
    if file.len() < HEADER_SIZE + FOOTER_SIZE {
        return Err(ContainerError::Truncated.into());
    }
    let mut header = Reader::new(&file[..HEADER_SIZE]);
    if header.tag()? != MAGIC {
        return Err(ContainerError::BadMagic.into());
    }
    let major = header.u16()?;
    if major != FORMAT_MAJOR {
        return Err(ContainerError::UnsupportedMajor { found: major, supported: FORMAT_MAJOR }.into());
    }
    header.take(MINOR_AND_FLAGS_SIZE)?;
    let tile_log2 = header.u8()?;
    if !(MIN_TILE_LOG2..=MAX_TILE_LOG2).contains(&tile_log2) {
        return Err(ChqError::Invalid("размер тайла вне допустимого"));
    }
    let footer_start = file.len() - FOOTER_SIZE;
    let footer = &file[footer_start..];
    let corrupt_footer = ContainerError::Corrupt { tag: FOOTER_TAG };
    if footer[FOOTER_SIZE - MAGIC_SIZE..] != FOOTER_MAGIC {
        return Err(corrupt_footer.into());
    }
    let mut footer_reader = Reader::new(footer);
    let (manifest_span, table_span) = (Span::read(&mut footer_reader)?, Span::read(&mut footer_reader)?);
    let tile_count = footer_reader.u32()? as usize;
    footer_reader.take(GENERATION_SIZE)?;
    if footer_reader.u32()? != crc32fast::hash(&footer[..FOOTER_BODY_SIZE]) || tile_count > MAX_TILES {
        return Err(corrupt_footer.into());
    }
    let table = decompress(Codec::Zstd, table_span.verified(file, footer_start, TABLE_TAG)?, tile_count * ENTRY_SIZE)?;
    if table.len() != tile_count * ENTRY_SIZE {
        return Err(ContainerError::Corrupt { tag: TABLE_TAG }.into());
    }
    let mut table_reader = Reader::new(&table);
    let entries = (0..tile_count).map(|_| TileEntry::read(&mut table_reader)).collect::<Result<Vec<_>, _>>()?;
    let manifest = decompress(Codec::Zstd, manifest_span.verified(file, footer_start, MANIFEST_TAG)?, MAX_MANIFEST_SIZE)?;
    Ok(Parsed { tile_log2, entries, chunks: read_chunks(&manifest)?, file, blobs_end: footer_start })
}
