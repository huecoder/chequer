
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock, RwLockReadGuard};

use chequer_core::text::FontFace;

pub const FIRST_FACE: u32 = 0;
pub const FONT_FILE_EXTENSIONS: [&str; 4] = ["ttf", "otf", "ttc", "otc"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FontChoice {
    #[default]
    Builtin,
    System(String),
    File(PathBuf),
}

pub struct LoadedFont {
    pub bytes: Vec<u8>,
    pub index: u32,
}

impl FontChoice {
    pub fn to_setting(&self) -> String {
        match self {
            FontChoice::Builtin => "default".to_owned(),
            FontChoice::System(family) => format!("system:{family}"),
            FontChoice::File(path) => format!("file:{}", path.display()),
        }
    }

    pub fn from_setting(value: &str) -> Self {
        match value.split_once(':') {
            Some(("system", family)) if !family.is_empty() => FontChoice::System(family.to_owned()),
            Some(("file", path)) if !path.is_empty() => FontChoice::File(PathBuf::from(path)),
            _ => FontChoice::Builtin,
        }
    }

    pub fn load(&self) -> Result<Option<LoadedFont>, String> {
        match self {
            FontChoice::Builtin => Ok(None),
            FontChoice::System(family) => load_system(family).map(Some),
            FontChoice::File(path) => load_file(path).map(Some),
        }
    }
}

fn checked(bytes: Vec<u8>, index: u32, what: &str) -> Result<LoadedFont, String> {
    ttf_parser::Face::parse(&bytes, index).map_err(|e| format!("{what}: {e}"))?;
    Ok(LoadedFont { bytes, index })
}

fn load_file(path: &Path) -> Result<LoadedFont, String> {
    let what = path.display().to_string();
    let bytes = std::fs::read(path).map_err(|e| format!("{what}: {e}"))?;
    checked(bytes, FIRST_FACE, &what)
}

fn system_database() -> &'static RwLock<fontdb::Database> {
    static DATABASE: OnceLock<RwLock<fontdb::Database>> = OnceLock::new();
    DATABASE.get_or_init(|| {
        let mut database = fontdb::Database::new();
        database.load_system_fonts();
        #[cfg(target_os = "android")]
        database.load_fonts_dir("/system/fonts");
        set_sans_serif_family(&mut database);
        RwLock::new(database)
    })
}

fn readable_database() -> RwLockReadGuard<'static, fontdb::Database> {
    system_database().read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn register_font_file(path: &Path) -> Result<Vec<String>, String> {
    let what = path.display().to_string();
    let bytes = std::fs::read(path).map_err(|e| format!("{what}: {e}"))?;
    ttf_parser::Face::parse(&bytes, FIRST_FACE).map_err(|e| format!("{what}: {e}"))?;
    let mut database = system_database().write().unwrap_or_else(|poisoned| poisoned.into_inner());
    let ids = database.load_font_source(fontdb::Source::Binary(Arc::new(bytes)));
    let mut families: Vec<String> = ids.iter().filter_map(|id| database.face(*id)).filter_map(|face| face.families.first().map(|(name, _)| name.clone())).collect();
    families.dedup();
    if families.is_empty() {
        return Err(format!("{what}: в файле нет гарнитур"));
    }
    Ok(families)
}

pub fn register_user_fonts(paths: &[PathBuf]) {
    for path in paths {
        let _ = register_font_file(path);
    }
}

const SANS_SERIF_CANDIDATES: [&str; 8] = ["Segoe UI", "SF Pro Text", "Roboto", "Noto Sans", "DejaVu Sans", "Liberation Sans", "Arial", "Helvetica"];
const LATIN_PROBE: char = 'a';

fn draws_latin(database: &fontdb::Database, id: fontdb::ID) -> bool {
    database
        .with_face_data(id, |data, index| {
            ttf_parser::Face::parse(data, index).is_ok_and(|face| face.glyph_index(LATIN_PROBE).is_some_and(|glyph| face.outline_glyph(glyph, &mut OutlineProbe).is_some()))
        })
        .unwrap_or(false)
}

struct OutlineProbe;

impl ttf_parser::OutlineBuilder for OutlineProbe {
    fn move_to(&mut self, _: f32, _: f32) {}
    fn line_to(&mut self, _: f32, _: f32) {}
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
    fn close(&mut self) {}
}

fn set_sans_serif_family(database: &mut fontdb::Database) {
    let installed = |name: &str| database.faces().any(|face| face.families.iter().any(|(family, _)| family == name));
    let any_family = || database.faces().filter(|face| draws_latin(database, face.id)).find_map(|face| face.families.first().map(|(name, _)| name.clone()));
    let chosen = SANS_SERIF_CANDIDATES.iter().find(|name| installed(name)).map(|name| (*name).to_owned()).or_else(any_family);
    if let Some(name) = chosen {
        database.set_sans_serif_family(name);
    }
}

fn load_system(family: &str) -> Result<LoadedFont, String> {
    let database = readable_database();
    let query = fontdb::Query { families: &[fontdb::Family::Name(family)], ..fontdb::Query::default() };
    let id = database.query(&query).ok_or_else(|| format!("{family}: шрифт не найден"))?;
    let (bytes, index) = database.with_face_data(id, |data, index| (data.to_vec(), index)).ok_or_else(|| format!("{family}: файл не читается"))?;
    checked(bytes, index, family)
}

#[derive(Clone)]
pub struct TextFace {
    pub face: FontFace,
    pub synthetic_bold: bool,
    pub synthetic_italic: bool,
}

const BOLD_WEIGHT: u16 = 600;

pub fn text_face(family: Option<&str>, bold: bool, italic: bool) -> Result<TextFace, String> {
    let database = readable_database();
    let families = [family.map_or(fontdb::Family::SansSerif, fontdb::Family::Name)];
    let query = fontdb::Query {
        families: &families,
        weight: if bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL },
        style: if italic { fontdb::Style::Italic } else { fontdb::Style::Normal },
        ..fontdb::Query::default()
    };
    let what = family.unwrap_or("sans-serif");
    let id = database.query(&query).ok_or_else(|| format!("{what}: шрифт не найден"))?;
    let info = database.face(id).ok_or_else(|| format!("{what}: шрифт не найден"))?;
    let (weight, style) = (info.weight.0, info.style);
    let (data, index) = database.with_face_data(id, |data, index| (Arc::<[u8]>::from(data), index)).ok_or_else(|| format!("{what}: файл не читается"))?;
    let face = FontFace::new(data, index).map_err(|e| format!("{what}: {e}"))?;
    Ok(TextFace { face, synthetic_bold: bold && weight < BOLD_WEIGHT, synthetic_italic: italic && style == fontdb::Style::Normal })
}

pub fn system_families() -> Vec<String> {
    let database = readable_database();
    let mut names: Vec<String> = database.faces().filter_map(|face| face.families.first().map(|(name, _)| name.clone())).collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setting_round_trips_every_kind_of_choice() {
        for choice in [FontChoice::Builtin, FontChoice::System("Noto Sans".into()), FontChoice::File("/tmp/my font.ttf".into())] {
            assert_eq!(FontChoice::from_setting(&choice.to_setting()), choice);
        }
    }

    #[test]
    fn garbage_setting_falls_back_to_the_builtin_font() {
        assert_eq!(FontChoice::from_setting("system:"), FontChoice::Builtin);
        assert_eq!(FontChoice::from_setting("nonsense"), FontChoice::Builtin);
    }

    #[test]
    fn broken_font_file_is_an_error_not_a_crash() {
        assert!(checked(vec![1, 2, 3], FIRST_FACE, "test").is_err());
    }
}
