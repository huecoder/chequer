use eframe::egui::epaint::{Mesh, WHITE_UV};
use eframe::egui::{self, Color32, Id, Pos2, Rect, Response, Sense, Shape, Stroke, TextEdit, Ui, vec2};

use crate::color::{Hsv, hsv_to_rgb, parse_hex, rgb_to_hsv, to_hex};
use crate::skin::{self, Plate, Skin};
use crate::widgets::{SLIDER_ROW_HEIGHT, Track, WHEEL_SLIDER_STEP, field_scope, gradient_slider, select_all_on_focus};
use chequer_core::raster::Rgba;

pub const SWATCH_SIZE: [f32; 2] = [32.0, 18.0];
const POPUP_WIDTH: f32 = 184.0;
const AREA_HEIGHT: f32 = 116.0;
const PREVIEW_WIDTH: f32 = 28.0;
const HEX_CHAR_LIMIT: usize = 9;
const MARKER_INNER_RADIUS: f32 = 4.0;
const MARKER_OUTER_RADIUS: f32 = 5.0;
const HUE_STOPS: usize = 6;
const DEGREES_PER_HUE_STOP: f32 = 60.0;
const FULL_TURN_DEGREES: f32 = 360.0;
const SPACING: f32 = 6.0;

fn to_color32(color: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3])
}

fn opaque(hsv: Hsv) -> Color32 {
    let [r, g, b] = hsv_to_rgb(hsv);
    Color32::from_rgb(r, g, b)
}

fn paint_swatch(ui: &Ui, skin: &Skin, rect: Rect, color: Rgba) {
    let inner = rect.shrink(skin::plate_inset(ui.painter().pixels_per_point(), skin::FIELD_RINGS));
    let painter = ui.painter().with_clip_rect(inner);
    if color[3] < u8::MAX {
        skin.paint_checker(&painter, inner, inner.min);
    }
    painter.rect_filled(inner, 0.0, to_color32(color));
    skin.paint_plate(ui.painter(), rect, &Plate { fill: Color32::TRANSPARENT, ..Plate::field(skin::theme().field) });
}

fn saturation_value_area(ui: &mut Ui, skin: &Skin, hsv: &mut Hsv) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(POPUP_WIDTH, AREA_HEIGHT), Sense::click_and_drag());
    let well = rect.shrink(skin::plate_inset(ui.painter().pixels_per_point(), skin::FIELD_RINGS));
    let painter = ui.painter().with_clip_rect(well);
    painter.add(Shape::mesh(corner_mesh(well, [Color32::WHITE, opaque(Hsv { s: 1.0, v: 1.0, ..*hsv }), opaque(Hsv { s: 1.0, v: 1.0, ..*hsv }), Color32::WHITE])));
    painter.add(Shape::mesh(corner_mesh(well, [Color32::TRANSPARENT, Color32::TRANSPARENT, Color32::BLACK, Color32::BLACK])));
    skin.paint_plate(ui.painter(), rect, &Plate { fill: Color32::TRANSPARENT, ..Plate::field(skin::theme().field) });
    let mut changed = false;
    if let Some(pos) = response.interact_pointer_pos().filter(|_| response.is_pointer_button_down_on() || response.dragged()) {
        hsv.s = ((pos.x - well.min.x) / well.width()).clamp(0.0, 1.0);
        hsv.v = (1.0 - (pos.y - well.min.y) / well.height()).clamp(0.0, 1.0);
        changed = true;
    }
    let marker = Pos2::new(well.min.x + hsv.s * well.width(), well.max.y - hsv.v * well.height());
    painter.circle_stroke(marker, MARKER_INNER_RADIUS, Stroke::new(1.0, Color32::BLACK));
    painter.circle_stroke(marker, MARKER_OUTER_RADIUS, Stroke::new(1.0, Color32::WHITE));
    changed
}

fn corner_mesh(rect: Rect, colors: [Color32; 4]) -> Mesh {
    let mut mesh = Mesh::default();
    for (corner, color) in [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()].into_iter().zip(colors) {
        mesh.vertices.push(egui::epaint::Vertex { pos: corner, uv: WHITE_UV, color });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    mesh
}

fn hue_slider(ui: &mut Ui, skin: &Skin, hsv: &mut Hsv) -> bool {
    let stops: Vec<(f32, Color32)> = (0..=HUE_STOPS).map(|step| (step as f32 / HUE_STOPS as f32, opaque(Hsv { h: step as f32 * DEGREES_PER_HUE_STOP, s: 1.0, v: 1.0 }))).collect();
    let moved = gradient_slider(ui, skin, hsv.h / FULL_TURN_DEGREES, &Track { stops: &stops, checker: false }, vec2(POPUP_WIDTH, SLIDER_ROW_HEIGHT), false, WHEEL_SLIDER_STEP);
    moved.inspect(|fraction| hsv.h = fraction * FULL_TURN_DEGREES).is_some()
}

fn alpha_slider(ui: &mut Ui, skin: &Skin, color: &mut Rgba) -> bool {
    let (clear, solid) = ([color[0], color[1], color[2], 0], [color[0], color[1], color[2], u8::MAX]);
    let stops = [(0.0, to_color32(clear)), (1.0, to_color32(solid))];
    let moved = gradient_slider(ui, skin, f32::from(color[3]) / f32::from(u8::MAX), &Track { stops: &stops, checker: true }, vec2(POPUP_WIDTH, SLIDER_ROW_HEIGHT), false, WHEEL_SLIDER_STEP);
    moved.inspect(|fraction| color[3] = (fraction * f32::from(u8::MAX)).round() as u8).is_some()
}

fn hex_row(ui: &mut Ui, skin: &Skin, id: Id, color: &mut Rgba) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let (preview, _) = ui.allocate_exact_size(vec2(PREVIEW_WIDTH, SLIDER_ROW_HEIGHT), Sense::hover());
        paint_swatch(ui, skin, preview, *color);
        let hex_id = id.with("hex");
        let focused = ui.memory(|memory| memory.has_focus(hex_id));
        let mut text = ui.data(|data| data.get_temp::<String>(hex_id)).filter(|_| focused).unwrap_or_else(|| to_hex(*color));
        let response = field_scope(ui, skin, |ui| ui.add_sized(vec2(ui.available_width(), SLIDER_ROW_HEIGHT), TextEdit::singleline(&mut text).id(hex_id).char_limit(HEX_CHAR_LIMIT)));
        select_all_on_focus(ui, &response, &text);
        if response.changed() {
            if let Some(parsed) = parse_hex(&text, color[3]) {
                *color = parsed;
                changed = true;
            }
        }
        ui.data_mut(|data| data.insert_temp(hex_id, text));
    });
    changed
}

pub fn color_button(ui: &mut Ui, skin: &Skin, color: &mut Rgba, with_alpha: bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(SWATCH_SIZE[0], SWATCH_SIZE[1]), Sense::click());
    paint_swatch(ui, skin, rect, *color);
    let id = response.id.with("picker");
    let mut changed = false;
    egui::Popup::from_toggle_button_response(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        skin::menu_style(ui);
        ui.set_width(POPUP_WIDTH);
        ui.spacing_mut().item_spacing = vec2(SPACING, SPACING);
        let mut hsv = ui.data(|data| data.get_temp::<Hsv>(id)).filter(|hsv| hsv_to_rgb(*hsv) == [color[0], color[1], color[2]]).unwrap_or_else(|| rgb_to_hsv([color[0], color[1], color[2]]));
        let picked = saturation_value_area(ui, skin, &mut hsv) | hue_slider(ui, skin, &mut hsv);
        if picked {
            let [r, g, b] = hsv_to_rgb(hsv);
            *color = [r, g, b, color[3]];
        }
        let faded = with_alpha && alpha_slider(ui, skin, color);
        let typed = hex_row(ui, skin, id, color);
        changed = picked || faded || typed;
        ui.data_mut(|data| data.insert_temp(id, hsv));
    });
    if changed {
        response.mark_changed();
    }
    response
}
