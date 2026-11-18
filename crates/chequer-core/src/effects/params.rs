
use super::EffectId;
use super::curve::Curve;
use crate::blend::BlendMode;
use crate::raster::Rgba;
use super::util::Edge;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSource {
    Fixed,
    Primary,
    Secondary,
}

#[derive(Clone, Copy, Debug)]
pub enum ParamKind {
    Int { min: i32, max: i32, default: i32 },
    Float { min: f32, max: f32, default: f32, step: f32 },
    Angle { min: f32, max: f32, default: f32 },
    Bool { default: bool },
    Choice { options: &'static [&'static str], default: usize },
    Blend,
    Point,
    Seed,
    Color { default: Rgba, source: ColorSource },
    Curve,
}

pub type Shown = (&'static str, &'static [&'static str]);

const ALWAYS_SHOWN: &[Shown] = &[];

#[derive(Clone, Copy, Debug)]
pub struct ParamSpec {
    pub key: &'static str,
    pub kind: ParamKind,
    pub shown_when: &'static [Shown],
}

impl ParamSpec {
    pub const fn only_if(self, conditions: &'static [Shown]) -> Self {
        Self { shown_when: conditions, ..self }
    }
}

pub const fn int(key: &'static str, min: i32, max: i32, default: i32) -> ParamSpec {
    ParamSpec { key, shown_when: ALWAYS_SHOWN, kind: ParamKind::Int { min, max, default } }
}

pub const fn float(key: &'static str, min: f32, max: f32, default: f32, step: f32) -> ParamSpec {
    ParamSpec { key, shown_when: ALWAYS_SHOWN, kind: ParamKind::Float { min, max, default, step } }
}

pub const fn angle(key: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
    ParamSpec { key, shown_when: ALWAYS_SHOWN, kind: ParamKind::Angle { min, max, default } }
}

pub const fn flag(key: &'static str, default: bool) -> ParamSpec {
    ParamSpec { key, shown_when: ALWAYS_SHOWN, kind: ParamKind::Bool { default } }
}

pub const fn choice(key: &'static str, options: &'static [&'static str], default: usize) -> ParamSpec {
    ParamSpec { key, shown_when: ALWAYS_SHOWN, kind: ParamKind::Choice { options, default } }
}

pub const fn blend(key: &'static str) -> ParamSpec {
    ParamSpec { key, shown_when: ALWAYS_SHOWN, kind: ParamKind::Blend }
}

pub const fn center(key: &'static str) -> ParamSpec {
    ParamSpec { key, shown_when: ALWAYS_SHOWN, kind: ParamKind::Point }
}

pub const fn seed(key: &'static str) -> ParamSpec {
    ParamSpec { key, shown_when: ALWAYS_SHOWN, kind: ParamKind::Seed }
}

pub const fn curve(key: &'static str) -> ParamSpec {
    ParamSpec { key, shown_when: ALWAYS_SHOWN, kind: ParamKind::Curve }
}

pub const fn color(key: &'static str, default: Rgba, source: ColorSource) -> ParamSpec {
    ParamSpec { key, shown_when: ALWAYS_SHOWN, kind: ParamKind::Color { default, source } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub primary: Rgba,
    pub secondary: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamValue {
    Int(i32),
    Float(f32),
    Bool(bool),
    Choice(usize),
    Point(f32, f32),
    Seed(u32),
    Color(Rgba),
    Curve(Curve),
}

impl ParamKind {
    pub fn default_value(self, palette: Palette) -> ParamValue {
        match self {
            ParamKind::Int { default, .. } => ParamValue::Int(default),
            ParamKind::Float { default, .. } | ParamKind::Angle { default, .. } => ParamValue::Float(default),
            ParamKind::Bool { default } => ParamValue::Bool(default),
            ParamKind::Choice { default, .. } => ParamValue::Choice(default),
            ParamKind::Blend => ParamValue::Choice(0),
            ParamKind::Point => ParamValue::Point(0.0, 0.0),
            ParamKind::Seed => ParamValue::Seed(0),
            ParamKind::Curve => ParamValue::Curve(Curve::identity()),
            ParamKind::Color { default, source } => ParamValue::Color(match source {
                ColorSource::Fixed => default,
                ColorSource::Primary => palette.primary,
                ColorSource::Secondary => palette.secondary,
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectParams {
    id: EffectId,
    values: Vec<ParamValue>,
}

impl EffectParams {
    pub fn defaults(id: EffectId, palette: Palette) -> Self {
        let values = id.params().iter().map(|spec| spec.kind.default_value(palette)).collect();
        Self { id, values }
    }

    pub fn id(&self) -> EffectId {
        self.id
    }

    pub fn values(&self) -> &[ParamValue] {
        &self.values
    }

    pub fn set(&mut self, index: usize, value: ParamValue) {
        self.values[index] = value;
    }

    pub fn reset(&mut self, index: usize, palette: Palette) {
        self.values[index] = self.id.params()[index].kind.default_value(palette);
    }

    pub fn is_shown(&self, index: usize) -> bool {
        self.id.params()[index].shown_when.iter().all(|(key, options)| options.contains(&self.choice_key(key)))
    }

    fn value(&self, key: &str) -> ParamValue {
        match self.id.params().iter().position(|spec| spec.key == key) {
            Some(index) => self.values[index],
            None => panic!("у эффекта {:?} нет параметра «{key}»", self.id),
        }
    }

    pub fn int(&self, key: &str) -> i32 {
        match self.value(key) {
            ParamValue::Int(v) => v,
            other => panic!("параметр «{key}» эффекта {:?}: ожидалось целое, лежит {other:?}", self.id),
        }
    }

    pub fn float(&self, key: &str) -> f32 {
        match self.value(key) {
            ParamValue::Float(v) => v,
            ParamValue::Int(v) => v as f32,
            other => panic!("параметр «{key}» эффекта {:?}: ожидалось дробное, лежит {other:?}", self.id),
        }
    }

    pub fn bool(&self, key: &str) -> bool {
        match self.value(key) {
            ParamValue::Bool(v) => v,
            other => panic!("параметр «{key}» эффекта {:?}: ожидался флажок, лежит {other:?}", self.id),
        }
    }

    pub fn choice(&self, key: &str) -> usize {
        match self.value(key) {
            ParamValue::Choice(v) => v,
            other => panic!("параметр «{key}» эффекта {:?}: ожидался выбор, лежит {other:?}", self.id),
        }
    }

    pub fn choice_key(&self, key: &str) -> &'static str {
        let index = self.choice(key);
        match self.id.params().iter().find(|spec| spec.key == key).map(|spec| spec.kind) {
            Some(ParamKind::Choice { options, .. }) => options[index],
            _ => panic!("параметр «{key}» эффекта {:?} не список", self.id),
        }
    }

    pub fn edge(&self, key: &str) -> Edge {
        Edge::from_key(self.choice_key(key))
    }

    pub fn blend_mode(&self, key: &str) -> BlendMode {
        BlendMode::ALL[self.choice(key).min(BlendMode::ALL.len() - 1)]
    }

    pub fn point(&self, key: &str) -> (f32, f32) {
        match self.value(key) {
            ParamValue::Point(x, y) => (x, y),
            other => panic!("параметр «{key}» эффекта {:?}: ожидалась точка, лежит {other:?}", self.id),
        }
    }

    pub fn seed(&self, key: &str) -> u32 {
        match self.value(key) {
            ParamValue::Seed(v) => v,
            other => panic!("параметр «{key}» эффекта {:?}: ожидалось зерно, лежит {other:?}", self.id),
        }
    }

    pub fn color(&self, key: &str) -> Rgba {
        match self.value(key) {
            ParamValue::Color(v) => v,
            other => panic!("параметр «{key}» эффекта {:?}: ожидался цвет, лежит {other:?}", self.id),
        }
    }

    pub fn curve(&self, key: &str) -> Curve {
        match self.value(key) {
            ParamValue::Curve(v) => v,
            other => panic!("параметр «{key}» эффекта {:?}: ожидалась кривая, лежит {other:?}", self.id),
        }
    }
}
