use chequer_core::brush::{BrushEngine, BrushSettings};

use crate::i18n::Text;

const HISTORY_LIMIT: usize = 100;

#[derive(Clone, Copy, PartialEq)]
pub enum SettingsPreset {
    Default,
    Pen,
    Airbrush,
    Calligraphy,
    Watercolor,
    Scatter,
    Stamp,
    Ribbon,
}

pub const SETTINGS_PRESETS: [SettingsPreset; 8] = [
    SettingsPreset::Default,
    SettingsPreset::Pen,
    SettingsPreset::Airbrush,
    SettingsPreset::Calligraphy,
    SettingsPreset::Watercolor,
    SettingsPreset::Scatter,
    SettingsPreset::Stamp,
    SettingsPreset::Ribbon,
];

impl SettingsPreset {
    pub fn label(self) -> Text {
        match self {
            Self::Default => Text::PresetDefault,
            Self::Pen => Text::PresetPen,
            Self::Airbrush => Text::PresetAirbrush,
            Self::Calligraphy => Text::PresetCalligraphy,
            Self::Watercolor => Text::PresetWatercolor,
            Self::Scatter => Text::PresetScatter,
            Self::Stamp => Text::PresetStamp,
            Self::Ribbon => Text::PresetRibbon,
        }
    }

    pub fn settings(self) -> BrushSettings {
        let base = BrushSettings::default();
        match self {
            Self::Default => base,
            Self::Pen => BrushSettings {
                spacing: 0.05,
                pressure_size: 1.0,
                pressure_opacity: 0.4,
                hardness: 0.9,
                ..base
            },
            Self::Airbrush => BrushSettings {
                spacing: 0.05,
                flow: 0.25,
                accumulate: true,
                hardness: 0.0,
                pressure_opacity: 0.5,
                ..base
            },
            Self::Calligraphy => BrushSettings {
                spacing: 0.04,
                angle: 45.0,
                roundness: 0.25,
                pressure_size: 0.5,
                hardness: 1.0,
                ..base
            },
            Self::Watercolor => BrushSettings {
                spacing: 0.08,
                flow: 0.35,
                opacity_jitter: 0.25,
                pressure_opacity: 0.6,
                hardness: 0.3,
                ..base
            },
            Self::Scatter => BrushSettings {
                spacing: 0.5,
                scatter: 1.5,
                size_jitter: 0.5,
                angle_jitter: 1.0,
                opacity_jitter: 0.3,
                ..base
            },
            Self::Stamp => BrushSettings {
                spacing: 1.0,
                follow_direction: true,
                pressure_size: 0.0,
                ..base
            },
            Self::Ribbon => BrushSettings {
                engine: BrushEngine::Ribbon,
                angle: 40.0,
                roundness: 0.22,
                pressure_size: 0.8,
                velocity_size: 0.2,
                size_start: 0.25,
                size_fade: 5.0,
                ..base
            },
        }
    }
}

#[derive(Default)]
pub struct SettingsHistory {
    undo: Vec<BrushSettings>,
    redo: Vec<BrushSettings>,
    open_step: Option<BrushSettings>,
}

impl SettingsHistory {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn begin_step(&mut self, before: BrushSettings) {
        self.open_step.get_or_insert(before);
    }

    pub fn record_step(&mut self, before: BrushSettings) {
        self.begin_step(before);
        self.seal();
    }

    pub fn seal_when_idle(&mut self, pointer_down: bool) {
        if !pointer_down {
            self.seal();
        }
    }

    pub fn undo(&mut self, current: &BrushSettings) -> Option<BrushSettings> {
        self.seal();
        let previous = self.undo.pop()?;
        self.redo.push(current.clone());
        Some(previous)
    }

    pub fn redo(&mut self, current: &BrushSettings) -> Option<BrushSettings> {
        self.seal();
        let next = self.redo.pop()?;
        self.undo.push(current.clone());
        Some(next)
    }

    fn seal(&mut self) {
        if let Some(step) = self.open_step.take() {
            self.undo.push(step);
            self.redo.clear();
            if self.undo.len() > HISTORY_LIMIT {
                self.undo.remove(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_merges_open_step_and_restores_both_ways() {
        let mut history = SettingsHistory::default();
        let initial = BrushSettings::default();
        history.begin_step(initial.clone());
        history.begin_step(BrushSettings { spacing: 0.5, ..initial.clone() });
        history.seal_when_idle(false);
        let edited = BrushSettings { spacing: 0.9, ..initial.clone() };
        assert_eq!(history.undo(&edited), Some(initial.clone()));
        assert_eq!(history.undo(&initial), None);
        assert_eq!(history.redo(&initial), Some(edited));
    }
}
