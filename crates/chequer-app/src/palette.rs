
use std::collections::HashMap;
use std::path::Path;

use chequer_core::document::DocumentPalette;
use chequer_core::raster::Raster;

use crate::color::{hsv_to_rgb, Hsv};

pub const MAX_COLORS: usize = 256;
const DEFAULT_COLUMNS: usize = 16;
const DEFAULT_SATURATIONS: [f32; 4] = [1.0, 0.75, 0.5, 0.25];
const DEFAULT_VALUES: [f32; 3] = [0.75, 0.5, 0.25];
const GPL_HEADER: &str = "GIMP Palette";
const JASC_HEADER: &str = "JASC-PAL";
const JASC_VERSION: &str = "0100";
const GPL_NAME_KEY: &str = "Name:";
const GPL_COLUMNS_KEY: &str = "Columns:";
const CHANNELS: usize = 3;
const JASC_HEADER_LINES: usize = 3;
const HEX_RGB_LEN: usize = 6;
const HEX_RGBA_LEN: usize = 8;

pub type Rgb = [u8; 3];

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<Rgb>,
}

impl From<&Palette> for DocumentPalette {
    fn from(palette: &Palette) -> Self {
        Self { name: palette.name.clone(), colors: palette.colors.clone() }
    }
}

impl From<DocumentPalette> for Palette {
    fn from(palette: DocumentPalette) -> Self {
        Self { name: palette.name, colors: palette.colors }
    }
}

#[derive(Debug)]
pub enum PaletteError {
    Io(std::io::Error),
    Image(chequer_core::io::IoError),
    Empty,
}

impl std::fmt::Display for PaletteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Image(e) => e.fmt(f),
            Self::Empty => write!(f, "нет цветов"),
        }
    }
}

impl From<std::io::Error> for PaletteError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextFormat {
    Gpl,
    Hex,
    Jasc,
}

impl TextFormat {
    fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "gpl" => Some(Self::Gpl),
            "hex" => Some(Self::Hex),
            "pal" => Some(Self::Jasc),
            _ => None,
        }
    }
}

pub const FILE_EXTENSIONS: [&str; 3] = ["gpl", "hex", "pal"];

impl Default for Palette {
    fn default() -> Self {
        let mut colors: Vec<Rgb> = (0..DEFAULT_COLUMNS)
            .map(|column| {
                let level = (column as f32 / (DEFAULT_COLUMNS - 1) as f32 * 255.0).round() as u8;
                [level, level, level]
            })
            .collect();
        let rows = DEFAULT_SATURATIONS.iter().map(|&s| (s, 1.0)).chain(DEFAULT_VALUES.iter().map(|&v| (1.0, v)));
        for (saturation, value) in rows {
            for column in 0..DEFAULT_COLUMNS {
                let h = column as f32 / DEFAULT_COLUMNS as f32 * 360.0;
                colors.push(hsv_to_rgb(Hsv { h, s: saturation, v: value }));
            }
        }
        Self { name: "Default".to_owned(), colors }
    }
}

impl Palette {
    pub fn contains(&self, color: Rgb) -> bool {
        self.position(color).is_some()
    }

    pub fn position(&self, color: Rgb) -> Option<usize> {
        self.colors.iter().position(|&c| c == color)
    }

    pub fn add(&mut self, color: Rgb) -> bool {
        if self.contains(color) || self.colors.len() >= MAX_COLORS {
            return false;
        }
        self.colors.push(color);
        true
    }

    pub fn remove(&mut self, color: Rgb) -> bool {
        self.position(color).map(|index| self.colors.remove(index)).is_some()
    }

    pub fn load(path: &Path) -> Result<Self, PaletteError> {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned();
        let palette = match TextFormat::from_extension(extension) {
            Some(format) => parse_text(&std::fs::read_to_string(path)?, format, name),
            None => {
                let raster = chequer_core::io::load_raster(path).map_err(PaletteError::Image)?;
                Self { name, colors: colors_of_raster(&raster, MAX_COLORS) }
            }
        };
        if palette.colors.is_empty() { Err(PaletteError::Empty) } else { Ok(palette) }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        let text = match TextFormat::from_extension(extension).unwrap_or(TextFormat::Gpl) {
            TextFormat::Gpl => self.to_gpl(),
            TextFormat::Hex => self.to_hex(),
            TextFormat::Jasc => self.to_jasc(),
        };
        std::fs::write(path, text)
    }

    pub fn to_gpl(&self) -> String {
        let mut text = format!("{GPL_HEADER}\n{GPL_NAME_KEY} {}\n{GPL_COLUMNS_KEY} {DEFAULT_COLUMNS}\n#\n", self.name);
        for [r, g, b] in &self.colors {
            text.push_str(&format!("{r:3} {g:3} {b:3}\t#{r:02X}{g:02X}{b:02X}\n"));
        }
        text
    }

    pub fn to_hex(&self) -> String {
        self.colors.iter().map(|[r, g, b]| format!("{r:02x}{g:02x}{b:02x}\n")).collect()
    }

    pub fn to_jasc(&self) -> String {
        let mut text = format!("{JASC_HEADER}\n{JASC_VERSION}\n{}\n", self.colors.len());
        for [r, g, b] in &self.colors {
            text.push_str(&format!("{r} {g} {b}\n"));
        }
        text
    }
}

fn parse_text(text: &str, format: TextFormat, name: String) -> Palette {
    let mut palette = Palette { name, colors: Vec::new() };
    let mut lines = text.lines().map(str::trim);
    match format {
        TextFormat::Gpl => {
            for line in lines {
                if let Some(title) = line.strip_prefix(GPL_NAME_KEY) {
                    palette.name = title.trim().to_owned();
                } else if let Some(color) = parse_channels(line) {
                    palette.colors.push(color);
                }
            }
        }
        TextFormat::Hex => palette.colors.extend(lines.filter_map(parse_hex_line)),
        TextFormat::Jasc => {
            palette.colors.extend(lines.by_ref().skip(JASC_HEADER_LINES).filter_map(parse_channels));
        }
    }
    palette.colors.truncate(MAX_COLORS);
    palette
}

fn parse_channels(line: &str) -> Option<Rgb> {
    let mut numbers = line.split_whitespace().map(|word| word.parse::<u8>());
    let (r, g, b) = (numbers.next()?.ok()?, numbers.next()?.ok()?, numbers.next()?.ok()?);
    Some([r, g, b])
}

fn parse_hex_line(line: &str) -> Option<Rgb> {
    let digits = line.trim_start_matches('#');
    if digits.len() != HEX_RGB_LEN && digits.len() != HEX_RGBA_LEN {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

pub fn colors_of_raster(raster: &Raster, limit: usize) -> Vec<Rgb> {
    let mut counts: HashMap<Rgb, usize> = HashMap::new();
    let mut order: Vec<Rgb> = Vec::new();
    for pixel in raster.as_bytes().chunks_exact(4) {
        if pixel[3] == 0 {
            continue;
        }
        let rgb = [pixel[0], pixel[1], pixel[2]];
        let count = counts.entry(rgb).or_insert_with(|| {
            order.push(rgb);
            0
        });
        *count += 1;
    }
    if order.len() <= limit { order } else { median_cut(order.iter().map(|rgb| (*rgb, counts[rgb])).collect(), limit) }
}

fn median_cut(colors: Vec<(Rgb, usize)>, limit: usize) -> Vec<Rgb> {
    let mut boxes = vec![colors];
    while boxes.len() < limit {
        let Some(widest) = (0..boxes.len()).filter(|&i| boxes[i].len() > 1).max_by_key(|&i| widest_axis(&boxes[i]).1) else {
            break;
        };
        let mut part = boxes.swap_remove(widest);
        let (axis, _) = widest_axis(&part);
        part.sort_by_key(|(rgb, _)| rgb[axis]);
        let upper = part.split_off(part.len() / 2);
        boxes.push(part);
        boxes.push(upper);
    }
    boxes.iter().map(|part| weighted_mean(part)).collect()
}

fn widest_axis(colors: &[(Rgb, usize)]) -> (usize, u8) {
    (0..CHANNELS)
        .map(|axis| {
            let min = colors.iter().map(|(rgb, _)| rgb[axis]).min().unwrap_or(0);
            let max = colors.iter().map(|(rgb, _)| rgb[axis]).max().unwrap_or(0);
            (axis, max - min)
        })
        .max_by_key(|&(_, spread)| spread)
        .unwrap_or((0, 0))
}

fn weighted_mean(colors: &[(Rgb, usize)]) -> Rgb {
    let total: usize = colors.iter().map(|(_, count)| count).sum::<usize>().max(1);
    let channel = |axis: usize| (colors.iter().map(|(rgb, count)| rgb[axis] as usize * count).sum::<usize>() as f32 / total as f32).round() as u8;
    [channel(0), channel(1), channel(2)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_round_trip() {
        let palette = Palette { name: "t".into(), colors: vec![[1, 2, 3], [255, 128, 0]] };
        assert_eq!(parse_text(&palette.to_gpl(), TextFormat::Gpl, String::new()), palette);
        assert_eq!(parse_text(&palette.to_jasc(), TextFormat::Jasc, "t".into()), palette);
        assert_eq!(parse_text(&palette.to_hex(), TextFormat::Hex, "t".into()), palette);
    }

    #[test]
    fn raster_colors_stay_within_limit() {
        let mut raster = Raster::transparent(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                raster.set(x, y, [x as u8 * 16, y as u8 * 16, 0, 255]);
            }
        }
        assert_eq!(colors_of_raster(&raster, 300).len(), 256);
        assert_eq!(colors_of_raster(&raster, 8).len(), 8);
    }
}
