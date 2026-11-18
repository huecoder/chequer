use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::RwLock;

pub trait Registered {
    type Id: Copy + Eq + Hash + Debug;

    fn id(&self) -> Self::Id;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DuplicateId<Id>(pub Id);

struct Catalog<D: Registered + 'static> {
    order: Vec<&'static D>,
    by_id: HashMap<D::Id, &'static D>,
}

pub struct Registry<D: Registered + 'static> {
    catalog: RwLock<Catalog<D>>,
}

impl<D: Registered + 'static> Default for Registry<D> {
    fn default() -> Self {
        Self { catalog: RwLock::new(Catalog { order: Vec::new(), by_id: HashMap::new() }) }
    }
}

impl<D: Registered + 'static> Registry<D> {
    pub fn with_defs(defs: impl IntoIterator<Item = &'static D>) -> Self {
        let registry = Self::default();
        for def in defs {
            registry.register(def).unwrap_or_else(|error| panic!("встроенное описание зарегистрировано дважды: {error:?}"));
        }
        registry
    }

    pub fn register(&self, def: &'static D) -> Result<(), DuplicateId<D::Id>> {
        let mut catalog = self.catalog.write().unwrap_or_else(|poisoned| poisoned.into_inner());
        if catalog.by_id.contains_key(&def.id()) {
            return Err(DuplicateId(def.id()));
        }
        catalog.by_id.insert(def.id(), def);
        catalog.order.push(def);
        Ok(())
    }

    pub fn lookup(&self, id: D::Id) -> Option<&'static D> {
        self.catalog.read().unwrap_or_else(|poisoned| poisoned.into_inner()).by_id.get(&id).copied()
    }

    pub fn ids(&self) -> Vec<D::Id> {
        self.catalog.read().unwrap_or_else(|poisoned| poisoned.into_inner()).order.iter().map(|def| def.id()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Item(&'static str);

    impl Registered for Item {
        type Id = &'static str;

        fn id(&self) -> &'static str {
            self.0
        }
    }

    static FIRST: Item = Item("first");
    static CLASH: Item = Item("first");

    #[test]
    fn registers_in_order_and_rejects_duplicates() {
        let registry = Registry::with_defs([&FIRST]);
        assert_eq!(registry.ids(), vec!["first"]);
        assert!(registry.lookup("first").is_some());
        assert!(registry.lookup("other").is_none());
        assert_eq!(registry.register(&CLASH), Err(DuplicateId("first")));
    }
}
