use super::Reader;
use crate::brush::{Brush, BrushError, BrushMeta, BrushSettings, BrushTip, MAX_TIP_SIDE, MaskTip};

const MAGIC: u32 = 0x4749_4D50;
const VERSION_ONE_HEADER: usize = 20;
const VERSION_TWO_HEADER: usize = 28;
const VERSION_ONE_SPACING_PERCENT: u32 = 25;
const SPACING_PERCENT: f32 = 100.0;
const GRAY_BYTES: u32 = 1;
const RGBA_BYTES: u32 = 4;
const ALPHA_OFFSET: usize = 3;

pub fn decode_gbr(bytes: &[u8]) -> Result<Brush, BrushError> {
    let mut reader = Reader::new(bytes);
    let header_size = reader.u32()? as usize;
    let version = reader.u32()?;
    let (width, height, depth) = (reader.u32()?, reader.u32()?, reader.u32()?);
    let (name_start, spacing_percent) = match version {
        1 => (VERSION_ONE_HEADER, VERSION_ONE_SPACING_PERCENT),
        2 => {
            if reader.u32()? != MAGIC {
                return Err(BrushError::Malformed("нет подписи GIMP"));
            }
            (VERSION_TWO_HEADER, reader.u32()?)
        }
        other => return Err(BrushError::Unsupported(format!("версия GBR {other}"))),
    };
    let name = bytes.get(name_start..header_size).ok_or(BrushError::Malformed("заголовок короче имени"))?;
    let name = String::from_utf8_lossy(name).trim_end_matches('\0').to_owned();
    if ![width, height].iter().all(|side| (1..=MAX_TIP_SIDE).contains(side)) {
        return Err(BrushError::TipSize { width, height });
    }
    reader.seek(header_size)?;
    let texels = width as usize * height as usize;
    let mask = match depth {
        GRAY_BYTES => reader.take(texels)?.to_vec(),
        RGBA_BYTES => reader.take(texels * RGBA_BYTES as usize)?.chunks_exact(RGBA_BYTES as usize).map(|pixel| pixel[ALPHA_OFFSET]).collect(),
        other => return Err(BrushError::Unsupported(format!("глубина GBR {other} байт"))),
    };
    let settings = BrushSettings { spacing: spacing_percent as f32 / SPACING_PERCENT, ..BrushSettings::default() }.clamped();
    Ok(Brush { meta: BrushMeta { name, ..BrushMeta::default() }, settings, tip: BrushTip::Mask(MaskTip::new(width, height, mask)?), textures: Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray_brush(version: u32, name: &str, spacing: u32, pixels: &[u8]) -> Vec<u8> {
        let header = if version == 1 { VERSION_ONE_HEADER } else { VERSION_TWO_HEADER };
        let mut bytes = Vec::new();
        for value in [(header + name.len() + 1) as u32, version, 2, 2, 1] {
            bytes.extend(value.to_be_bytes());
        }
        if version == 2 {
            bytes.extend(MAGIC.to_be_bytes());
            bytes.extend(spacing.to_be_bytes());
        }
        bytes.extend(name.bytes());
        bytes.push(0);
        bytes.extend(pixels);
        bytes
    }

    #[test]
    fn version_two_reads_name_spacing_and_mask() {
        let brush = decode_gbr(&gray_brush(2, "Звезда", 50, &[0, 85, 170, 255])).unwrap();
        assert_eq!(brush.meta.name, "Звезда");
        assert_eq!(brush.settings.spacing, 0.5);
        let BrushTip::Mask(mask) = brush.tip else { panic!("ожидалась маска") };
        assert_eq!(mask.pixels(), &[0, 85, 170, 255]);
    }

    #[test]
    fn version_one_has_default_spacing_and_truncated_data_is_refused() {
        let brush = decode_gbr(&gray_brush(1, "old", 0, &[255; 4])).unwrap();
        assert_eq!(brush.settings.spacing, 0.25);
        assert!(decode_gbr(&gray_brush(2, "cut", 10, &[255; 3])).is_err());
    }
}
