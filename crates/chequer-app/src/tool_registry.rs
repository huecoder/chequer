use std::sync::OnceLock;

use chequer_core::document::Document;
use chequer_core::geom::{FPoint, IRect};
use chequer_core::raster::Rgba;
use chequer_core::registry::{DuplicateId, Registered, Registry};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ToolId(&'static str);

impl ToolId {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolButton {
    Primary,
    Secondary,
}

#[derive(Clone, Copy, Debug)]
pub struct PointerInput {
    pub position: FPoint,
    pub button: ToolButton,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub pressure: f32,
}

pub struct ToolContext<'a> {
    doc: &'a mut Document,
    dirty: &'a mut Vec<IRect>,
    primary: Rgba,
    secondary: Rgba,
}

impl<'a> ToolContext<'a> {
    pub fn new(doc: &'a mut Document, dirty: &'a mut Vec<IRect>, primary: Rgba, secondary: Rgba) -> Self {
        Self { doc, dirty, primary, secondary }
    }

    pub fn doc(&mut self) -> &mut Document {
        self.doc
    }

    pub fn mark_dirty(&mut self, area: IRect) {
        self.dirty.push(area);
    }

    pub fn color(&self, button: ToolButton) -> Rgba {
        match button {
            ToolButton::Primary => self.primary,
            ToolButton::Secondary => self.secondary,
        }
    }
}

pub trait ToolGesture: Send {
    fn moved(&mut self, cx: &mut ToolContext, input: &PointerInput);

    fn released(self: Box<Self>, cx: &mut ToolContext, input: &PointerInput);
}

pub struct ToolDef {
    pub id: ToolId,
    pub begin: fn(&mut ToolContext, &PointerInput) -> Option<Box<dyn ToolGesture>>,
}

impl Registered for ToolDef {
    type Id = ToolId;

    fn id(&self) -> ToolId {
        self.id
    }
}

fn catalog() -> &'static Registry<ToolDef> {
    static CATALOG: OnceLock<Registry<ToolDef>> = OnceLock::new();
    CATALOG.get_or_init(Registry::default)
}

pub fn register_tool(def: &'static ToolDef) -> Result<(), DuplicateId<ToolId>> {
    catalog().register(def)
}

pub fn lookup_tool(id: ToolId) -> Option<&'static ToolDef> {
    catalog().lookup(id)
}

pub fn all_tools() -> Vec<ToolId> {
    catalog().ids()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_ID: ToolId = ToolId::new("test.fill_on_release");
    const WHITE: Rgba = [255, 255, 255, 255];
    const RED: Rgba = [200, 0, 0, 255];

    struct FillOnRelease {
        button: ToolButton,
    }

    impl ToolGesture for FillOnRelease {
        fn moved(&mut self, _: &mut ToolContext, _: &PointerInput) {}

        fn released(self: Box<Self>, cx: &mut ToolContext, _: &PointerInput) {
            let color = cx.color(self.button);
            cx.doc().select_all();
            let area = cx.doc().fill_selection(color);
            cx.mark_dirty(area);
        }
    }

    fn begin(_: &mut ToolContext, input: &PointerInput) -> Option<Box<dyn ToolGesture>> {
        Some(Box::new(FillOnRelease { button: input.button }))
    }

    static FILL_TOOL: ToolDef = ToolDef { id: TEST_ID, begin };

    #[test]
    fn registered_tool_edits_document_and_reports_dirty_area() {
        assert_eq!(register_tool(&FILL_TOOL), Ok(()));
        assert_eq!(register_tool(&FILL_TOOL), Err(DuplicateId(TEST_ID)));
        assert!(all_tools().contains(&TEST_ID));
        let mut doc = Document::new(4, 4, WHITE, "layer".to_owned()).expect("документ создан");
        let mut dirty = Vec::new();
        let mut cx = ToolContext::new(&mut doc, &mut dirty, RED, WHITE);
        let input = PointerInput { position: FPoint::new(1.0, 1.0), button: ToolButton::Primary, shift: false, ctrl: false, alt: false, pressure: 1.0 };
        let gesture = (lookup_tool(TEST_ID).expect("зарегистрирован").begin)(&mut cx, &input).expect("жест начат");
        gesture.released(&mut cx, &input);
        assert_eq!(dirty.len(), 1);
        assert_eq!(doc.active_raster().get(2, 2), RED);
    }
}
