#![cfg_attr(windows, windows_subsystem = "windows")]

mod filepicker;
mod ants;
mod anim;
mod app;
mod brush_dialog;
mod brush_editor;
mod brush_library;
mod brush_list;
mod brush_presets;
mod brush_preview;
mod brush_settings;
mod texture_editor;
mod layer_editor;
mod tip_generator_editor;
mod brush_texts;
mod scroll_fx;
mod canvas;
mod clipboard;
mod color;
mod color_popup;
mod curve_editor;
mod config;
pub mod command_registry;
mod dialogs;
mod effect_texts;
mod input_clock;
mod metrics;
#[cfg(target_os = "android")]
mod bridge;
mod device;
mod modal;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
mod pen_feed;
mod new_document;
mod effects_ui;
mod font_picker;
mod fonts;
mod formula;
mod handles;
mod i18n;
mod icons;
mod live;
mod menus;
mod palette;
mod panels;
mod pen;
mod recovery;
mod selection_ui;
mod skin;
mod sound;
mod stylus;
mod text_tool;
mod themes;
mod touch;
mod plugin_tools;
mod tools;
mod timeline;
pub mod tool_registry;
mod trackpad;
mod view;
mod welcome;
mod widgets;

use std::path::PathBuf;

use eframe::egui;

const WINDOW_TITLE: &str = "Chequer";
const INITIAL_WINDOW_SIZE: [f32; 2] = [1280.0, 760.0];
const MIN_WINDOW_SIZE: [f32; 2] = [800.0, 500.0];
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../assets/icon/chequer-256.png");

fn window_icon() -> egui::IconData {
    let decoded = image::load_from_memory_with_format(WINDOW_ICON_PNG, image::ImageFormat::Png).expect("встроенная иконка - корректный PNG").into_rgba8();
    egui::IconData { width: decoded.width(), height: decoded.height(), rgba: decoded.into_raw() }
}

fn run(files: Vec<PathBuf>, extra: impl FnOnce(&mut eframe::NativeOptions)) -> eframe::Result {
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_app_id("chequer")
            .with_icon(window_icon())
            .with_inner_size(INITIAL_WINDOW_SIZE)
            .with_min_inner_size(MIN_WINDOW_SIZE),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration { surface: eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY, ..Default::default() },
        ..Default::default()
    };
    extra(&mut options);
    eframe::run_native(WINDOW_TITLE, options, Box::new(|cc| Ok(Box::new(app::App::new(cc, files)))))
}

pub fn run_desktop() -> eframe::Result {
    run(std::env::args().skip(1).map(PathBuf::from).collect(), |_| {})
}

#[cfg(not(target_os = "android"))]
pub fn show_startup_error(error: &eframe::Error) {
    i18n::set_lang(config::load_lang());
    rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title(i18n::t(i18n::Text::StartupFailed)).set_description(error.to_string()).show();
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    if let Some(dir) = app.internal_data_path() {
        bridge::set_dir(dir.clone());
        config::set_android_data_dir(dir);
    }
    let _ = run(Vec::new(), |options| options.android_app = Some(app));
}
