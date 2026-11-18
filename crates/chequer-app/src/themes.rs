use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use eframe::egui::Color32;

use crate::formula;
use crate::i18n::{Lang, lang};
use crate::skin::{CornerStyle, Theme};

pub const DEFAULT_LIGHT_ID: &str = "default-light";

const THEME_FILE_EXTENSION: &str = "ini";
const PIXEL_SIZES: RangeInclusive<u32> = 1..=4;
const DEFAULT_PIXEL: u32 = 2;
const MAX_RADIUS: f64 = 64.0;
const MAX_BEVEL_RINGS: f64 = 8.0;
const ROUND_STYLE_VALUE: f64 = 1.0;
const DEFAULT_NUMBER_MAX: f64 = 100.0;
const DEFAULT_NUMBER_STEP: f64 = 1.0;
const TOGGLE_THRESHOLD: f64 = 0.5;
const COMMENT_PREFIXES: [char; 2] = ['#', ';'];
const SETTING_PREFIX: &str = "setting.";
const BIND_PREFIX: &str = "bind.";
const COLOR_PREFIX: &str = "color.";
const ROOT_BASE: &str = "none";
const BUNDLED_THEMES: &[(&str, &str)] = &[
    ("default-light", include_str!("../assets/themes/default-light.ini")),
    ("default-dark", include_str!("../assets/themes/default-dark.ini")),
    ("realistic", include_str!("../assets/themes/realistic.ini")),
    ("realistic-dark", include_str!("../assets/themes/realistic-dark.ini")),
    ("mocha", include_str!("../assets/themes/mocha.ini")),
    ("latte", include_str!("../assets/themes/latte.ini")),
    ("verdigris", include_str!("../assets/themes/verdigris.ini")),
    ("mint", include_str!("../assets/themes/mint.ini")),
    ("prussian", include_str!("../assets/themes/prussian.ini")),
    ("scilla", include_str!("../assets/themes/scilla.ini")),
    ("heliotrope", include_str!("../assets/themes/heliotrope.ini")),
    ("thistle", include_str!("../assets/themes/thistle.ini")),
    ("claret", include_str!("../assets/themes/claret.ini")),
    ("camellia", include_str!("../assets/themes/camellia.ini")),
    ("gamboge", include_str!("../assets/themes/gamboge.ini")),
    ("primrose", include_str!("../assets/themes/primrose.ini")),
    ("nord-night", include_str!("../assets/themes/nord-night.ini")),
    ("nord-day", include_str!("../assets/themes/nord-day.ini")),
    ("rose-pine", include_str!("../assets/themes/rose-pine.ini")),
    ("rose-pine-dawn", include_str!("../assets/themes/rose-pine-dawn.ini")),
    ("gruvbox-dark", include_str!("../assets/themes/gruvbox-dark.ini")),
    ("gruvbox-light", include_str!("../assets/themes/gruvbox-light.ini")),
    ("tokyo-night", include_str!("../assets/themes/tokyo-night.ini")),
    ("tokyo-day", include_str!("../assets/themes/tokyo-day.ini")),
    ("outrun", include_str!("../assets/themes/outrun.ini")),
    ("outrun-daylight", include_str!("../assets/themes/outrun-daylight.ini")),
    ("phosphor", include_str!("../assets/themes/phosphor.ini")),
    ("paper-terminal", include_str!("../assets/themes/paper-terminal.ini")),
    ("blueprint", include_str!("../assets/themes/blueprint.ini")),
    ("vellum", include_str!("../assets/themes/vellum.ini")),
    ("sumi", include_str!("../assets/themes/sumi.ini")),
    ("washi", include_str!("../assets/themes/washi.ini")),
    ("high-contrast", include_str!("../assets/themes/high-contrast.ini")),
    ("oled", include_str!("../assets/themes/oled.ini")),
    ("oled-ice", include_str!("../assets/themes/oled-ice.ini")),
    ("oled-ember", include_str!("../assets/themes/oled-ember.ini")),
];
const BUNDLED_ICONS_DIR: &str = "";

pub type SettingValues = BTreeMap<String, f64>;
pub type ColorOverrides = BTreeMap<String, Color32>;

pub fn scoped_key(theme_id: &str, name: &str) -> String {
    format!("{theme_id}.{name}")
}

#[derive(Clone, Debug)]
pub struct Label {
    ru: String,
    en: String,
}

impl Label {
    fn both(text: &str) -> Self {
        Self { ru: text.to_owned(), en: text.to_owned() }
    }

    pub fn get(&self) -> &str {
        if lang() == Lang::Ru { &self.ru } else { &self.en }
    }
}

#[derive(Clone, Debug)]
pub enum SettingKind {
    Toggle,
    Number { min: f64, max: f64, step: f64 },
    Choice(Vec<(Label, f64)>),
}

#[derive(Clone, Debug)]
pub struct SettingSpec {
    pub id: String,
    pub label: Label,
    pub kind: SettingKind,
    pub default: f64,
    pub drives: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    WindowRadius,
    ButtonRadius,
    ShadowAlpha,
    BevelRings,
    CornerStyle,
}

impl Target {
    fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "window_radius" => Self::WindowRadius,
            "button_radius" => Self::ButtonRadius,
            "shadow_alpha" => Self::ShadowAlpha,
            "bevel_rings" => Self::BevelRings,
            "corner_style" => Self::CornerStyle,
            _ => return None,
        })
    }

    fn assign(self, theme: &mut Theme, value: f64) {
        let whole = value.round().max(0.0);
        match self {
            Self::WindowRadius => theme.window_radius = whole.min(MAX_RADIUS) as u32,
            Self::ButtonRadius => theme.button_radius = whole.min(MAX_RADIUS) as u32,
            Self::ShadowAlpha => theme.shadow_alpha = whole.min(u8::MAX as f64) as u8,
            Self::BevelRings => theme.bevel_rings = whole.min(MAX_BEVEL_RINGS) as usize,
            Self::CornerStyle => theme.corner_style = if whole >= ROUND_STYLE_VALUE { CornerStyle::Round } else { CornerStyle::Bevel },
        }
    }
}

#[derive(Clone, Debug)]
struct Binding {
    target: Target,
    formula: String,
}

fn evaluate_bound(formula_text: &str, variables: &BTreeMap<&str, f64>) -> Option<f64> {
    let mut expression = String::new();
    let mut chars = formula_text.chars().peekable();
    while let Some(c) = chars.next() {
        if !(c.is_alphabetic() || c == '_') {
            expression.push(c);
            continue;
        }
        let mut name = String::from(c);
        while let Some(next) = chars.next_if(|n| n.is_alphanumeric() || *n == '_') {
            name.push(next);
        }
        expression.push_str(&format!("({})", variables.get(name.as_str())?));
    }
    formula::evaluate(&expression)
}

#[derive(Clone, Debug)]
pub struct ThemeSpec {
    pub id: String,
    pub name: Label,
    pub pixel: u32,
    pub icons_dir: Option<PathBuf>,
    pub settings: Vec<SettingSpec>,
    base: Theme,
    bindings: Vec<Binding>,
}

impl ThemeSpec {
    fn blank() -> Self {
        Self { id: String::new(), name: Label::both(""), pixel: DEFAULT_PIXEL, icons_dir: None, settings: Vec::new(), base: Theme::default(), bindings: Vec::new() }
    }

    pub fn value(&self, setting: &SettingSpec, values: &SettingValues) -> f64 {
        let stored = values.get(&scoped_key(&self.id, &setting.id)).copied().unwrap_or(setting.default);
        match &setting.kind {
            SettingKind::Toggle => f64::from(stored > TOGGLE_THRESHOLD),
            SettingKind::Number { min, max, .. } => stored.clamp(*min, *max),
            SettingKind::Choice(_) => stored,
        }
    }

    pub fn resolve(&self, values: &SettingValues, colors: &ColorOverrides) -> Theme {
        let variables: BTreeMap<&str, f64> = self.settings.iter().map(|setting| (setting.id.as_str(), self.value(setting, values))).collect();
        let mut theme = self.base;
        for binding in &self.bindings {
            if let Some(number) = evaluate_bound(&binding.formula, &variables) {
                binding.target.assign(&mut theme, number);
            }
        }
        for (key, _, _, slot) in theme.colors_mut() {
            if let Some(color) = colors.get(&scoped_key(&self.id, key)) {
                *slot = *color;
            }
        }
        theme
    }
}

pub struct ThemeLibrary {
    pub specs: Vec<ThemeSpec>,
    pub problems: Vec<String>,
}

fn parse_bundled() -> (Vec<ThemeSpec>, Vec<String>) {
    let mut specs = Vec::new();
    let mut problems = Vec::new();
    for (id, text) in BUNDLED_THEMES {
        match parse_custom(id, text, Path::new(BUNDLED_ICONS_DIR), &specs) {
            Ok(spec) => specs.push(spec),
            Err(error) => problems.push(format!("{id}: {error}")),
        }
    }
    (specs, problems)
}

impl ThemeLibrary {
    pub fn load() -> Self {
        let (mut specs, mut problems) = parse_bundled();
        assert!(!specs.is_empty(), "no bundled theme parsed: {problems:?}");
        let Some(directory) = crate::config::themes_dir() else {
            return Self { specs, problems };
        };
        let mut files: Vec<PathBuf> = std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == THEME_FILE_EXTENSION))
            .collect();
        files.sort();
        for path in files {
            let Some(id) = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()) else {
                continue;
            };
            let file_name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            if specs.iter().any(|spec| spec.id == id) {
                problems.push(format!("{file_name}: id already taken"));
                continue;
            }
            let parsed = std::fs::read_to_string(&path).map_err(|error| error.to_string()).and_then(|text| parse_custom(&id, &text, &directory, &specs));
            match parsed {
                Ok(spec) => specs.push(spec),
                Err(error) => problems.push(format!("{file_name}: {error}")),
            }
        }
        Self { specs, problems }
    }

    pub fn find(&self, id: &str) -> &ThemeSpec {
        self.specs.iter().find(|spec| spec.id == id).unwrap_or(&self.specs[0])
    }
}

fn parse_number(key: &str, value: &str) -> Result<f64, String> {
    value.parse::<f64>().ok().filter(|number| number.is_finite()).ok_or_else(|| format!("{key}: not a number"))
}

fn parse_color(key: &str, value: &str) -> Result<Color32, String> {
    let digits = value.strip_prefix('#').filter(|digits| digits.len() == 6);
    let number = digits.and_then(|digits| u32::from_str_radix(digits, 16).ok()).ok_or_else(|| format!("{key}: expected #rrggbb"))?;
    Ok(Color32::from_rgb((number >> 16) as u8, (number >> 8) as u8, number as u8))
}

fn parse_custom(id: &str, text: &str, directory: &Path, loaded: &[ThemeSpec]) -> Result<ThemeSpec, String> {
    let entries: Vec<(&str, &str)> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(COMMENT_PREFIXES))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim(), value.trim()))
        .collect();
    let base_id = entries.iter().find(|(key, _)| *key == "base").map_or(DEFAULT_LIGHT_ID, |(_, value)| *value);
    let is_root = base_id == ROOT_BASE;
    let mut spec = if is_root { ThemeSpec::blank() } else { loaded.iter().find(|spec| spec.id == base_id).cloned().ok_or_else(|| format!("base: unknown theme {base_id}"))? };
    spec.id = id.to_owned();
    spec.name = Label::both(id);
    let mut declared: Vec<(String, BTreeMap<String, String>)> = Vec::new();
    let mut defined_colors: Vec<&str> = Vec::new();
    for (key, value) in entries {
        if let Some(target) = Target::from_key(key) {
            target.assign(&mut spec.base, parse_number(key, value)?);
            spec.bindings.retain(|binding| binding.target != target);
            continue;
        }
        match key {
            "base" => {}
            "name" => spec.name = Label::both(value),
            "name.ru" => spec.name.ru = value.to_owned(),
            "pixel" => spec.pixel = value.parse().ok().filter(|pixel| PIXEL_SIZES.contains(pixel)).ok_or_else(|| "pixel: expected 1..4".to_owned())?,
            "icons" => spec.icons_dir = Some(directory.join(value)),
            _ if key.starts_with(COLOR_PREFIX) => {
                let field = &key[COLOR_PREFIX.len()..];
                let color = parse_color(key, value)?;
                let slot = spec.base.colors_mut().into_iter().find(|(name, ..)| *name == field).map(|(.., slot)| slot).ok_or_else(|| format!("{key}: unknown color"))?;
                *slot = color;
                defined_colors.push(field);
            }
            _ if key.starts_with(BIND_PREFIX) => {
                let target = Target::from_key(&key[BIND_PREFIX.len()..]).ok_or_else(|| format!("{key}: unknown target"))?;
                spec.bindings.retain(|binding| binding.target != target);
                spec.bindings.push(Binding { target, formula: value.to_owned() });
            }
            _ if key.starts_with(SETTING_PREFIX) => {
                let (setting_id, attribute) = key[SETTING_PREFIX.len()..].split_once('.').ok_or_else(|| format!("{key}: expected setting.<id>.<attribute>"))?;
                let position = declared.iter().position(|(name, _)| name == setting_id).unwrap_or_else(|| {
                    declared.push((setting_id.to_owned(), BTreeMap::new()));
                    declared.len() - 1
                });
                declared[position].1.insert(attribute.to_owned(), value.to_owned());
            }
            _ => return Err(format!("{key}: unknown key")),
        }
    }
    for (setting_id, attributes) in declared {
        if let Some(existing) = spec.settings.iter_mut().find(|existing| existing.id == setting_id).filter(|_| !attributes.contains_key("type")) {
            let default = attributes.get("default").ok_or_else(|| format!("setting.{setting_id}: only default can be overridden"))?;
            existing.default = parse_number(&format!("setting.{setting_id}.default"), default)?;
            continue;
        }
        let setting = parse_setting(&setting_id, &attributes)?;
        spec.settings.retain(|existing| existing.id != setting.id);
        spec.settings.push(setting);
    }
    if is_root {
        require_all_colors(&mut spec.base, &defined_colors)?;
    }
    Ok(spec)
}

fn require_all_colors(theme: &mut Theme, defined: &[&str]) -> Result<(), String> {
    match theme.colors_mut().into_iter().find(|(key, ..)| !defined.contains(key)) {
        Some((key, ..)) => Err(format!("color.{key}: required when base={ROOT_BASE}")),
        None => Ok(()),
    }
}

fn parse_setting(id: &str, attributes: &BTreeMap<String, String>) -> Result<SettingSpec, String> {
    let number = |attribute: &str, fallback: f64| attributes.get(attribute).map_or(Ok(fallback), |value| parse_number(&format!("setting.{id}.{attribute}"), value));
    let kind = match attributes.get("type").map(String::as_str) {
        Some("toggle") => SettingKind::Toggle,
        Some("number") => SettingKind::Number { min: number("min", 0.0)?, max: number("max", DEFAULT_NUMBER_MAX)?, step: number("step", DEFAULT_NUMBER_STEP)? },
        Some("choice") => SettingKind::Choice(parse_options(id, attributes.get("options").map_or("", String::as_str))?),
        _ => return Err(format!("setting.{id}.type: expected toggle, number or choice")),
    };
    let mut label = Label::both(attributes.get("label").map_or(id, String::as_str));
    if let Some(ru) = attributes.get("label.ru") {
        label.ru = ru.clone();
    }
    let default = number("default", 0.0)?;
    let drives = attributes.get("drives").map(|list| list.split(',').map(|name| name.trim().to_owned()).filter(|name| !name.is_empty()).collect()).unwrap_or_default();
    Ok(SettingSpec { id: id.to_owned(), label, kind, default, drives })
}

fn parse_options(id: &str, text: &str) -> Result<Vec<(Label, f64)>, String> {
    let options: Vec<(Label, f64)> = text
        .split(',')
        .filter_map(|item| item.rsplit_once(':'))
        .map(|(name, value)| parse_number(&format!("setting.{id}.options"), value.trim()).map(|number| (Label::both(name.trim()), number)))
        .collect::<Result<_, _>>()?;
    if options.is_empty() {
        return Err(format!("setting.{id}.options: expected name:value,name:value"));
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_theme_parses() {
        let (_, problems) = parse_bundled();
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn root_theme_without_every_color_is_rejected() {
        let error = parse_custom("x", "base=none\ncolor.ink=#000000\n", Path::new(""), &[]).unwrap_err();
        assert!(error.contains("required when base=none"), "{error}");
    }
}
