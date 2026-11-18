
use eframe::egui::{self, Align, Align2, Color32, Context, CursorIcon, Layout, Rect, Response, Sense, TextureHandle, TextureOptions, Ui, pos2, vec2};

use crate::metrics::GAP_SECTION;
use crate::app::{APP_NAME, App, Command};
use crate::modal::{DIALOG_MARGIN, modal_content_height, title_bar};
use crate::icons::{Icon, icon_side};
use crate::i18n::{Text, t};
use crate::skin::{self, Font};
use crate::widgets::WINDOW_INSET;
use crate::scroll_fx::ScrollFx;

const BANNER_PNG: &[u8] = include_bytes!("../assets/banner/welcome.png");
const BANNER_HEIGHT: f32 = 300.0;
const CREDIT_TEXT: &str = "Banner by Hundert-Man";
const CREDIT_URL: &str = "https://Hundertman.nekoweb.org";
const CREDIT_MARGIN: egui::Vec2 = vec2(12.0, 8.0);
const CREDIT_ALPHA: u8 = 190;
const CREDIT_SHADOW_ALPHA: u8 = 210;
const ROW_HEIGHT: f32 = 26.0;
const ROW_TEXT_INDENT: f32 = 8.0;
const COLUMN_GAP: f32 = 24.0;
const RECENT_VISIBLE_ROWS: f32 = 6.0;
const RECENT_COLUMN_HEIGHT: f32 = ROW_HEIGHT * (RECENT_VISIBLE_ROWS + 1.0);
const COMPACT_ROWS: f32 = 4.0;
const FULL_BODY_HEIGHT: f32 = GAP_SECTION * 2.0 + RECENT_COLUMN_HEIGHT + ROW_HEIGHT;
const COMPACT_BODY_HEIGHT: f32 = GAP_SECTION + ROW_HEIGHT * COMPACT_ROWS;
const SPACING_SLACK: f32 = 32.0;
const MIN_FULL_BANNER_HEIGHT: f32 = 180.0;
const MIN_BANNER_HEIGHT: f32 = 40.0;
const FADE_HEIGHT: f32 = 110.0;
const FADE_STEPS: usize = 24;

fn decode_banner(ctx: &Context) -> TextureHandle {
    let decoded = image::load_from_memory_with_format(BANNER_PNG, image::ImageFormat::Png).expect("встроенный баннер - корректный PNG").into_rgba8();
    let size = [decoded.width() as usize, decoded.height() as usize];
    ctx.load_texture("welcome-banner", egui::ColorImage::from_rgba_unmultiplied(size, decoded.as_raw()), TextureOptions::LINEAR)
}

fn splash_row(ui: &mut Ui, skin: &skin::Skin, width: f32, icon: Icon, label: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::click());
    let theme = skin::theme();
    let ink = if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, theme.highlight);
        theme.highlight_text
    } else {
        theme.ink
    };
    let icon_rect = Rect::from_center_size(pos2(rect.min.x + ROW_TEXT_INDENT + icon_side() / 2.0, rect.center().y), vec2(icon_side(), icon_side()));
    ui.painter().image(skin.icons.texture_id(icon, icon_rect.width()), icon_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    let text_origin = pos2(icon_rect.max.x + ROW_TEXT_INDENT, rect.center().y);
    skin::text_at(&ui.painter().with_clip_rect(rect), text_origin, Align2::LEFT_CENTER, label, Font::Body, ink);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

fn banner_credit(ui: &mut Ui, ctx: &Context, banner: Rect) {
    let width = skin::text_width(ui.painter(), CREDIT_TEXT, Font::Body);
    let rect = Rect::from_min_size(pos2(banner.min.x + CREDIT_MARGIN.x, banner.max.y - CREDIT_MARGIN.y - ROW_HEIGHT / 2.0), vec2(width, ROW_HEIGHT / 2.0));
    let response = ui.interact(rect, ui.scope_id().with("banner-credit"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    let alpha = if response.hovered() { u8::MAX } else { CREDIT_ALPHA };
    skin::text_at(ui.painter(), rect.left_center() + vec2(1.0, 1.0), Align2::LEFT_CENTER, CREDIT_TEXT, Font::Body, Color32::from_black_alpha(CREDIT_SHADOW_ALPHA));
    skin::text_at(ui.painter(), rect.left_center(), Align2::LEFT_CENTER, CREDIT_TEXT, Font::Body, Color32::from_white_alpha(alpha));
    if response.hovered() {
        ui.painter().hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0, Color32::from_white_alpha(alpha)));
    }
    if response.clicked() {
        ctx.open_url(egui::OpenUrl::new_tab(CREDIT_URL));
    }
}

fn paint_fade(painter: &egui::Painter, banner: Rect) {
    let window = skin::theme().window;
    let fade_height = FADE_HEIGHT.min(banner.height() / 2.0);
    let top = banner.max.y - fade_height;
    let mut mesh = egui::Mesh::default();
    for step in 0..=FADE_STEPS {
        let progress = step as f32 / FADE_STEPS as f32;
        let alpha = progress * progress * (3.0 - 2.0 * progress);
        let color = window.gamma_multiply(alpha);
        let y = top + fade_height * progress;
        mesh.colored_vertex(pos2(banner.min.x, y), color);
        mesh.colored_vertex(pos2(banner.max.x, y), color);
        if step > 0 {
            let base = (step * 2) as u32;
            mesh.add_triangle(base - 2, base - 1, base);
            mesh.add_triangle(base - 1, base + 1, base);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn banner_height(available: f32, body: f32) -> f32 {
    (available - body - SPACING_SLACK).clamp(MIN_BANNER_HEIGHT, BANNER_HEIGHT)
}

fn paint_banner(ui: &mut Ui, ctx: &Context, texture: &TextureHandle, height: f32) {
    let reach = f32::from(DIALOG_MARGIN) - WINDOW_INSET;
    let width = ui.available_width() + 2.0 * reach;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height - reach), Sense::hover());
    let banner = Rect::from_min_max(rect.min - vec2(reach, reach), pos2(rect.max.x + reach, rect.max.y));
    let [image_width, image_height] = texture.size();
    let visible = height / (width * image_height as f32 / image_width as f32);
    ui.painter().image(texture.id(), banner, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, visible.min(1.0))), Color32::WHITE);
    paint_fade(ui.painter(), banner);
    banner_credit(ui, ctx, banner);
}

impl App {
    pub(crate) fn welcome_dialog(&mut self, ui: &mut Ui, ctx: &Context, skin: &skin::Skin) -> bool {
        title_bar(ui, APP_NAME);
        let texture = self.welcome_banner.get_or_insert_with(|| decode_banner(ctx)).clone();
        let available = modal_content_height(ctx);
        let compact = banner_height(available, FULL_BODY_HEIGHT) < MIN_FULL_BANNER_HEIGHT;
        let body = if compact { COMPACT_BODY_HEIGHT } else { FULL_BODY_HEIGHT };
        let recent_rows = if compact { COMPACT_ROWS - 1.0 } else { RECENT_VISIBLE_ROWS };
        let column_height = if compact { ROW_HEIGHT * COMPACT_ROWS } else { RECENT_COLUMN_HEIGHT };
        paint_banner(ui, ctx, &texture, banner_height(available, body));
        ui.add_space(GAP_SECTION);
        let column = (ui.available_width() - COLUMN_GAP) / 2.0;
        let recent = self.recent.clone();
        let mut command = None;
        let mut opened = None;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(COLUMN_GAP, 0.0);
            ui.allocate_ui_with_layout(vec2(column, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let settings_row = compact.then_some((Icon::SettingsWindow, t(Text::Settings), Command::Settings));
                for (icon, label, action) in [
                    (Icon::New, t(Text::WelcomeNewImage), Command::New),
                    (Icon::Open, t(Text::WelcomeOpenImage), Command::Open),
                    (Icon::Exit, t(Text::Exit), Command::Exit),
                ]
                .into_iter()
                .chain(settings_row)
                {
                    if splash_row(ui, skin, column, icon, label).clicked() {
                        command = Some(action);
                    }
                }
            });
            ui.allocate_ui_with_layout(vec2(column, column_height), Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(egui::RichText::new(t(Text::OpenRecent)).color(skin::theme().ink_dim));
                if recent.is_empty() {
                    ui.label(egui::RichText::new(t(Text::WelcomeNoRecent)).color(skin::theme().ink_faint));
                }
                egui::ScrollArea::vertical().max_height(ROW_HEIGHT * recent_rows).auto_shrink([false, true]).show(ui, |ui| {
                    for path in &recent {
                        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                        if splash_row(ui, skin, column, Icon::Picture, &name).on_hover_text(path.display().to_string()).clicked() {
                            opened = Some(path.clone());
                        }
                    }
                }).with_fx(ui, skin::theme().window);
            });
        });
        if !compact {
            ui.add_space(GAP_SECTION);
            if splash_row(ui, skin, column, Icon::SettingsWindow, t(Text::Settings)).clicked() {
                command = Some(Command::Settings);
            }
        }
        let acted = opened.is_some() || command.is_some();
        if let Some(path) = opened {
            self.open_path(&path);
        }
        if let Some(command) = command {
            self.run(ctx, command);
        }
        !acted
    }
}
