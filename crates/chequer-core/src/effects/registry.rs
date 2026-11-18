use std::sync::OnceLock;

use super::{Category, EffectDef, ParamSpec};
use crate::registry::{DuplicateId, Registered, Registry};
use super::{adjust, artistic, blur, color, distort, glitch, noise, object, photo, render, stylize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EffectId(&'static str);

impl EffectId {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub fn as_str(self) -> &'static str {
        self.0
    }

    pub fn def(self) -> &'static EffectDef {
        lookup(self).unwrap_or_else(|| panic!("у эффекта {self:?} нет описания"))
    }

    pub fn category(self) -> Category {
        self.def().category
    }

    pub fn params(self) -> &'static [ParamSpec] {
        self.def().params
    }
}

impl serde::Serialize for EffectId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

impl<'de> serde::Deserialize<'de> for EffectId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        all_effects().into_iter().find(|id| id.as_str() == name).ok_or_else(|| serde::de::Error::custom(format!("неизвестный эффект «{name}»")))
    }
}

impl Registered for EffectDef {
    type Id = EffectId;

    fn id(&self) -> EffectId {
        self.id
    }
}

fn catalog() -> &'static Registry<EffectDef> {
    static CATALOG: OnceLock<Registry<EffectDef>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let builtin = [adjust::DEFS.as_slice(), photo::DEFS.as_slice(), distort::DEFS.as_slice(), object::DEFS.as_slice(), blur::DEFS.as_slice(), stylize::DEFS.as_slice(), render::DEFS.as_slice(), artistic::DEFS.as_slice(), color::DEFS.as_slice(), noise::DEFS.as_slice(), glitch::DEFS.as_slice()];
        Registry::with_defs(builtin.into_iter().flatten())
    })
}

pub fn register(def: &'static EffectDef) -> Result<(), DuplicateId<EffectId>> {
    catalog().register(def)
}

pub fn lookup(id: EffectId) -> Option<&'static EffectDef> {
    catalog().lookup(id)
}

pub fn all_effects() -> Vec<EffectId> {
    catalog().ids()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Job;
    use crate::raster::Raster;

    const EXTERNAL_ID: EffectId = EffectId::new("test.external");

    fn passthrough(job: &Job) -> Option<Raster> {
        Some(job.source.crop(job.area))
    }

    static EXTERNAL: EffectDef = EffectDef { id: EXTERNAL_ID, category: Category::Color, params: &[], render: passthrough };

    #[test]
    fn external_effect_joins_catalog_once() {
        assert_eq!(register(&EXTERNAL), Ok(()));
        assert!(all_effects().contains(&EXTERNAL_ID));
        assert_eq!(EXTERNAL_ID.category(), Category::Color);
        assert_eq!(register(&EXTERNAL), Err(DuplicateId(EXTERNAL_ID)));
    }
}
