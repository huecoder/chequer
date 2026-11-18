
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::Arc;

use eframe::egui::{Context, Vec2};

#[cfg(target_os = "linux")]
mod wayland;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrackpadMode {
    #[default]
    Auto,
    System,
}

impl TrackpadMode {
    pub fn to_setting(self) -> &'static str {
        match self {
            TrackpadMode::Auto => "auto",
            TrackpadMode::System => "system",
        }
    }

    pub fn from_setting(value: &str) -> Self {
        if value == "system" { TrackpadMode::System } else { TrackpadMode::Auto }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GestureEvent {
    Pinch { factor: f32 },
    Rotate { radians: f32 },
    Pan { delta: Vec2 },
    Swipe { fingers: u32, direction: SwipeDirection },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackpadSettings {
    pub mode: TrackpadMode,
    pub pinch_sensitivity: f32,
    pub rotate: bool,
    pub swipes: bool,
}

impl Default for TrackpadSettings {
    fn default() -> Self {
        Self { mode: TrackpadMode::Auto, pinch_sensitivity: 1.0, rotate: true, swipes: true }
    }
}

pub const PINCH_SENSITIVITY_MIN: f32 = 0.25;
pub const PINCH_SENSITIVITY_MAX: f32 = 3.0;

#[cfg(target_os = "linux")]
pub fn display_address(cc: &eframe::CreationContext<'_>) -> Option<usize> {
    use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
    match cc.display_handle().ok()?.as_raw() {
        RawDisplayHandle::Wayland(handle) => Some(handle.display.as_ptr() as usize),
        _ => None,
    }
}

#[cfg(not(target_os = "linux"))]
pub fn display_address(_cc: &eframe::CreationContext<'_>) -> Option<usize> {
    None
}

pub struct Trackpad {
    events: Receiver<GestureEvent>,
    sender: Sender<GestureEvent>,
    enabled: Arc<AtomicBool>,
    display: Option<usize>,
    started: bool,
}

impl Trackpad {
    pub fn new(display: Option<usize>) -> Self {
        let (sender, events) = channel();
        Self { events, sender, enabled: Arc::new(AtomicBool::new(false)), display, started: false }
    }

    pub fn set_mode(&mut self, mode: TrackpadMode, ctx: &Context) {
        let auto = mode == TrackpadMode::Auto;
        self.enabled.store(auto, Ordering::Relaxed);
        if auto && !self.started {
            self.started = true;
            self.start(ctx);
        }
    }

    #[cfg(target_os = "linux")]
    fn start(&self, ctx: &Context) {
        if let Some(display) = self.display {
            wayland::spawn(display, self.sender.clone(), self.enabled.clone(), ctx.clone());
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn start(&self, _ctx: &Context) {
        let _ = (&self.sender, &self.display);
    }

    pub fn drain(&self) -> Vec<GestureEvent> {
        self.events.try_iter().collect()
    }
}
