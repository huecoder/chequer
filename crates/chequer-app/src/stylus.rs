use std::collections::HashSet;

use eframe::egui::epaint::util::hash;
use eframe::egui::{Event, Modifiers, PointerButton, TouchDeviceId, TouchPhase};

#[derive(Default)]
pub struct Stylus {
    pen_devices: HashSet<u64>,
    seen_devices: HashSet<u64>,
    pen_touches: HashSet<u64>,
    pen_seen: bool,
    pen_only: bool,
}

impl Stylus {
    pub fn detect() -> Self {
        let mut stylus = Self::default();
        stylus.reload_pen_devices();
        stylus
    }

    fn reload_pen_devices(&mut self) {
        self.pen_devices = crate::filepicker::stylus_device_ids().into_iter().map(hash).collect();
    }

    pub fn fingers_blocked(&self) -> bool {
        self.pen_seen && self.fingers_locked()
    }

    pub fn fingers_locked(&self) -> bool {
        self.pen_only
    }

    pub fn set_pen_only(&mut self, pen_only: bool) {
        self.pen_only = pen_only;
    }

    pub fn is_pen(&self, device: TouchDeviceId) -> bool {
        self.pen_devices.contains(&device.0)
    }

    pub fn draws(&self, device: TouchDeviceId) -> bool {
        !self.fingers_blocked() || self.is_pen(device)
    }

    pub fn pen_is_down(&self) -> bool {
        !self.pen_touches.is_empty()
    }

    pub fn observe(&mut self, events: &[Event]) {
        for event in events {
            let Event::Touch { device_id, id, phase, .. } = event else { continue };
            if self.seen_devices.insert(device_id.0) {
                self.reload_pen_devices();
            }
            if !self.is_pen(*device_id) {
                continue;
            }
            self.pen_seen = true;
            match phase {
                TouchPhase::Start | TouchPhase::Move => self.pen_touches.insert(id.0),
                TouchPhase::End | TouchPhase::Cancel => self.pen_touches.remove(&id.0),
            };
        }
    }

    pub fn fingers(&self, events: &[Event]) -> Vec<Event> {
        events.iter().filter(|event| !matches!(event, Event::Touch { device_id, .. } if self.is_pen(*device_id))).cloned().collect()
    }

    pub fn pointer_events(&self, events: Vec<Event>) -> Vec<Event> {
        if !self.fingers_blocked() {
            return events;
        }
        events.iter().flat_map(|event| self.pen_pointer_events(event)).collect()
    }

    fn pen_pointer_events(&self, event: &Event) -> Vec<Event> {
        let Event::Touch { device_id, phase, pos, .. } = event else { return Vec::new() };
        if !self.is_pen(*device_id) {
            return Vec::new();
        }
        let button = |pressed| Event::PointerButton { pos: *pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        match phase {
            TouchPhase::Start => vec![Event::PointerMoved(*pos), button(true)],
            TouchPhase::Move => vec![Event::PointerMoved(*pos)],
            TouchPhase::End | TouchPhase::Cancel => vec![button(false)],
        }
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{TouchId, pos2};

    use super::*;

    fn stylus_with_pen(device: u64) -> Stylus {
        Stylus { pen_devices: HashSet::from([device]), pen_seen: true, pen_only: true, ..Stylus::default() }
    }

    fn touch(device: u64, phase: TouchPhase) -> Event {
        Event::Touch { device_id: TouchDeviceId(device), id: TouchId(0), phase, pos: pos2(5.0, 5.0), force: None }
    }

    #[test]
    fn blocked_fingers_leave_only_pen_pointer_events() {
        let stylus = stylus_with_pen(7);
        let events = vec![touch(1, TouchPhase::Start), touch(7, TouchPhase::Start), touch(7, TouchPhase::End)];
        let pointer = stylus.pointer_events(events);
        assert_eq!(pointer.len(), 3);
        assert!(matches!(pointer[1], Event::PointerButton { pressed: true, .. }));
        assert!(matches!(pointer[2], Event::PointerButton { pressed: false, .. }));
    }

    #[test]
    fn allowing_fingers_passes_events_through() {
        let mut stylus = stylus_with_pen(7);
        stylus.set_pen_only(false);
        assert!(!stylus.fingers_blocked());
        assert_eq!(stylus.pointer_events(vec![touch(1, TouchPhase::Start)]).len(), 1);
    }

    #[test]
    fn fingers_stay_free_until_the_pen_touches_the_screen() {
        let mut stylus = Stylus { pen_devices: HashSet::from([7]), seen_devices: HashSet::from([7]), pen_only: true, ..Stylus::default() };
        assert!(!stylus.fingers_blocked());
        stylus.observe(&[touch(7, TouchPhase::Start)]);
        assert!(stylus.fingers_blocked());
    }

    #[test]
    fn without_a_pen_nothing_is_blocked() {
        let stylus = Stylus::default();
        assert!(!stylus.fingers_blocked());
        assert!(stylus.draws(TouchDeviceId(1)));
    }

    #[test]
    fn pen_events_are_hidden_from_finger_gestures() {
        let stylus = stylus_with_pen(7);
        let fingers = stylus.fingers(&[touch(1, TouchPhase::Start), touch(7, TouchPhase::Start)]);
        assert_eq!(fingers.len(), 1);
    }
}
