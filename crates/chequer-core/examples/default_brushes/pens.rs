use chequer_core::brush::{Brush, BrushEngine, BrushSettings, BrushTip, Curve, TipSpec, build_tip};

use super::classic::brush;

const DRAWING_CATEGORY: &str = "drawing";
const MONOLINE_ID: &str = "rib.monoline";
const INKER_ID: &str = "rib.inker";
const SOFT_TAPER_ID: &str = "rib.soft_taper";
const FLOURISH_ID: &str = "rib.flourish";
const TILT_NIB_ID: &str = "rib.tilt_nib";
const CHALK_MARKER_ID: &str = "rib.chalk_marker";
const DRY_MARKER_ID: &str = "dry.ink_marker";
const DRY_FLAT_BRISTLE_ID: &str = "dry.flat_bristle";
const SPLIT_NIB_ID: &str = "cal.split_nib";
const SPLIT_NIB_SPACING: f32 = 0.04;
const SPLIT_NIB_FLOW: f32 = 0.95;
const SPLIT_NIB_PRESSURE_SIZE: f32 = 0.3;
const TINES_ACROSS_HEADING_DEGREES: f32 = 90.0;

const INKER_HAIRLINE: f32 = 0.12;
const INKER_MIDDLE: f32 = 0.55;
const SOFT_TAPER_HAIRLINE: f32 = 0.05;
const SOFT_TAPER_KNEE: (f32, f32) = (0.35, 0.5);
const FLOURISH_HAIRLINE: f32 = 0.03;
const FLOURISH_KNEE: (f32, f32) = (0.6, 0.4);

pub fn pen_brushes() -> Vec<Brush> {
    let ribbon = BrushSettings { engine: BrushEngine::Ribbon, hardness: 1.0, ..BrushSettings::default() };
    vec![
        brush(MONOLINE_ID, DRAWING_CATEGORY, BrushSettings { pressure_size: 0.08, ..ribbon.clone() }, BrushTip::Round),
        brush(
            INKER_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                pressure_size: 1.0,
                pressure_size_curve: Curve::new(vec![(0.0, INKER_HAIRLINE), (0.5, INKER_MIDDLE), (1.0, 1.0)]),
                velocity_size: 0.45,
                size_start: 0.25,
                size_fade: 3.0,
                ..ribbon.clone()
            },
            BrushTip::Round,
        ),
        brush(
            SOFT_TAPER_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                roundness: 0.7,
                pressure_size: 0.9,
                pressure_size_curve: Curve::new(vec![(0.0, SOFT_TAPER_HAIRLINE), SOFT_TAPER_KNEE, (1.0, 1.0)]),
                pressure_opacity: 0.15,
                velocity_size: 0.6,
                size_start: 0.08,
                size_fade: 7.0,
                ..ribbon.clone()
            },
            BrushTip::Round,
        ),
        brush(
            FLOURISH_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                pressure_size: 1.0,
                pressure_size_curve: Curve::new(vec![(0.0, FLOURISH_HAIRLINE), FLOURISH_KNEE, (1.0, 1.0)]),
                velocity_size: 0.8,
                size_start: 0.05,
                size_fade: 10.0,
                ..ribbon.clone()
            },
            BrushTip::Round,
        ),
        brush(TILT_NIB_ID, DRAWING_CATEGORY, BrushSettings { roundness: 0.2, tilt_angle: 1.0, pressure_size: 0.4, ..ribbon.clone() }, BrushTip::Round),
        brush(
            CHALK_MARKER_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                angle: 30.0,
                roundness: 0.3,
                flow: 0.92,
                pressure_size: 0.35,
                grain: 0.55,
                grain_scale: 2.5,
                bristles: 0.3,
                velocity_dryness: 0.5,
                pressure_dryness: 0.4,
                ..ribbon.clone()
            },
            BrushTip::Round,
        ),
        brush(
            DRY_MARKER_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                angle: 35.0,
                roundness: 0.3,
                flow: 0.96,
                pressure_size: 0.3,
                bristles: 0.8,
                dryness: 0.12,
                velocity_dryness: 0.45,
                pressure_dryness: 0.25,
                depletion: 0.1,
                grain: 0.2,
                grain_scale: 2.0,
                ..ribbon.clone()
            },
            BrushTip::Round,
        ),
        brush(
            DRY_FLAT_BRISTLE_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                follow_direction: true,
                angle: 90.0,
                roundness: 0.16,
                flow: 0.92,
                pressure_size: 0.35,
                size_start: 0.6,
                size_fade: 2.0,
                bristles: 1.0,
                dryness: 0.1,
                velocity_dryness: 0.5,
                pressure_dryness: 0.25,
                grain: 0.25,
                grain_scale: 2.0,
                ..ribbon
            },
            BrushTip::Round,
        ),
        brush(
            SPLIT_NIB_ID,
            DRAWING_CATEGORY,
            BrushSettings {
                spacing: SPLIT_NIB_SPACING,
                hardness: 1.0,
                flow: SPLIT_NIB_FLOW,
                follow_direction: true,
                angle: TINES_ACROSS_HEADING_DEGREES,
                pressure_size: SPLIT_NIB_PRESSURE_SIZE,
                ..BrushSettings::default()
            },
            split_nib_tip(),
        ),
    ]
}

fn split_nib_tip() -> BrushTip {
    let spec = TipSpec { generator: "split".into(), params: Default::default(), seed: 0 };
    build_tip(&spec).map_or(BrushTip::Round, BrushTip::Mask)
}
