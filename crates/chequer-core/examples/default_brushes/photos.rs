use std::path::PathBuf;

use chequer_core::brush::{Brush, BrushSettings, BrushTip, MaskTip, TipVariant};

use super::classic::brush;

const DECOR_CATEGORY: &str = "decor";
const LIGHTNING_ID: &str = "nat.lightning";
const DAISIES_ID: &str = "nat.daisies";
const PEBBLES_ID: &str = "nat.pebbles";
const SOURCES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sources");
const LIGHTNING_SOURCE: &str = "lightning_loop.png";
const DAISY_SOURCE: &str = "osteospermum_1.png";
const PEBBLE_SOURCES: [&str; 3] = ["stone_1.png", "stone_2.png", "stone_3.png"];
const LICENSE: &str = "CC0";
const LIGHTNING_CREDIT: &str = "Wikimedia Commons, File:Lightning In Sky.jpg";
const DAISY_CREDIT: &str = "Wikimedia Commons, File:White daisy (Unsplash).jpg";
const PEBBLE_CREDIT: &str = "Wikimedia Commons, File:Light grey stones.jpg";

pub fn photo_brushes() -> Vec<Brush> {
    let hard = BrushSettings { hardness: 1.0, ..BrushSettings::default() };
    vec![
        credited(
            brush(LIGHTNING_ID, DECOR_CATEGORY, BrushSettings { spacing: 1.0, follow_direction: true, pressure_size: 0.0, ..hard.clone() }, BrushTip::Mask(source_mask(LIGHTNING_SOURCE))),
            LIGHTNING_CREDIT,
        ),
        credited(
            brush(DAISIES_ID, DECOR_CATEGORY, BrushSettings { spacing: 1.2, scatter: 0.4, size_jitter: 0.4, angle_jitter: 0.08, pressure_size: 0.3, ..hard.clone() }, variants(vec![source_mask(DAISY_SOURCE), mirrored(&source_mask(DAISY_SOURCE))])),
            DAISY_CREDIT,
        ),
        credited(
            brush(PEBBLES_ID, DECOR_CATEGORY, BrushSettings { spacing: 0.9, scatter: 0.3, size_jitter: 0.5, angle_jitter: 1.0, pressure_size: 0.3, ..hard }, variants(PEBBLE_SOURCES.iter().map(|name| source_mask(name)).collect())),
            PEBBLE_CREDIT,
        ),
    ]
}

fn source_mask(name: &str) -> MaskTip {
    let path = PathBuf::from(SOURCES_DIR).join(name);
    let gray = image::open(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display())).to_luma8();
    MaskTip::new(gray.width(), gray.height(), gray.into_raw()).expect("source mask is within tip limits")
}

fn mirrored(mask: &MaskTip) -> MaskTip {
    let (width, height) = (mask.width() as usize, mask.height() as usize);
    let pixels = (0..width * height).map(|index| mask.pixels()[index / width * width + (width - 1 - index % width)]).collect();
    MaskTip::new(mask.width(), mask.height(), pixels).expect("mirrored mask keeps its size")
}

fn variants(masks: Vec<MaskTip>) -> BrushTip {
    BrushTip::from_variants(masks.into_iter().map(TipVariant::new).collect())
}

fn credited(mut brush: Brush, source: &str) -> Brush {
    brush.meta.author = source.into();
    brush.meta.license = LICENSE.into();
    brush
}
