
use std::path::PathBuf;

use eframe::egui::{Color32, vec2};

use crate::app::{PanelId, Panels, Settings};
use crate::fonts::FontChoice;
use crate::i18n::Lang;
use crate::new_document::NewBackground;
use crate::palette::Palette;
use crate::panels::{ColorSelector, LayerDoubleClick, SwatchSize};
use crate::timeline::TimelineCells;
use crate::trackpad::{PINCH_SENSITIVITY_MAX, PINCH_SENSITIVITY_MIN, TrackpadMode};
use crate::tools::TextUnit;
use crate::view::ZoomOutFilter;
use crate::widgets::Corner;

const CONFIG_DIR: &str = "chequer";
const CONFIG_FILE: &str = "settings.ini";
const AUTO: &str = "auto";
const THEME_VALUE_PREFIX: &str = "theme_value.";
const THEME_COLOR_PREFIX: &str = "theme_color.";
const THEMES_DIR: &str = "themes";
const RECOVERY_DIR: &str = "recovery";
const LAYOUT_FILE: &str = "layout.ini";
const RECENT_FILE: &str = "recent.txt";
const PALETTE_FILE: &str = "palette.gpl";
const BRUSHES_DIR: &str = "brushes";
const ACTIVE_BRUSHES_FILE: &str = "active-brushes.chqbrush";
const BRUSH_OVERRIDES_FILE: &str = "brush-overrides.chqbrush";
const FAVORITE_BRUSHES_FILE: &str = "favorite-brushes.txt";
const SEEDED_BRUSHES_FILE: &str = "seeded-brushes.txt";

fn config_path() -> Option<PathBuf> {
    config_file(CONFIG_FILE)
}

#[cfg(target_os = "android")]
static ANDROID_DATA_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

#[cfg(target_os = "android")]
pub fn set_android_data_dir(dir: PathBuf) {
    let _ = ANDROID_DATA_DIR.set(dir);
}

#[cfg(target_os = "android")]
fn config_file(name: &str) -> Option<PathBuf> {
    ANDROID_DATA_DIR.get().map(|dir| dir.join(CONFIG_DIR).join(name))
}

#[cfg(not(target_os = "android"))]
fn config_file(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join(CONFIG_DIR).join(name))
}

pub fn brushes_dir() -> Option<PathBuf> {
    config_file(BRUSHES_DIR)
}

pub fn active_brushes_path() -> Option<PathBuf> {
    config_file(ACTIVE_BRUSHES_FILE)
}

pub fn brush_overrides_path() -> Option<PathBuf> {
    config_file(BRUSH_OVERRIDES_FILE)
}

pub fn recovery_dir() -> Option<PathBuf> {
    config_file(RECOVERY_DIR)
}

pub fn themes_dir() -> Option<PathBuf> {
    config_file(THEMES_DIR)
}

pub fn load(settings: &mut Settings) {
    if let Some(text) = config_path().and_then(|path| std::fs::read_to_string(path).ok()) {
        apply_text(settings, &text);
    }
}

pub fn load_lang() -> Lang {
    let text = config_path().and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
    let configured = text.lines().filter_map(|line| line.split_once('=')).find(|(key, _)| key.trim() == "lang");
    match configured {
        Some((_, value)) if value.trim() == "en" => Lang::En,
        _ => Lang::Ru,
    }
}

pub fn apply_text(settings: &mut Settings, text: &str) -> usize {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, value)| apply(settings, key.trim(), value.trim()))
        .count()
}

fn apply(settings: &mut Settings, key: &str, value: &str) -> bool {
    match key {
        "lang" => settings.lang = if value == "en" { Lang::En } else { Lang::Ru },
        "ui_scale" => settings.ui_scale = value.parse().ok().filter(|_| value != AUTO),
        "theme" => settings.theme_id = value.to_owned(),
        "font" => settings.font = FontChoice::from_setting(value),
        "user_font" => {
            let path = PathBuf::from(value);
            if !value.is_empty() && !settings.user_fonts.contains(&path) {
                settings.user_fonts.push(path);
            }
        }
        "trackpad" => settings.trackpad.mode = TrackpadMode::from_setting(value),
        "pinch_sensitivity" => {
            settings.trackpad.pinch_sensitivity = value.parse().map_or(settings.trackpad.pinch_sensitivity, |s: f32| s.clamp(PINCH_SENSITIVITY_MIN, PINCH_SENSITIVITY_MAX))
        }
        "gesture_rotate" => settings.trackpad.rotate = value == "true",
        "gesture_swipes" => settings.trackpad.swipes = value == "true",
        "pen_wayland" => settings.pen_wayland = value == "true",
        "haptics" => settings.haptics = value == "true",
        "pen_only" => settings.pen_only = value == "true",
        "volume_undo" => settings.volume_undo = value == "true",
        "pen_curve" => settings.pen_response.curve = parse_or(value, settings.pen_response.curve),
        "pen_dead_zone" => settings.pen_response.dead_zone = parse_or(value, settings.pen_response.dead_zone),
        "pen_smoothing" => settings.pen_response.smoothing = parse_or(value, settings.pen_response.smoothing),
        "pen_min_pressure" => settings.pen_response.min_pressure = parse_or(value, settings.pen_response.min_pressure),
        "picker_selector" => settings.picker.selector = ColorSelector::from_setting(value),
        "picker_rgb" => settings.picker.rgb = value == "true",
        "picker_hsv" => settings.picker.hsv = value == "true",
        "picker_alpha" => settings.picker.alpha = value == "true",
        "picker_hex" => settings.picker.hex = value == "true",
        "picker_beside" => settings.picker.beside = value == "true",
        "picker_swatch" => settings.picker.swatch = SwatchSize::from_setting(value),
        "zoom_out_filter" => settings.zoom_out_filter = ZoomOutFilter::from_setting(value),
        "checker_light" => settings.checker_light = parse_hex(value),
        "checker_dark" => settings.checker_dark = parse_hex(value),
        "text_unit" => settings.text_unit = TextUnit::from_setting(value),
        "compact_colors" => settings.compact_colors = value == "true",
        "high_contrast_icons" => settings.high_contrast_icons = value == "true",
        "palette_below" => settings.palette_below = value == "true",
        "layer_quick_settings" => settings.layer_quick_settings = value == "true",
        "layer_double_click" => settings.layer_double_click = LayerDoubleClick::from_setting(value),
        "timeline_cells" => settings.timeline_cells = TimelineCells::from_setting(value),
        "onion_before" => settings.animation.onion.before = value.parse().unwrap_or(settings.animation.onion.before),
        "onion_after" => settings.animation.onion.after = value.parse().unwrap_or(settings.animation.onion.after),
        "onion_opacity" => settings.animation.onion.opacity = value.parse().unwrap_or(settings.animation.onion.opacity),
        "onion_before_color" => settings.animation.onion.before_color = parse_hex(value).map_or(settings.animation.onion.before_color, rgb_of),
        "onion_after_color" => settings.animation.onion.after_color = parse_hex(value).map_or(settings.animation.onion.after_color, rgb_of),
        "space_plays" => settings.animation.keys.space_plays = value == "true",
        "arrows_navigate" => settings.animation.keys.arrows_navigate = value == "true",
        "wheel_navigates" => settings.animation.keys.wheel_navigates = value == "true",
        "new_background" => settings.new_document.background = NewBackground::from_setting(value),
        "new_background_color" => {
            if let Some(color) = parse_hex(value) {
                settings.new_document.color = color.to_srgba_unmultiplied();
            }
        }
        "grain_enabled" => settings.grain_enabled = value == "true",
        "grain" => settings.grain = value.parse().ok().map_or(settings.grain, |grain: f32| grain.clamp(0.0, 1.0)),
        _ => {
            if let Some(name) = key.strip_prefix(THEME_VALUE_PREFIX) {
                let Ok(number) = value.parse::<f64>() else {
                    return false;
                };
                settings.theme_values.insert(name.to_owned(), number);
                return true;
            }
            let (Some(name), Some(color)) = (key.strip_prefix(THEME_COLOR_PREFIX), parse_hex(value)) else {
                return false;
            };
            settings.theme_colors.insert(name.to_owned(), color);
        }
    }
    true
}

fn parse_or(value: &str, fallback: f32) -> f32 {
    value.parse().unwrap_or(fallback)
}

fn rgb_of(color: Color32) -> [u8; 3] {
    [color.r(), color.g(), color.b()]
}

fn parse_hex(value: &str) -> Option<Color32> {
    let digits = value.strip_prefix('#')?;
    let number = u32::from_str_radix(digits, 16).ok().filter(|_| digits.len() == 6)?;
    Some(Color32::from_rgb((number >> 16) as u8, (number >> 8) as u8, number as u8))
}

pub fn to_text(settings: &Settings) -> String {
    let mut text = format!(
        "lang={}\nui_scale={}\nfont={}\ngrain={}\ngrain_enabled={}\ntheme={}\ntrackpad={}\npinch_sensitivity={}\ngesture_rotate={}\ngesture_swipes={}\npen_wayland={}\nhaptics={}\npen_only={}\nvolume_undo={}\npen_curve={}\npen_dead_zone={}\npen_smoothing={}\npen_min_pressure={}\npicker_selector={}\npicker_rgb={}\npicker_hsv={}\npicker_alpha={}\npicker_hex={}\npicker_swatch={}\n",
        if settings.lang == Lang::En {
            "en"
        } else {
            "ru"
        },
        settings
            .ui_scale
            .map_or_else(|| AUTO.to_owned(), |scale| scale.to_string()),
        settings.font.to_setting(),
        settings.grain,
        settings.grain_enabled,
        settings.theme_id,
        settings.trackpad.mode.to_setting(),
        settings.trackpad.pinch_sensitivity,
        settings.trackpad.rotate,
        settings.trackpad.swipes,
        settings.pen_wayland,
        settings.haptics,
        settings.pen_only,
        settings.volume_undo,
        settings.pen_response.curve,
        settings.pen_response.dead_zone,
        settings.pen_response.smoothing,
        settings.pen_response.min_pressure,
        settings.picker.selector.to_setting(),
        settings.picker.rgb,
        settings.picker.hsv,
        settings.picker.alpha,
        settings.picker.hex,
        settings.picker.swatch.to_setting(),
    );
    text.push_str(&format!("layer_quick_settings={}\nlayer_double_click={}\ntimeline_cells={}\n", settings.layer_quick_settings, settings.layer_double_click.to_setting(), settings.timeline_cells.to_setting()));
    let onion = settings.animation.onion;
    let keys = settings.animation.keys;
    text.push_str(&format!(
        "onion_before={}\nonion_after={}\nonion_opacity={}\nonion_before_color=#{:02x}{:02x}{:02x}\nonion_after_color=#{:02x}{:02x}{:02x}\nspace_plays={}\narrows_navigate={}\nwheel_navigates={}\n",
        onion.before, onion.after, onion.opacity, onion.before_color[0], onion.before_color[1], onion.before_color[2], onion.after_color[0], onion.after_color[1], onion.after_color[2], keys.space_plays, keys.arrows_navigate, keys.wheel_navigates
    ));
    let [red, green, blue, _] = settings.new_document.color;
    text.push_str(&format!("high_contrast_icons={}\nnew_background={}\nnew_background_color=#{red:02x}{green:02x}{blue:02x}\n", settings.high_contrast_icons, settings.new_document.background.to_setting()));
    text.push_str(&format!("compact_colors={}\npalette_below={}\nzoom_out_filter={}\npicker_beside={}\ntext_unit={}\n", settings.compact_colors, settings.palette_below, settings.zoom_out_filter.to_setting(), settings.picker.beside, settings.text_unit.to_setting()));
    for (key, color) in [("checker_light", settings.checker_light), ("checker_dark", settings.checker_dark)] {
        if let Some(color) = color {
            text.push_str(&format!("{key}=#{:02x}{:02x}{:02x}\n", color.r(), color.g(), color.b()));
        }
    }
    for path in &settings.user_fonts {
        text.push_str(&format!("user_font={}\n", path.display()));
    }
    for (name, number) in &settings.theme_values {
        text.push_str(&format!("{THEME_VALUE_PREFIX}{name}={number}\n"));
    }
    for (name, color) in &settings.theme_colors {
        text.push_str(&format!("{THEME_COLOR_PREFIX}{name}=#{:02x}{:02x}{:02x}\n", color.r(), color.g(), color.b()));
    }
    text
}

pub fn save(settings: &Settings) {
    let Some(path) = config_path() else {
        return;
    };
    let text = to_text(settings);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, text);
}

fn panel_key(id: PanelId) -> &'static str {
    match id {
        PanelId::Tools => "tools",
        PanelId::History => "history",
        PanelId::Layers => "layers",
        PanelId::Colors => "colors",
        PanelId::Swatches => "swatches",
        PanelId::Timeline => "timeline",
    }
}

pub fn save_panels(panels: &Panels) {
    save_panels_text(&panels_text(panels));
}

pub fn save_panels_text(text: &str) {
    let Some(path) = config_file(LAYOUT_FILE) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, text);
}

pub fn panels_text(panels: &Panels) -> String {
    PanelId::ALL
        .into_iter()
        .filter_map(|id| panels.get(id).map(|state| (id, state)))
        .map(|(id, state)| {
            let (corner, gap, size) = (state.corner, state.gap, state.size);
            format!("{}={} {} {} {} {} {} {}\n", panel_key(id), state.open, corner.right, corner.bottom, gap.x, gap.y, size.x, size.y)
        })
        .collect()
}

fn apply_panel_line(panels: &mut Panels, line: &str) -> bool {
    let Some((key, value)) = line.split_once('=') else {
        return false;
    };
    let Some(id) = PanelId::ALL.into_iter().find(|id| panel_key(*id) == key.trim()) else {
        return false;
    };
    let fields: Vec<&str> = value.split_whitespace().collect();
    let [open, right, bottom, gap_x, gap_y, width, height] = fields[..] else {
        return false;
    };
    let (Ok(open), Ok(right), Ok(bottom)) = (open.parse(), right.parse(), bottom.parse()) else {
        return false;
    };
    let (Ok(gap_x), Ok(gap_y), Ok(width), Ok(height)) = (gap_x.parse::<f32>(), gap_y.parse::<f32>(), width.parse::<f32>(), height.parse::<f32>()) else {
        return false;
    };
    let state = panels.get_mut(id);
    state.open = open;
    state.corner = Corner { right, bottom };
    state.gap = vec2(gap_x, gap_y);
    state.size = vec2(width, height);
    true
}

pub fn load_panels(panels: &mut Panels) -> bool {
    let Some(text) = config_file(LAYOUT_FILE).and_then(|path| std::fs::read_to_string(path).ok()) else {
        return false;
    };
    text.lines().fold(false, |applied, line| apply_panel_line(panels, line) || applied)
}

pub fn load_recent() -> Vec<PathBuf> {
    let Some(text) = config_file(RECENT_FILE).and_then(|path| std::fs::read_to_string(path).ok()) else {
        return Vec::new();
    };
    text.lines().filter(|line| !line.is_empty()).map(PathBuf::from).filter(|path| path.exists()).collect()
}

pub fn save_recent(recent: &[PathBuf]) {
    let Some(path) = config_file(RECENT_FILE) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text: String = recent.iter().map(|path| format!("{}\n", path.display())).collect();
    let _ = std::fs::write(path, text);
}

pub fn load_favorite_brushes() -> Vec<String> {
    load_lines(FAVORITE_BRUSHES_FILE)
}

pub fn save_favorite_brushes(keys: &[String]) {
    save_lines(FAVORITE_BRUSHES_FILE, keys);
}

pub fn load_seeded_brushes() -> Vec<String> {
    load_lines(SEEDED_BRUSHES_FILE)
}

pub fn save_seeded_brushes(ids: &[String]) {
    save_lines(SEEDED_BRUSHES_FILE, ids);
}

fn load_lines(name: &str) -> Vec<String> {
    let Some(text) = config_file(name).and_then(|path| std::fs::read_to_string(path).ok()) else {
        return Vec::new();
    };
    text.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

fn save_lines(name: &str, lines: &[String]) {
    let Some(path) = config_file(name) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
    let _ = std::fs::write(path, text);
}

pub fn load_palette() -> Palette {
    config_file(PALETTE_FILE).and_then(|path| Palette::load(&path).ok()).unwrap_or_default()
}

pub fn save_palette(palette: &Palette) {
    let Some(path) = config_file(PALETTE_FILE) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = palette.save(&path);
}
