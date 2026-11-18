
use std::collections::HashMap;

use eframe::egui::{Event, Pos2, TouchPhase, Vec2};

const TAP_MAX_SECONDS: f64 = 0.35;
pub(crate) const TAP_MAX_TRAVEL: f32 = 6.0;
const MIN_SPREAD: f32 = 4.0;
const MAX_TAP_FINGERS: usize = 4;
const LONG_PRESS_SECONDS: f64 = 0.5;

struct Finger {
    start: Pos2,
    pos: Pos2,
}

struct Session {
    started: f64,
    max_fingers: usize,
    travel: f32,
    cancelled: bool,
    long_pressed: bool,
    picked_at: Option<Pos2>,
}

#[derive(Debug, PartialEq)]
pub enum TouchAction {
    AbortDrawing,
    Transform { anchor: Pos2, zoom: f32, pan: Vec2 },
    Tap { fingers: usize },
    LongPress { pos: Pos2 },
    LongPressDrag { pos: Pos2 },
}

#[derive(Default)]
pub struct Touches {
    fingers: HashMap<u64, Finger>,
    session: Option<Session>,
    last: Option<(usize, Pos2, f32)>,
    single_finger_pans: bool,
}

impl Touches {
    pub fn blocks_pointer(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.max_fingers >= 2 || s.long_pressed)
    }

    pub fn is_active(&self) -> bool {
        self.session.is_some()
    }

    pub fn set_single_finger_pans(&mut self, enabled: bool) {
        self.single_finger_pans = enabled;
    }

    pub fn is_holding(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.max_fingers == 1 && !s.long_pressed)
    }

    pub fn update(&mut self, events: &[Event], time: f64, starts_on_canvas: impl Fn(Pos2) -> bool) -> Vec<TouchAction> {
        let mut actions = Vec::new();
        for event in events {
            let Event::Touch { id, phase, pos, .. } = event else { continue };
            match phase {
                TouchPhase::Start => self.finger_down(id.0, *pos, time, &starts_on_canvas, &mut actions),
                TouchPhase::Move => self.finger_moved(id.0, *pos),
                TouchPhase::End => self.finger_up(id.0, time, false, &mut actions),
                TouchPhase::Cancel => self.finger_up(id.0, time, true, &mut actions),
            }
        }
        actions.extend(self.transform());
        actions.extend(self.long_press(time));
        actions.extend(self.long_press_drag());
        actions
    }

    fn finger_down(&mut self, id: u64, pos: Pos2, time: f64, starts_on_canvas: &impl Fn(Pos2) -> bool, actions: &mut Vec<TouchAction>) {
        if self.fingers.is_empty() {
            self.session = starts_on_canvas(pos).then_some(Session { started: time, max_fingers: 1, travel: 0.0, cancelled: false, long_pressed: false, picked_at: None });
        }
        self.fingers.insert(id, Finger { start: pos, pos });
        if let Some(session) = self.session.as_mut() {
            let was_single = session.max_fingers < 2;
            session.max_fingers = session.max_fingers.max(self.fingers.len());
            if was_single && session.max_fingers >= 2 && !self.single_finger_pans {
                actions.push(TouchAction::AbortDrawing);
            }
        }
    }

    fn finger_moved(&mut self, id: u64, pos: Pos2) {
        let Some(finger) = self.fingers.get_mut(&id) else { return };
        finger.pos = pos;
        if let Some(session) = self.session.as_mut() {
            session.travel = session.travel.max(finger.start.distance(pos));
        }
    }

    fn finger_up(&mut self, id: u64, time: f64, cancelled: bool, actions: &mut Vec<TouchAction>) {
        self.fingers.remove(&id);
        if let Some(session) = self.session.as_mut() {
            session.cancelled |= cancelled;
        }
        if !self.fingers.is_empty() {
            return;
        }
        self.last = None;
        let Some(session) = self.session.take() else { return };
        let quick = time - session.started < TAP_MAX_SECONDS && session.travel < TAP_MAX_TRAVEL;
        if quick && !session.cancelled && session.max_fingers >= 2 {
            actions.push(TouchAction::Tap { fingers: session.max_fingers.min(MAX_TAP_FINGERS) });
        }
    }

    fn long_press(&mut self, time: f64) -> Option<TouchAction> {
        let session = self.session.as_mut()?;
        let finger = self.fingers.values().next().filter(|_| self.fingers.len() == 1 && session.max_fingers == 1)?;
        let held = !session.long_pressed && session.travel < TAP_MAX_TRAVEL && time - session.started >= LONG_PRESS_SECONDS;
        held.then(|| {
            session.long_pressed = true;
            session.picked_at = Some(finger.pos);
            TouchAction::LongPress { pos: finger.pos }
        })
    }

    fn long_press_drag(&mut self) -> Option<TouchAction> {
        let session = self.session.as_mut().filter(|s| s.long_pressed && s.max_fingers == 1)?;
        let finger = self.fingers.values().next().filter(|f| session.picked_at != Some(f.pos))?;
        session.picked_at = Some(finger.pos);
        Some(TouchAction::LongPressDrag { pos: finger.pos })
    }

    fn transform(&mut self) -> Option<TouchAction> {
        let count = self.fingers.len();
        let moving = self.session.as_ref().is_some_and(|s| s.travel >= TAP_MAX_TRAVEL);
        if count < 2 && !(self.single_finger_pans && count == 1) {
            self.last = None;
            return None;
        }
        let center = self.fingers.values().fold(Vec2::ZERO, |sum, f| sum + f.pos.to_vec2()).to_pos2() / count as f32;
        let spread = self.fingers.values().map(|f| f.pos.distance(center)).sum::<f32>() / count as f32;
        let previous = self.last.replace((count, center, spread));
        let (previous_count, previous_center, previous_spread) = previous?;
        if previous_count != count || !moving {
            return None;
        }
        let zoom = if previous_spread >= MIN_SPREAD && spread >= MIN_SPREAD { spread / previous_spread } else { 1.0 };
        let pan = center - previous_center;
        (zoom != 1.0 || pan != Vec2::ZERO).then_some(TouchAction::Transform { anchor: previous_center, zoom, pan })
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{TouchDeviceId, TouchId, pos2};

    use super::*;

    fn touch(id: u64, phase: TouchPhase, x: f32, y: f32) -> Event {
        Event::Touch { device_id: TouchDeviceId(0), id: TouchId(id), phase, pos: pos2(x, y), force: None }
    }

    #[test]
    fn quick_two_finger_tap_is_a_tap_and_aborts_drawing() {
        let mut touches = Touches::default();
        let first = touches.update(&[touch(1, TouchPhase::Start, 10.0, 10.0), touch(2, TouchPhase::Start, 50.0, 10.0)], 0.0, |_| true);
        assert_eq!(first, vec![TouchAction::AbortDrawing]);
        let last = touches.update(&[touch(1, TouchPhase::End, 10.0, 10.0), touch(2, TouchPhase::End, 50.0, 10.0)], 0.1, |_| true);
        assert_eq!(last, vec![TouchAction::Tap { fingers: 2 }]);
    }

    #[test]
    fn spreading_fingers_zoom_instead_of_tapping() {
        let mut touches = Touches::default();
        touches.update(&[touch(1, TouchPhase::Start, 10.0, 10.0), touch(2, TouchPhase::Start, 50.0, 10.0)], 0.0, |_| true);
        let moved = touches.update(&[touch(1, TouchPhase::Move, -10.0, 10.0), touch(2, TouchPhase::Move, 70.0, 10.0)], 0.05, |_| true);
        assert!(matches!(moved.as_slice(), [TouchAction::Transform { zoom, .. }] if (*zoom - 2.0).abs() < 1e-4));
        let ended = touches.update(&[touch(1, TouchPhase::End, -10.0, 10.0), touch(2, TouchPhase::End, 70.0, 10.0)], 0.1, |_| true);
        assert!(ended.is_empty());
    }

    #[test]
    fn a_short_pinch_is_never_a_tap() {
        let mut touches = Touches::default();
        touches.update(&[touch(1, TouchPhase::Start, 10.0, 10.0), touch(2, TouchPhase::Start, 50.0, 10.0)], 0.0, |_| true);
        touches.update(&[touch(1, TouchPhase::Move, 3.0, 10.0), touch(2, TouchPhase::Move, 57.0, 10.0)], 0.05, |_| true);
        let ended = touches.update(&[touch(1, TouchPhase::End, 3.0, 10.0), touch(2, TouchPhase::End, 57.0, 10.0)], 0.1, |_| true);
        assert!(ended.is_empty());
    }

    #[test]
    fn single_finger_pans_only_when_enabled() {
        let drag = [touch(1, TouchPhase::Move, 30.0, 10.0), touch(1, TouchPhase::Move, 50.0, 10.0)];
        let mut touches = Touches::default();
        touches.update(&[touch(1, TouchPhase::Start, 10.0, 10.0)], 0.0, |_| true);
        assert!(touches.update(&drag, 0.1, |_| true).is_empty());

        let mut touches = Touches::default();
        touches.set_single_finger_pans(true);
        assert!(touches.update(&[touch(1, TouchPhase::Start, 10.0, 10.0)], 0.0, |_| true).is_empty());
        let moved = touches.update(&drag, 0.1, |_| true);
        assert!(matches!(moved.as_slice(), [TouchAction::Transform { pan, .. }] if pan.x > 0.0));
    }

    #[test]
    fn a_still_finger_long_presses_once() {
        let mut touches = Touches::default();
        touches.update(&[touch(1, TouchPhase::Start, 10.0, 10.0)], 0.0, |_| true);
        assert!(touches.update(&[], 0.2, |_| true).is_empty());
        assert_eq!(touches.update(&[], 0.6, |_| true), vec![TouchAction::LongPress { pos: pos2(10.0, 10.0) }]);
        assert!(touches.update(&[], 0.9, |_| true).is_empty());
        assert!(touches.blocks_pointer());
    }

    #[test]
    fn a_long_press_follows_the_finger_until_it_lifts() {
        let mut touches = Touches::default();
        touches.update(&[touch(1, TouchPhase::Start, 10.0, 10.0)], 0.0, |_| true);
        touches.update(&[], 0.6, |_| true);
        let moved = touches.update(&[touch(1, TouchPhase::Move, 40.0, 10.0)], 0.7, |_| true);
        assert_eq!(moved, vec![TouchAction::LongPressDrag { pos: pos2(40.0, 10.0) }]);
        assert!(touches.update(&[], 0.8, |_| true).is_empty());
        assert!(touches.update(&[touch(1, TouchPhase::End, 40.0, 10.0)], 0.9, |_| true).is_empty());
    }

    #[test]
    fn a_moving_finger_never_long_presses() {
        let mut touches = Touches::default();
        touches.update(&[touch(1, TouchPhase::Start, 10.0, 10.0)], 0.0, |_| true);
        touches.update(&[touch(1, TouchPhase::Move, 40.0, 10.0)], 0.1, |_| true);
        assert!(touches.update(&[], 0.8, |_| true).is_empty());
    }
}
