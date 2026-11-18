use super::Reader;
use crate::brush::{Brush, BrushError, BrushMeta, BrushSettings, BrushTip, MAX_TIP_SIDE, MaskTip};

const SECTION_SIGNATURE: &[u8; 4] = b"8BIM";
const SAMPLES_KEY: &[u8; 4] = b"samp";
const SAMPLED_KIND: u16 = 2;
const NAMED_VERSION: u16 = 2;
const MODERN_VERSIONS: [u16; 2] = [6, 10];
const FIRST_SUBVERSION: u16 = 1;
const FIRST_SUBVERSION_SKIP: usize = 47;
const LATER_SUBVERSION_SKIP: usize = 301;
const ITEM_ALIGNMENT: usize = 4;
const CLASSIC_BOUNDS_SHORTS: usize = 4;
const BITS_PER_BYTE: u16 = 8;
const SPACING_PERCENT: f32 = 100.0;
const MODERN_SPACING: f32 = 0.25;
const STORED_RUNS: i8 = -128;

struct Sample {
    width: u32,
    height: u32,
    depth_bits: u16,
    compressed: bool,
}

pub fn decode_abr(bytes: &[u8], stem: &str) -> Result<Vec<Brush>, BrushError> {
    let mut reader = Reader::new(bytes);
    let version = reader.u16()?;
    let count_or_subversion = reader.u16()?;
    let brushes = match version {
        1 | 2 => read_classic(&mut reader, version, count_or_subversion, stem)?,
        v if MODERN_VERSIONS.contains(&v) => read_modern(&mut reader, count_or_subversion, stem)?,
        other => return Err(BrushError::Unsupported(format!("версия ABR {other}"))),
    };
    if brushes.is_empty() { Err(BrushError::Empty) } else { Ok(brushes) }
}

fn read_classic(reader: &mut Reader, version: u16, count: u16, stem: &str) -> Result<Vec<Brush>, BrushError> {
    let mut brushes = Vec::new();
    for index in 0..count {
        let kind = reader.u16()?;
        let size = reader.u32()? as usize;
        let block = reader.take(size)?;
        if kind != SAMPLED_KIND {
            continue;
        }
        let mut block = Reader::new(block);
        block.skip(std::mem::size_of::<u32>())?;
        let spacing = block.u16()? as f32 / SPACING_PERCENT;
        let name = if version == NAMED_VERSION { block.ucs2_text()? } else { String::new() };
        block.skip(std::mem::size_of::<u8>() + CLASSIC_BOUNDS_SHORTS * std::mem::size_of::<u16>())?;
        if let Some(brush) = read_sample(&mut block, index, stem, name, spacing)? {
            brushes.push(brush);
        }
    }
    Ok(brushes)
}

fn read_modern(reader: &mut Reader, subversion: u16, stem: &str) -> Result<Vec<Brush>, BrushError> {
    while reader.remaining() >= SECTION_SIGNATURE.len() * 2 + std::mem::size_of::<u32>() {
        if reader.take(SECTION_SIGNATURE.len())? != SECTION_SIGNATURE {
            break;
        }
        let key = reader.take(SAMPLES_KEY.len())?;
        let length = reader.u32()? as usize;
        let body = reader.take(length)?;
        if key == SAMPLES_KEY {
            return read_samples(body, subversion, stem);
        }
    }
    Err(BrushError::Malformed("нет секции с наконечниками"))
}

fn read_samples(body: &[u8], subversion: u16, stem: &str) -> Result<Vec<Brush>, BrushError> {
    let skip = if subversion == FIRST_SUBVERSION { FIRST_SUBVERSION_SKIP } else { LATER_SUBVERSION_SKIP };
    let mut reader = Reader::new(body);
    let mut brushes = Vec::new();
    let mut index = 0;
    while reader.remaining() >= std::mem::size_of::<u32>() {
        let size = reader.u32()? as usize;
        let padded = size.next_multiple_of(ITEM_ALIGNMENT).min(reader.remaining());
        let mut item = Reader::new(reader.take(padded)?);
        item.skip(skip)?;
        if let Some(brush) = read_sample(&mut item, index, stem, String::new(), MODERN_SPACING)? {
            brushes.push(brush);
        }
        index += 1;
    }
    Ok(brushes)
}

fn read_sample(reader: &mut Reader, index: u16, stem: &str, name: String, spacing: f32) -> Result<Option<Brush>, BrushError> {
    let (top, left, bottom, right) = (reader.i32()?, reader.i32()?, reader.i32()?, reader.i32()?);
    let depth_bits = reader.u16()?;
    let compressed = reader.u8()? != 0;
    let (width, height) = (right.saturating_sub(left), bottom.saturating_sub(top));
    let in_range = |side: i32| (1..=MAX_TIP_SIDE as i32).contains(&side);
    if !in_range(width) || !in_range(height) {
        return Err(BrushError::TipSize { width: width.max(0) as u32, height: height.max(0) as u32 });
    }
    let sample = Sample { width: width as u32, height: height as u32, depth_bits, compressed };
    let Some(mask) = read_mask(reader, &sample)? else {
        return Ok(None);
    };
    let name = if name.is_empty() { format!("{stem}-{:03}", index + 1) } else { name };
    let settings = BrushSettings { spacing, ..BrushSettings::default() }.clamped();
    Ok(Some(Brush { meta: BrushMeta { name, ..BrushMeta::default() }, settings, tip: BrushTip::Mask(MaskTip::new(sample.width, sample.height, mask)?), textures: Default::default() }))
}

fn read_mask(reader: &mut Reader, sample: &Sample) -> Result<Option<Vec<u8>>, BrushError> {
    if sample.depth_bits != BITS_PER_BYTE {
        return Ok(None);
    }
    let (width, height) = (sample.width as usize, sample.height as usize);
    if !sample.compressed {
        return Ok(Some(reader.take(width * height)?.to_vec()));
    }
    let run_lengths = (0..height).map(|_| reader.u16().map(usize::from)).collect::<Result<Vec<_>, _>>()?;
    let mut mask = Vec::with_capacity(width * height);
    for length in run_lengths {
        mask.extend(unpack_row(reader.take(length)?, width)?);
    }
    Ok(Some(mask))
}

fn unpack_row(packed: &[u8], width: usize) -> Result<Vec<u8>, BrushError> {
    const BAD_RUNS: BrushError = BrushError::Malformed("строка PackBits повреждена");
    let mut row = Vec::with_capacity(width);
    let mut at = 0;
    while row.len() < width {
        let control = *packed.get(at).ok_or(BAD_RUNS)? as i8;
        at += 1;
        if control == STORED_RUNS {
            continue;
        }
        if control >= 0 {
            let literal = control as usize + 1;
            row.extend_from_slice(packed.get(at..at + literal).ok_or(BAD_RUNS)?);
            at += literal;
        } else {
            let value = *packed.get(at).ok_or(BAD_RUNS)?;
            at += 1;
            row.resize(row.len() + (1 - control as isize) as usize, value);
        }
    }
    if row.len() == width { Ok(row) } else { Err(BAD_RUNS) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packbits_row_mixes_literals_and_repeats() {
        assert_eq!(unpack_row(&[1, 7, 8, 254, 9], 5).unwrap(), vec![7, 8, 9, 9, 9]);
        assert!(unpack_row(&[1, 7], 5).is_err());
    }

    #[test]
    fn classic_sampled_tip_is_read_with_spacing() {
        let mut block = vec![0, 0, 0, 0, 0, 50, 0];
        block.extend([0; 8]);
        for value in [0i32, 0, 2, 2] {
            block.extend(value.to_be_bytes());
        }
        block.extend(8u16.to_be_bytes());
        block.push(0);
        block.extend([0, 100, 200, 255]);
        let mut file = vec![0, 1, 0, 1, 0, 2];
        file.extend((block.len() as u32).to_be_bytes());
        file.extend(block);
        let brushes = decode_abr(&file, "set").unwrap();
        assert_eq!(brushes.len(), 1);
        assert_eq!(brushes[0].meta.name, "set-001");
        assert_eq!(brushes[0].settings.spacing, 0.5);
    }
}
