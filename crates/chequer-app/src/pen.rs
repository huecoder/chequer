use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use chequer_core::draw::Tilt;
use eframe::egui::{Context, Event, TouchDeviceId, TouchPhase};

#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "windows")]
mod windows;

const FULL_PRESSURE: f32 = 1.0;
pub const CURVE_RANGE: (f32, f32) = (0.5, 3.0);
pub const DEAD_ZONE_MAX: f32 = 0.5;
pub const SMOOTHING_MAX: f32 = 0.95;
pub const MIN_PRESSURE_MAX: f32 = 0.9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenResponse {
    pub curve: f32,
    pub dead_zone: f32,
    pub smoothing: f32,
    pub min_pressure: f32,
}

impl Default for PenResponse {
    fn default() -> Self {
        Self { curve: 1.6, dead_zone: 0.03, smoothing: 0.5, min_pressure: 0.05 }
    }
}

impl PenResponse {
    pub fn clamped(self) -> Self {
        Self {
            curve: self.curve.clamp(CURVE_RANGE.0, CURVE_RANGE.1),
            dead_zone: self.dead_zone.clamp(0.0, DEAD_ZONE_MAX),
            smoothing: self.smoothing.clamp(0.0, SMOOTHING_MAX),
            min_pressure: self.min_pressure.clamp(0.0, MIN_PRESSURE_MAX),
        }
    }

    fn shape(self, raw: f32) -> f32 {
        let outside_dead_zone = ((raw - self.dead_zone) / (FULL_PRESSURE - self.dead_zone)).clamp(0.0, FULL_PRESSURE);
        self.min_pressure + (FULL_PRESSURE - self.min_pressure) * outside_dead_zone.powf(self.curve)
    }
}

#[derive(Default)]
struct PenShared {
    pressure_bits: AtomicU32,
    tilt_x_bits: AtomicU32,
    tilt_y_bits: AtomicU32,
    down: AtomicBool,
    barrel: AtomicBool,
    enabled: AtomicBool,
}

pub struct Pen {
    shared: Arc<PenShared>,
    native_handle: Option<usize>,
    started: bool,
    touch_force: Option<f32>,
    response: PenResponse,
    filtered: f32,
    was_active: bool,
}

impl Pen {
    pub fn new(native_handle: Option<usize>) -> Self {
        Self {
            shared: Arc::default(),
            native_handle,
            started: false,
            touch_force: None,
            response: PenResponse::default(),
            filtered: FULL_PRESSURE,
            was_active: false,
        }
    }

    pub fn update(&mut self, ctx: &Context, wayland_tablet: bool, response: PenResponse, counts_device: impl Fn(TouchDeviceId) -> bool) {
        self.response = response.clamped();
        let tablet_backend = wayland_tablet || cfg!(target_os = "windows");
        self.shared.enabled.store(tablet_backend, Ordering::Relaxed);
        if tablet_backend && !self.started {
            self.started = true;
            self.start(ctx);
        }
        let touch_force = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Touch { device_id, phase, force, .. } if counts_device(*device_id) => Some((*phase, *force)),
                    _ => None,
                })
                .last()
        });
        match touch_force {
            Some((TouchPhase::End | TouchPhase::Cancel, _)) => self.touch_force = None,
            Some((_, force)) => self.touch_force = force.or(self.touch_force),
            None => {}
        }
        let active = self.tablet_down() || self.touch_force.is_some();
        let lifted = self.was_active && !active;
        if !lifted {
            let raw = self.raw_pressure();
            self.filtered = if active && self.was_active { self.filtered + (raw - self.filtered) * (FULL_PRESSURE - self.response.smoothing) } else { raw };
        }
        self.was_active = active;
    }

    fn tablet_down(&self) -> bool {
        self.shared.enabled.load(Ordering::Relaxed) && self.shared.down.load(Ordering::Relaxed)
    }

    fn raw_pressure(&self) -> f32 {
        if self.tablet_down() {
            return f32::from_bits(self.shared.pressure_bits.load(Ordering::Relaxed)).clamp(0.0, FULL_PRESSURE);
        }
        self.touch_force
            .unwrap_or(FULL_PRESSURE)
            .clamp(0.0, FULL_PRESSURE)
    }

    pub fn tilt(&self) -> Option<Tilt> {
        self.tablet_down().then(|| Tilt {
            x: f32::from_bits(self.shared.tilt_x_bits.load(Ordering::Relaxed)),
            y: f32::from_bits(self.shared.tilt_y_bits.load(Ordering::Relaxed)),
        })
    }

    pub fn barrel_pressed(&self) -> bool {
        self.tablet_down() && self.shared.barrel.load(Ordering::Relaxed) || crate::pen_feed::barrel_pressed()
    }

    pub fn shape_pressure(&self, raw: f32) -> f32 {
        self.response.shape(raw.clamp(0.0, FULL_PRESSURE))
    }

    pub fn pressure(&self) -> f32 {
        self.response.shape(self.filtered)
    }

    #[cfg(target_os = "linux")]
    fn start(&self, ctx: &Context) {
        if let Some(display) = self.native_handle {
            wayland::spawn(display, self.shared.clone(), ctx.clone());
        }
    }

    #[cfg(target_os = "windows")]
    fn start(&self, _ctx: &Context) {
        if let Some(window) = self.native_handle {
            windows::install(window, self.shared.clone());
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    fn start(&self, _ctx: &Context) {
        let _ = (&self.shared, &self.native_handle);
    }
}

#[cfg(target_os = "windows")]
pub fn native_handle(cc: &eframe::CreationContext<'_>) -> Option<usize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as usize),
        _ => None,
    }
}

#[cfg(not(target_os = "windows"))]
pub fn native_handle(cc: &eframe::CreationContext<'_>) -> Option<usize> {
    crate::trackpad::display_address(cc)
}
