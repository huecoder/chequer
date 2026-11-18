use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use chequer_core::brush::{BrushTextures, HAIR_KINDS, PAPER_KINDS, PatternKind, PatternSpec, Texture, generate};
use eframe::egui::{self, ColorImage, Context, Id, TextureHandle, TextureOptions, Ui, Vec2, vec2};

use crate::brush_settings::{UNIT_RANGE, Unit, hinted_label, number_row};
use crate::brush_preview::paint_texture;
use crate::i18n::{Text, t};
use crate::skin;
use crate::widgets::{bevel_over, combo_box, text_button};

const PAPER_THUMB: Vec2 = vec2(56.0, 56.0);
const HAIR_THUMB: Vec2 = vec2(150.0, 14.0);
const COMBO_WIDTH: f32 = 120.0;
const MIN_THUMB_WIDTH: f32 = 24.0;
const MIN_COMBO_WIDTH: f32 = 64.0;
const THUMB_CACHE: &str = "texture-thumb";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TextureSlot {
    Paper,
    Hairs,
}

impl TextureSlot {
    pub(crate) fn of(self, textures: &mut BrushTextures) -> &mut Option<Texture> {
        match self {
            Self::Paper => &mut textures.paper,
            Self::Hairs => &mut textures.hairs,
        }
    }

    fn kinds(self) -> &'static [PatternKind] {
        match self {
            Self::Paper => &PAPER_KINDS,
            Self::Hairs => &HAIR_KINDS,
        }
    }

    fn thumb_size(self) -> Vec2 {
        match self {
            Self::Paper => PAPER_THUMB,
            Self::Hairs => HAIR_THUMB,
        }
    }

    fn builtin(self) -> &'static Texture {
        match self {
            Self::Paper => Texture::builtin_paper(),
            Self::Hairs => Texture::builtin_hairs(),
        }
    }

    fn filter(self) -> TextureOptions {
        match self {
            Self::Paper => TextureOptions::LINEAR,
            Self::Hairs => TextureOptions::NEAREST,
        }
    }
}

#[derive(Default)]
pub(crate) struct TextureOutcome {
    pub changed: bool,
    pub pick_file: bool,
}

pub(crate) fn kind_text(kind: PatternKind) -> Text {
    match kind {
        PatternKind::Noise => Text::TextureNoise,
        PatternKind::Fibers => Text::TextureFibers,
        PatternKind::Dots => Text::TextureDots,
        PatternKind::Cells => Text::TextureCells,
        PatternKind::Weave => Text::TextureWeave,
        PatternKind::Bristles => Text::TextureBristles,
    }
}

fn selected_text(texture: &Option<Texture>) -> &'static str {
    match texture {
        None => t(Text::BrushTextureBuiltin),
        Some(texture) => texture.spec().map_or(t(Text::BrushTextureFile), |spec| t(kind_text(spec.kind))),
    }
}

fn content_key(texture: &Texture) -> u64 {
    let mut hasher = DefaultHasher::new();
    (texture.width(), texture.height(), texture.pixels()).hash(&mut hasher);
    hasher.finish()
}

fn thumbnail(ctx: &Context, slot: TextureSlot, texture: &Texture) -> TextureHandle {
    texture_handle(ctx, Id::new((THUMB_CACHE, slot)), texture, slot.filter())
}

pub(crate) fn texture_handle(ctx: &Context, id: Id, texture: &Texture, options: TextureOptions) -> TextureHandle {
    let key = content_key(texture);
    if let Some((cached_key, handle)) = ctx.data(|data| data.get_temp::<(u64, TextureHandle)>(id)) {
        if cached_key == key {
            return handle;
        }
    }
    let image = ColorImage::from_gray([texture.width() as usize, texture.height() as usize], texture.pixels());
    let handle = ctx.load_texture(THUMB_CACHE, image, options);
    ctx.data_mut(|data| data.insert_temp(id, (key, handle.clone())));
    handle
}

pub(crate) fn texture_rows(ui: &mut Ui, skin: &skin::Skin, slot: TextureSlot, label: Text, hint: Text, texture: &mut Option<Texture>) -> TextureOutcome {
    let mut outcome = TextureOutcome::default();
    hinted_label(ui, label, hint);
    ui.horizontal(|ui| {
        let shown = texture.as_ref().unwrap_or_else(|| slot.builtin());
        let handle = thumbnail(ui.ctx(), slot, shown);
        let thumb = slot.thumb_size();
        let room = ui.available_width() - ui.spacing().item_spacing.x;
        let thumb_width = (room - COMBO_WIDTH).clamp(MIN_THUMB_WIDTH, thumb.x);
        let combo_width = (room - thumb_width).clamp(MIN_COMBO_WIDTH, COMBO_WIDTH);
        let (rect, _) = ui.allocate_exact_size(vec2(thumb_width, thumb.y), egui::Sense::hover());
        paint_texture(ui, &handle, rect);
        bevel_over(ui, skin, rect, skin::Bevel::Sunken);
        combo_box(ui, skin, &format!("{THUMB_CACHE}-{slot:?}"), combo_width, selected_text(texture), |ui| {
            if ui.selectable_label(texture.is_none(), t(Text::BrushTextureBuiltin)).clicked() {
                *texture = None;
                outcome.changed = true;
            }
            for &kind in slot.kinds() {
                let current = texture.as_ref().and_then(Texture::spec).is_some_and(|spec| spec.kind == kind);
                if ui.selectable_label(current, t(kind_text(kind))).clicked() && !current {
                    *texture = Some(generate(PatternSpec::preset(kind)));
                    outcome.changed = true;
                }
            }
            if ui.selectable_label(false, t(Text::BrushTextureFile)).clicked() {
                outcome.pick_file = true;
            }
        });
    });
    outcome.changed |= spec_rows(ui, skin, texture);
    outcome
}

fn spec_rows(ui: &mut Ui, skin: &skin::Skin, texture: &mut Option<Texture>) -> bool {
    let Some(before) = texture.as_ref().and_then(Texture::spec) else {
        return false;
    };
    let mut spec = before;
    let bristles = spec.kind == PatternKind::Bristles;
    let (scale_label, scale_hint) = if bristles { (Text::TextureThickness, Text::BrushHintTextureThickness) } else { (Text::TextureScale, Text::BrushHintTextureScale) };
    let (contrast_label, contrast_hint) = if bristles { (Text::TextureUneven, Text::BrushHintTextureUneven) } else { (Text::TextureContrast, Text::BrushHintTextureContrast) };
    number_row(ui, skin, scale_label, scale_hint, &mut spec.scale, UNIT_RANGE, Unit::Percent);
    if spec.kind.uses_density() {
        number_row(ui, skin, Text::TextureDensity, Text::BrushHintTextureDensity, &mut spec.density, UNIT_RANGE, Unit::Percent);
    }
    number_row(ui, skin, contrast_label, contrast_hint, &mut spec.contrast, UNIT_RANGE, Unit::Percent);
    if text_button(ui, skin, t(Text::TextureReseed)).clicked() {
        spec = spec.reseeded();
    }
    if spec == before {
        return false;
    }
    *texture = Some(generate(spec));
    true
}
