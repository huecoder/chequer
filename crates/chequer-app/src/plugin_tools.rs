use eframe::egui::{Modifiers, PointerButton};

use chequer_core::geom::FPoint;

use crate::app::App;
use crate::canvas::Gesture;
use crate::tool_registry::{PointerInput, ToolButton, ToolContext, ToolGesture, ToolId, lookup_tool};

fn tool_button(button: PointerButton) -> ToolButton {
    if button == PointerButton::Secondary { ToolButton::Secondary } else { ToolButton::Primary }
}

impl App {
    pub(crate) fn plugin_tool_pressed(&mut self, id: ToolId, position: FPoint, button: PointerButton, mods: Modifiers) {
        let Some(def) = lookup_tool(id) else {
            return;
        };
        let button = tool_button(button);
        let input = self.pointer_input(position, button, mods);
        if let Some(gesture) = self.with_tool_context(|cx| (def.begin)(cx, &input)) {
            self.tab_mut().gesture = Gesture::Plugin { gesture, button };
        }
    }

    pub(crate) fn plugin_tool_moved(&mut self, position: FPoint, mods: Modifiers) {
        let Gesture::Plugin { mut gesture, button } = std::mem::replace(&mut self.tab_mut().gesture, Gesture::None) else {
            return;
        };
        let input = self.pointer_input(position, button, mods);
        self.with_tool_context(|cx| gesture.moved(cx, &input));
        self.tab_mut().gesture = Gesture::Plugin { gesture, button };
    }

    pub(crate) fn plugin_tool_released(&mut self, gesture: Box<dyn ToolGesture>, button: ToolButton, position: FPoint, mods: Modifiers) {
        let input = self.pointer_input(position, button, mods);
        self.with_tool_context(|cx| gesture.released(cx, &input));
    }

    fn pointer_input(&self, position: FPoint, button: ToolButton, mods: Modifiers) -> PointerInput {
        PointerInput { position, button, shift: mods.shift, ctrl: mods.ctrl, alt: mods.alt, pressure: self.pen.pressure() }
    }

    fn with_tool_context<R>(&mut self, run: impl FnOnce(&mut ToolContext) -> R) -> R {
        let (primary, secondary) = (self.colors.primary, self.colors.secondary);
        let tab = self.tab_mut();
        let mut dirty = Vec::new();
        let result = run(&mut ToolContext::new(&mut tab.doc, &mut dirty, primary, secondary));
        for area in dirty {
            tab.mark_dirty(area);
        }
        result
    }
}
