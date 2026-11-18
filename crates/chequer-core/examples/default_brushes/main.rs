mod art;
mod classic;
mod crystals;
mod dashes;
mod decor;
mod dynamic;
mod footprints;
mod natural;
mod photos;
mod pens;
mod seams;
mod shapes;
mod texture;
mod tires;

use std::path::PathBuf;

use chequer_core::brush::{Brush, BrushEngine, BrushTip, encode_pack};

const OUTPUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/default_brushes.chqbrush");

const RIBBON_CONVERTED: [&str; 9] = [
    "paint.calligraphy_forward",
    "paint.calligraphy_backward",
    "paint.marker",
    "art.ink_pen",
    "art.dip_pen",
    "cal.broad_nib",
    "cal.brush_pen",
    "cal.taper_out",
    "cal.swell",
];

const NAMES: [(&str, &str, &str); 87] = [
    ("builtin.round", "Круглая", "Round"),
    ("paint.brush", "Кисть", "Brush"),
    ("paint.calligraphy_forward", "Каллиграфия /", "Calligraphy /"),
    ("paint.calligraphy_backward", "Каллиграфия \\", "Calligraphy \\"),
    ("paint.spray", "Спрей", "Spray"),
    ("paint.marker", "Маркер", "Marker"),
    ("builtin.airbrush", "Аэрограф", "Airbrush"),
    ("builtin.square", "Квадратная", "Square"),
    ("builtin.chalk", "Мел", "Chalk"),
    ("builtin.splatter", "Брызги", "Splatter"),
    ("builtin.star", "Звёзды", "Stars"),
    ("art.pencil", "Карандаш HB", "Pencil HB"),
    ("art.pencil_soft", "Карандаш 6B", "Pencil 6B"),
    ("art.charcoal", "Уголь", "Charcoal"),
    ("art.pastel", "Пастель", "Pastel"),
    ("art.ink_pen", "Перо для туши", "Ink pen"),
    ("art.felt_marker", "Фломастер", "Felt marker"),
    ("art.watercolor", "Акварель", "Watercolor"),
    ("art.oil", "Масло (щетина)", "Oil bristle"),
    ("art.flat_bristle", "Плоская кисть", "Flat brush"),
    ("art.dry_brush", "Сухая кисть", "Dry brush"),
    ("art.sponge", "Губка", "Sponge"),
    ("art.ink_blot", "Чернильная клякса", "Ink blot"),
    ("art.dip_pen", "Перо", "Dip pen"),
    ("art.technical_pencil", "Технический карандаш", "Technical pencil"),
    ("art.gouache", "Гуашь", "Gouache"),
    ("art.crayon", "Восковой мелок", "Wax crayon"),
    ("art.stipple", "Пунктир точками", "Stipple"),
    ("art.fur", "Шерсть", "Fur"),
    ("art.sumi", "Суми (тушь кистью)", "Sumi ink"),
    ("nat.rough_ink", "Рваная тушь", "Ragged ink"),
    ("nat.dry_streak", "Сухие полосы", "Dry streaks"),
    ("nat.charcoal_side", "Уголь боком", "Charcoal side"),
    ("nat.pastel_soft", "Сухая пастель", "Soft pastel"),
    ("nat.wash", "Акварельная заливка", "Watercolor wash"),
    ("nat.ink_splash", "Брызги туши", "Ink splash"),
    ("nat.sea_sponge", "Морская губка", "Sea sponge"),
    ("nat.smoke", "Дым", "Smoke"),
    ("nat.grunge", "Гранж", "Grunge"),
    ("nat.grass", "Трава", "Grass"),
    ("nat.cracks", "Трещины", "Cracks"),
    ("cal.leaf_nib", "Перо-лист", "Leaf nib"),
    ("cal.comet", "Комета (росчерк)", "Comet flourish"),
    ("cal.broad_nib", "Плоское перо", "Broad nib"),
    ("cal.brush_pen", "Кисть-ручка", "Brush pen"),
    ("cal.taper_out", "Сужающийся мазок", "Tapering stroke"),
    ("cal.swell", "Нарастающий мазок", "Swelling stroke"),
    ("dyn.bubbles", "Пузыри", "Bubbles"),
    ("dyn.dots", "Горошек", "Polka dots"),
    ("dyn.leaves", "Листья", "Leaves"),
    ("dyn.beads", "Растущие бусины", "Growing beads"),
    ("rib.monoline", "Монолиния", "Monoline"),
    ("rib.inker", "Линер", "Inker"),
    ("rib.soft_taper", "Мягкий конус", "Soft taper"),
    ("rib.flourish", "Росчерк", "Flourish"),
    ("rib.tilt_nib", "Перо по наклону", "Tilt nib"),
    ("rib.chalk_marker", "Меловой маркер", "Chalk marker"),
    ("dry.ink_marker", "Сухой маркер", "Dry ink marker"),
    ("dry.flat_bristle", "Сухая щетина", "Dry bristle"),
    ("cal.split_nib", "Раздвоенное перо", "Split nib"),
    ("trk.tire_winter", "Зимняя шина", "Winter tyre"),
    ("trk.tire_summer", "Летняя шина", "Summer tyre"),
    ("trk.shoe", "След туфли", "Dress shoe print"),
    ("trk.sneaker", "След кеда", "Sneaker print"),
    ("trk.bare_foot", "След босой ноги", "Bare footprint"),
    ("nat.snowflakes", "Снежинки", "Snowflakes"),
    ("nat.frost", "Иней", "Frost"),
    ("dash.dashed", "Пунктир", "Dashed"),
    ("dash.dash_dot", "Пунктир-точка", "Dash-dot"),
    ("dash.dotted", "Точечный пунктир", "Dotted"),
    ("dash.dashed_round", "Пунктир скруглённый", "Dashed rounded"),
    ("dash.dash_dot_round", "Пунктир-точка скруглённый", "Dash-dot rounded"),
    ("dash.dotted_round", "Точечный пунктир скруглённый", "Dotted rounded"),
    ("stitch.running", "Шов", "Running stitch"),
    ("stitch.cross", "Шов крестиком", "Cross stitch"),
    ("line.wave", "Волна", "Wave"),
    ("nat.raindrops", "Капли дождя", "Raindrops"),
    ("nat.starglints", "Звёзды-блики", "Star glints"),
    ("nat.blossoms", "Цветочки", "Blossoms"),
    ("nat.daisies", "Ромашки", "Daisies"),
    ("nat.pebbles", "Камни", "Pebbles"),
    ("nat.lightning", "Молния", "Lightning"),
    ("dec.hearts", "Сердечки", "Hearts"),
    ("dec.confetti", "Конфетти", "Confetti"),
    ("dec.glitter", "Блёстки", "Glitter"),
    ("cmx.burst", "Взрыв", "Comic burst"),
    ("cmx.speed_lines", "Линии скорости", "Speed lines"),
];

fn named(mut brush: Brush) -> Brush {
    let (_, ru, en) = NAMES.iter().find(|(id, _, _)| *id == brush.meta.id).unwrap_or_else(|| panic!("no name for {}", brush.meta.id));
    brush.meta.names = [("ru".to_owned(), (*ru).to_owned()), ("en".to_owned(), (*en).to_owned())].into();
    brush.meta.name = (*en).to_owned();
    brush
}

const PRESSURE_SIZE_KNEE: f32 = 0.3;
const PRESSURE_SIZE_SHARE_ABOVE_KNEE: f32 = 0.3;
const PRESSURE_SIZE_PRECISION: f32 = 100.0;

fn softened_pressure_size(mut brush: Brush) -> Brush {
    let size = brush.settings.pressure_size;
    if size > PRESSURE_SIZE_KNEE {
        let softened = PRESSURE_SIZE_KNEE + (size - PRESSURE_SIZE_KNEE) * PRESSURE_SIZE_SHARE_ABOVE_KNEE;
        brush.settings.pressure_size = (softened * PRESSURE_SIZE_PRECISION).round() / PRESSURE_SIZE_PRECISION;
    }
    brush
}

fn ribbonized(mut brush: Brush) -> Brush {
    if RIBBON_CONVERTED.contains(&brush.meta.id.as_str()) {
        brush.settings.engine = BrushEngine::Ribbon;
        brush.tip = BrushTip::Round;
    }
    brush
}

fn main() {
    let existing = classic::classic_brushes().into_iter().chain(art::art_brushes()).chain(natural::natural_brushes()).chain(dynamic::dynamic_brushes()).map(softened_pressure_size).map(ribbonized);
    let brushes: Vec<Brush> = existing.chain(pens::pen_brushes()).chain(tires::tire_brushes()).chain(footprints::footprint_brushes()).chain(crystals::crystal_brushes()).chain(dashes::dash_brushes()).chain(seams::seam_brushes()).chain(photos::photo_brushes()).chain(decor::decor_brushes()).map(named).collect();
    assert_eq!(brushes.len(), NAMES.len(), "NAMES and generated brushes diverged");
    let bytes = encode_pack(&brushes).expect("encode default brushes");
    std::fs::write(PathBuf::from(OUTPUT), bytes).expect("write default brushes");
}
