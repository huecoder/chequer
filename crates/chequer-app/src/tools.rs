use std::sync::Arc;

use chequer_core::blend::BlendMode;
use chequer_core::brush::Brush;
use chequer_core::document::{AlphaMode, Resample};
use chequer_core::gradient::GradientKind;
use chequer_core::raster::Rgba;
use chequer_core::sample::{AlphaMix, MixSpace};
use chequer_core::selection::CombineMode;
use chequer_core::shapes::{ShapeKind, ShapeParams, StrokeAlign};
use chequer_core::text::{TextAlign, TextSmoothing};
use chequer_core::selection::LassoSmoothing;
use chequer_core::spline::LineMode;
use chequer_core::transform::RotationMethod;

use crate::i18n::Text;
use crate::icons::Icon;
use crate::i18n::{TextScope, t, text_or_key};
use crate::tool_registry::{ToolId, all_tools};

const PLUGIN_TOOL_ICON: Icon = Icon::Group;
const FIRST_HOTKEY_LETTER: char = 'a';
const LAST_HOTKEY_LETTER: char = 'z';

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    RectSelect,
    MovePixels,
    Lasso,
    MoveSelection,
    EllipseSelect,
    Zoom,
    MagicWand,
    Pan,
    Bucket,
    Gradient,
    Brush,
    Eraser,
    Pencil,
    Picker,
    CloneStamp,
    Recolor,
    Text,
    Line,
    Shapes,
    Plugin(ToolId),
}

impl Tool {
    pub const GRID: [Tool; 19] = [
        Tool::RectSelect,
        Tool::MovePixels,
        Tool::Lasso,
        Tool::MoveSelection,
        Tool::EllipseSelect,
        Tool::Zoom,
        Tool::MagicWand,
        Tool::Pan,
        Tool::Bucket,
        Tool::Gradient,
        Tool::Brush,
        Tool::Eraser,
        Tool::Pencil,
        Tool::Picker,
        Tool::CloneStamp,
        Tool::Recolor,
        Tool::Text,
        Tool::Line,
        Tool::Shapes,
    ];

    pub fn all() -> Vec<Tool> {
        Self::GRID.into_iter().chain(all_tools().into_iter().map(Tool::Plugin)).collect()
    }

    pub fn is_available(self) -> bool {
        self != Tool::Recolor
    }

    pub fn icon(self) -> Icon {
        match self {
            Tool::RectSelect => Icon::Rect,
            Tool::MovePixels => Icon::MovePixels,
            Tool::Lasso => Icon::Lasso,
            Tool::MoveSelection => Icon::MoveSelection,
            Tool::EllipseSelect => Icon::Ellipse,
            Tool::Zoom => Icon::Zoom,
            Tool::MagicWand => Icon::Wand,
            Tool::Pan => Icon::Pan,
            Tool::Bucket => Icon::Bucket,
            Tool::Gradient => Icon::Gradient,
            Tool::Eraser => Icon::Eraser,
            Tool::Brush => Icon::Brush,
            Tool::Picker => Icon::Picker,
            Tool::Pencil => Icon::Pencil,
            Tool::CloneStamp => Icon::CloneStamp,
            Tool::Recolor => Icon::Recolor,
            Tool::Text => Icon::TextTool,
            Tool::Line => Icon::Line,
            Tool::Shapes => Icon::ShapesTool,
            Tool::Plugin(_) => PLUGIN_TOOL_ICON,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::Plugin(id) => text_or_key(TextScope::Tool, id.as_str()),
            _ => t(self.builtin_name()),
        }
    }

    fn builtin_name(self) -> Text {
        match self {
            Tool::RectSelect => Text::ToolRectSelect,
            Tool::MovePixels => Text::ToolMovePixels,
            Tool::Lasso => Text::ToolLasso,
            Tool::MoveSelection => Text::ToolMoveSelection,
            Tool::EllipseSelect => Text::ToolEllipseSelect,
            Tool::Zoom => Text::ToolZoom,
            Tool::MagicWand => Text::ToolMagicWand,
            Tool::Pan => Text::ToolPan,
            Tool::Bucket => Text::ToolBucket,
            Tool::Gradient => Text::ToolGradient,
            Tool::Eraser => Text::ToolEraser,
            Tool::Brush => Text::ToolBrush,
            Tool::Picker => Text::ToolPicker,
            Tool::Pencil => Text::ToolPencil,
            Tool::CloneStamp => Text::ToolCloneStamp,
            Tool::Recolor => Text::ToolRecolor,
            Tool::Text => Text::ToolText,
            Tool::Line => Text::ToolLine,
            Tool::Shapes => Text::ToolShapes,
            Tool::Plugin(_) => unreachable!("плагинный инструмент называется по ключу"),
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Tool::Plugin(id) => text_or_key(TextScope::ToolHint, id.as_str()),
            _ => t(self.builtin_hint()),
        }
    }

    fn builtin_hint(self) -> Text {
        match self {
            Tool::RectSelect | Tool::EllipseSelect => Text::HintRectSelect,
            Tool::MovePixels => Text::HintMovePixels,
            Tool::Lasso => Text::HintLasso,
            Tool::MoveSelection => Text::HintMoveSelection,
            Tool::Zoom => Text::HintZoom,
            Tool::MagicWand => Text::HintWand,
            Tool::Pan => Text::HintPan,
            Tool::Bucket => Text::HintBucket,
            Tool::Gradient => Text::HintGradient,
            Tool::Eraser => Text::HintEraser,
            Tool::Brush => Text::HintBrush,
            Tool::Picker => Text::HintPicker,
            Tool::Pencil => Text::HintPencil,
            Tool::CloneStamp => Text::HintCloneStamp,
            Tool::Recolor => Text::SoonHint,
            Tool::Text => Text::HintText,
            Tool::Line => Text::HintLine,
            Tool::Shapes => Text::HintShapes,
            Tool::Plugin(_) => unreachable!("плагинный инструмент подсказывается по ключу"),
        }
    }

    pub fn is_selection_tool(self) -> bool {
        matches!(self, Tool::RectSelect | Tool::EllipseSelect | Tool::Lasso | Tool::MagicWand)
    }

    pub fn uses_brush_size(self) -> bool {
        matches!(self, Tool::Brush | Tool::Eraser | Tool::CloneStamp | Tool::Line | Tool::Shapes)
    }

    pub fn is_shape_tool(self) -> bool {
        matches!(self, Tool::Line | Tool::Shapes)
    }

    pub fn hotkey_group(key: char) -> &'static [Tool] {
        match key {
            's' => &[Tool::RectSelect, Tool::Lasso, Tool::EllipseSelect, Tool::MagicWand],
            'm' => &[Tool::MovePixels, Tool::MoveSelection],
            'o' => &[Tool::Line, Tool::Shapes],
            'f' => &[Tool::Bucket],
            'g' => &[Tool::Gradient],
            'b' => &[Tool::Brush],
            'p' => &[Tool::Pencil],
            'e' => &[Tool::Eraser],
            'k' => &[Tool::Picker],
            'l' => &[Tool::CloneStamp],
            'z' => &[Tool::Zoom],
            'h' => &[Tool::Pan],
            _ => &[],
        }
    }

    pub fn hotkey(self) -> Option<char> {
        (FIRST_HOTKEY_LETTER..=LAST_HOTKEY_LETTER).find(|&key| Self::hotkey_group(key).contains(&self))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillStyle {
    Outline,
    Filled,
    Both,
}

impl FillStyle {
    pub fn colors(self, primary: Rgba, secondary: Rgba) -> (Option<Rgba>, Option<Rgba>) {
        match self {
            FillStyle::Outline => (Some(primary), None),
            FillStyle::Filled => (None, Some(primary)),
            FillStyle::Both => (Some(primary), Some(secondary)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextUnit {
    #[default]
    Point,
    Pixel,
}

impl TextUnit {
    pub fn to_setting(self) -> &'static str {
        match self {
            TextUnit::Point => "pt",
            TextUnit::Pixel => "px",
        }
    }

    pub fn from_setting(value: &str) -> Self {
        if value == "px" { TextUnit::Pixel } else { TextUnit::Point }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            TextUnit::Point => " pt",
            TextUnit::Pixel => " px",
        }
    }

    pub fn from_pixels(self, pixels: f32) -> f32 {
        match self {
            TextUnit::Point => pixels * POINTS_PER_INCH / SCREEN_DPI,
            TextUnit::Pixel => pixels,
        }
    }

    pub fn to_pixels(self, value: f32) -> f32 {
        match self {
            TextUnit::Point => value * SCREEN_DPI / POINTS_PER_INCH,
            TextUnit::Pixel => value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sampling {
    Layer,
    Image,
}

pub const PICK_SIZES: [(u32, &str); 5] = [(1, "1×1"), (3, "3×3"), (5, "5×5"), (9, "9×9"), (17, "17×17")];

const SCREEN_DPI: f32 = 96.0;
const POINTS_PER_INCH: f32 = 72.0;
pub const TEXT_SIZE_DEFAULT: f32 = 32.0;
pub const TEXT_SIZE_MIN: f32 = 4.0;
pub const TEXT_SIZE_MAX: f32 = 1000.0;
pub const TEXT_OUTLINE_WIDTH_MIN: f32 = 0.5;
pub const TEXT_OUTLINE_WIDTH_MAX: f32 = 100.0;

#[derive(Clone, Debug, PartialEq)]
pub struct TextOptions {
    pub family: Option<String>,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub hinting: bool,
    pub smoothing: TextSmoothing,
    pub align: TextAlign,
    pub fill_style: FillStyle,
    pub outline_width: f32,
    pub outline_align: StrokeAlign,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            family: None,
            size: TEXT_SIZE_DEFAULT,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            hinting: true,
            smoothing: TextSmoothing::Grayscale,
            align: TextAlign::Left,
            fill_style: FillStyle::Filled,
            outline_width: 1.0,
            outline_align: StrokeAlign::Outside,
        }
    }
}

pub const BRUSH_SIZE_MIN: f32 = 1.0;
pub const BRUSH_SIZE_MAX: f32 = 2000.0;
pub const BRUSH_SIZE_STEP: f32 = 1.0;
pub const BRUSH_SIZE_STEP_LARGE: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveMethod {
    Transform,
    Perspective,
}

#[derive(Clone, Debug)]
pub struct ToolOptions {
    pub brush: Arc<Brush>,
    pub eraser_brush: Arc<Brush>,
    pub size: f32,
    pub antialias: bool,
    pub pixel_perfect: bool,
    pub smooth_path: bool,
    pub blend: BlendMode,
    pub tolerance: f32,
    pub flood_global: bool,
    pub sampling: Sampling,
    pub pick_size: u32,
    pub pick_space: MixSpace,
    pub pick_alpha: AlphaMix,
    pub fill_style: FillStyle,
    pub shape_kind: ShapeKind,
    pub shape_params: ShapeParams,
    pub gradient_kind: GradientKind,
    pub select_mode: CombineMode,
    pub float_mode: AlphaMode,
    pub move_resample: Resample,
    pub rotation: RotationMethod,
    pub move_method: MoveMethod,
    pub lasso_smoothing: LassoSmoothing,
    pub line_mode: LineMode,
    pub text: TextOptions,
}

impl ToolOptions {
    pub fn brush_for(&self, tool: Tool) -> &Arc<Brush> {
        if tool == Tool::Eraser { &self.eraser_brush } else { &self.brush }
    }

    pub fn brush_for_mut(&mut self, tool: Tool) -> &mut Arc<Brush> {
        if tool == Tool::Eraser { &mut self.eraser_brush } else { &mut self.brush }
    }
}

impl Default for ToolOptions {
    fn default() -> Self {
        Self {
            brush: Arc::default(),
            eraser_brush: Arc::default(),
            size: 2.0,
            antialias: true,
            pixel_perfect: true,
            smooth_path: false,
            blend: BlendMode::Normal,
            tolerance: 0.5,
            flood_global: false,
            sampling: Sampling::Layer,
            pick_size: PICK_SIZES[0].0,
            pick_space: MixSpace::Linear,
            pick_alpha: AlphaMix::Weighted,
            fill_style: FillStyle::Outline,
            shape_kind: ShapeKind::Rectangle,
            shape_params: ShapeParams::default(),
            gradient_kind: GradientKind::Linear,
            select_mode: CombineMode::Replace,
            float_mode: AlphaMode::Blend,
            move_resample: Resample::Bilinear,
            rotation: RotationMethod::Plain,
            move_method: MoveMethod::Transform,
            lasso_smoothing: LassoSmoothing::Classic,
            line_mode: LineMode::Straight,
            text: TextOptions::default(),
        }
    }
}

impl ToolOptions {
    pub fn adjust_size(&mut self, delta: f32) {
        self.size = (self.size + delta).clamp(BRUSH_SIZE_MIN, BRUSH_SIZE_MAX);
    }

    pub fn shape_colors(&self, primary: Rgba, secondary: Rgba) -> (Option<Rgba>, Option<Rgba>) {
        self.fill_style.colors(primary, secondary)
    }
}
