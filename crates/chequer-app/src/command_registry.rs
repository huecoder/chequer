use std::sync::OnceLock;

use chequer_core::document::Document;
use chequer_core::registry::{DuplicateId, Registered, Registry};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CommandId(&'static str);

impl CommandId {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub fn as_str(self) -> &'static str {
        self.0
    }
}

pub struct CommandDef {
    pub id: CommandId,
    pub run: fn(&mut Document),
}

impl Registered for CommandDef {
    type Id = CommandId;

    fn id(&self) -> CommandId {
        self.id
    }
}

fn catalog() -> &'static Registry<CommandDef> {
    static CATALOG: OnceLock<Registry<CommandDef>> = OnceLock::new();
    CATALOG.get_or_init(Registry::default)
}

pub fn register_command(def: &'static CommandDef) -> Result<(), DuplicateId<CommandId>> {
    catalog().register(def)
}

pub fn lookup_command(id: CommandId) -> Option<&'static CommandDef> {
    catalog().lookup(id)
}

pub fn all_commands() -> Vec<CommandId> {
    catalog().ids()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_ID: CommandId = CommandId::new("test.noop");

    fn noop(_: &mut Document) {}

    static TEST_COMMAND: CommandDef = CommandDef { id: TEST_ID, run: noop };

    #[test]
    fn command_registers_once_and_is_found() {
        assert_eq!(register_command(&TEST_COMMAND), Ok(()));
        assert!(all_commands().contains(&TEST_ID));
        assert!(lookup_command(TEST_ID).is_some());
        assert_eq!(register_command(&TEST_COMMAND), Err(DuplicateId(TEST_ID)));
    }
}
