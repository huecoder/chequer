use chequer_core::document::{Document, DocumentError, Layer, check_size};
use chequer_core::raster::{Raster, Rgba};

use crate::i18n::{Text, t};

const WHITE: Rgba = [255, 255, 255, 255];
const FIRST_LAYER_NUMBER: usize = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NewBackground {
    #[default]
    Color,
    Transparent,
}

impl NewBackground {
    pub fn to_setting(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Transparent => "transparent",
        }
    }

    pub fn from_setting(value: &str) -> Self {
        if value == Self::Transparent.to_setting() { Self::Transparent } else { Self::Color }
    }

    pub fn label(self) -> &'static str {
        t(match self {
            Self::Color => Text::NewBackgroundColor,
            Self::Transparent => Text::NewBackgroundTransparent,
        })
    }

    pub fn labeled() -> [(NewBackground, &'static str); 2] {
        [Self::Color, Self::Transparent].map(|background| (background, background.label()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewDocumentSetup {
    pub background: NewBackground,
    pub color: Rgba,
}

impl Default for NewDocumentSetup {
    fn default() -> Self {
        Self { background: NewBackground::default(), color: WHITE }
    }
}

impl NewDocumentSetup {
    pub fn build(self, width: u32, height: u32) -> Result<Document, DocumentError> {
        check_size(width, height)?;
        let layer = Layer::new(format!("{} {FIRST_LAYER_NUMBER}", t(Text::LayerN)), Raster::transparent(width, height));
        let layers = match self.background {
            NewBackground::Color => vec![Layer::new(t(Text::Background).to_owned(), Raster::filled(width, height, self.color)), layer],
            NewBackground::Transparent => vec![layer],
        };
        let top = layers.len() - 1;
        Document::from_layers(layers, top)
    }
}
