use std::sync::Mutex;
use std::time::{Duration, Instant};

use chequer_core::draw::Tilt;

const LIVE_TIMEOUT: Duration = Duration::from_millis(200);
const MIN_TILT_RADIANS: f32 = 0.01;
const PHASE_DOWN: i32 = 0;
const PHASE_UP: i32 = 2;
const BARREL_PRESSED: i32 = 1;
const HOVER_ACTIVE: i32 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PenPhase {
    Down,
    Move,
    Up,
}

impl PenPhase {
    fn from_code(code: i32) -> Self {
        match code {
            PHASE_DOWN => Self::Down,
            PHASE_UP => Self::Up,
            _ => Self::Move,
        }
    }
}

#[derive(Clone, Copy)]
pub struct PenSample {
    pub phase: PenPhase,
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub tilt: Option<Tilt>,
    pub time_ms: f64,
}

#[derive(Default)]
struct Feed {
    samples: Vec<PenSample>,
    down: bool,
    last_at: Option<Instant>,
    barrel: bool,
    hover: Option<(f32, f32)>,
}

static FEED: Mutex<Feed> = Mutex::new(Feed { samples: Vec::new(), down: false, last_at: None, barrel: false, hover: None });

fn feed() -> std::sync::MutexGuard<'static, Feed> {
    FEED.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn tilt_from(tilt_radians: f32, orientation_radians: f32) -> Option<Tilt> {
    (tilt_radians > MIN_TILT_RADIANS).then(|| {
        let degrees = tilt_radians.to_degrees();
        Tilt { x: degrees * orientation_radians.sin(), y: -degrees * orientation_radians.cos() }
    })
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn push(phase: PenPhase, x: f32, y: f32, pressure: f32, tilt: Option<Tilt>, time_ms: f64, buttons: i32) {
    let mut feed = feed();
    feed.samples.push(PenSample { phase, x, y, pressure, tilt, time_ms });
    feed.down = phase != PenPhase::Up;
    feed.barrel = phase != PenPhase::Up && buttons & BARREL_PRESSED != 0;
    if phase == PenPhase::Down {
        feed.hover = None;
    }
    feed.last_at = Some(Instant::now());
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn set_hover(active: i32, x: f32, y: f32, buttons: i32) {
    {
        let mut feed = feed();
        feed.hover = (active == HOVER_ACTIVE).then_some((x, y));
        feed.barrel = active == HOVER_ACTIVE && buttons & BARREL_PRESSED != 0;
    }
    crate::device::request_repaint();
}

pub fn barrel_pressed() -> bool {
    feed().barrel
}

pub fn hover_point() -> Option<(f32, f32)> {
    feed().hover
}

pub fn is_live() -> bool {
    let feed = feed();
    feed.down && feed.last_at.is_some_and(|at| at.elapsed() < LIVE_TIMEOUT)
}

pub fn drain(stroking: bool) -> Vec<PenSample> {
    let mut feed = feed();
    if stroking {
        return std::mem::take(&mut feed.samples);
    }
    match feed.samples.iter().rposition(|sample| sample.phase == PenPhase::Down) {
        Some(last_down) => {
            feed.samples.drain(..last_down);
        }
        None => feed.samples.clear(),
    }
    if feed.samples.last().is_some_and(|sample| sample.phase == PenPhase::Up) {
        feed.samples.clear();
    }
    Vec::new()
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_huecoder_chequer_MainActivity_nativePenSample(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    phase: i32,
    x: f32,
    y: f32,
    pressure: f32,
    tilt: f32,
    orientation: f32,
    age_ms: f32,
    buttons: i32,
) {
    push(PenPhase::from_code(phase), x, y, pressure, tilt_from(tilt, orientation), crate::input_clock::now_ms() - f64::from(age_ms), buttons);
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_huecoder_chequer_MainActivity_nativePenHover(_env: *mut std::ffi::c_void, _class: *mut std::ffi::c_void, active: i32, x: f32, y: f32, buttons: i32) {
    set_hover(active, x, y, buttons);
}
