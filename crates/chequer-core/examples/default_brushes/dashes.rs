use chequer_core::brush::{Brush, BrushEngine, BrushSettings, BrushTextures, BrushTip, LayerSpec, Mark, TrackLayer};

use super::classic::brush;

pub const LINES_CATEGORY: &str = "lines";
const DASHED_ID: &str = "dash.dashed";
const DASH_DOT_ID: &str = "dash.dash_dot";
const DOTTED_ID: &str = "dash.dotted";
const DASHED_ROUND_ID: &str = "dash.dashed_round";
const DASH_DOT_ROUND_ID: &str = "dash.dash_dot_round";
const DOTTED_ROUND_ID: &str = "dash.dotted_round";

const SHARP: f32 = 0.0;
const ROUND: f32 = 1.0;

const DASH_PITCH: f32 = 5.0;
const DASH: (f32, f32) = (1.0, 3.0);
const DASH_DOT_PITCH: f32 = 7.0;
const DASH_DOT: [(f32, f32); 2] = [(0.75, 3.0), (5.25, 1.0)];
const DOT_PITCH: f32 = 2.0;
const DOT: (f32, f32) = (0.5, 1.0);

pub fn marked_line(id: &str, pitch: f32, marks: Vec<Mark>) -> Brush {
    let settings = BrushSettings { engine: BrushEngine::Ribbon, follow_direction: true, hardness: 1.0, pressure_size: 0.0, ..BrushSettings::default() };
    let layer = TrackLayer::marked(LayerSpec { pitch, marks, ..LayerSpec::default() });
    let mut line = brush(id, LINES_CATEGORY, settings, BrushTip::Round);
    line.textures = BrushTextures { layers: vec![layer], ..BrushTextures::default() };
    line
}

fn segments(runs: &[(f32, f32)], rounding: f32) -> Vec<Mark> {
    runs.iter().map(|&(start, length)| Mark { start, length, rounding, ..Mark::default() }).collect()
}

pub fn dash_brushes() -> Vec<Brush> {
    vec![
        marked_line(DASHED_ID, DASH_PITCH, segments(&[DASH], SHARP)),
        marked_line(DASH_DOT_ID, DASH_DOT_PITCH, segments(&DASH_DOT, SHARP)),
        marked_line(DOTTED_ID, DOT_PITCH, segments(&[DOT], SHARP)),
        marked_line(DASHED_ROUND_ID, DASH_PITCH, segments(&[DASH], ROUND)),
        marked_line(DASH_DOT_ROUND_ID, DASH_DOT_PITCH, segments(&DASH_DOT, ROUND)),
        marked_line(DOTTED_ROUND_ID, DOT_PITCH, segments(&[DOT], ROUND)),
    ]
}
