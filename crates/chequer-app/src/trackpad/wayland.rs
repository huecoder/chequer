use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use eframe::egui::{Context, vec2};
use wayland_backend::client::Backend;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::pointer_gestures::zv1::client::{
    zwp_pointer_gesture_pinch_v1::{self, ZwpPointerGesturePinchV1},
    zwp_pointer_gesture_swipe_v1::{self, ZwpPointerGestureSwipeV1},
    zwp_pointer_gestures_v1::ZwpPointerGesturesV1,
};

use super::{GestureEvent, SwipeDirection};

const SWIPE_MIN_DISTANCE: f64 = 60.0;
const SEAT_VERSION_RANGE: std::ops::RangeInclusive<u32> = 1..=7;
const GESTURES_VERSION_RANGE: std::ops::RangeInclusive<u32> = 1..=3;

struct DisplayAddress(usize);

unsafe impl Send for DisplayAddress {}

struct State {
    sender: Sender<GestureEvent>,
    enabled: Arc<AtomicBool>,
    ctx: Context,
    pinch_scale: f64,
    pinch_rotation: f64,
    swipe_total: (f64, f64),
    swipe_fingers: u32,
}

impl State {
    fn send(&self, event: GestureEvent) {
        if self.enabled.load(Ordering::Relaxed) {
            let _ = self.sender.send(event);
            self.ctx.request_repaint();
        }
    }
}

pub fn spawn(display: usize, sender: Sender<GestureEvent>, enabled: Arc<AtomicBool>, ctx: Context) {
    let display = DisplayAddress(display);
    let spawned = std::thread::Builder::new()
        .name("trackpad-wayland".to_owned())
        .spawn(move || {
            if let Err(reason) = run(
                display,
                State {
                    sender,
                    enabled,
                    ctx,
                    pinch_scale: 1.0,
                    pinch_rotation: 0.0,
                    swipe_total: (0.0, 0.0),
                    swipe_fingers: 0,
                },
            ) {
                eprintln!("chequer: жесты Wayland недоступны ({reason}), остаются системные");
            }
        });
    if let Err(e) = spawned {
        eprintln!("chequer: поток жестов не запустился ({e}), остаются системные");
    }
}

fn run(display: DisplayAddress, mut state: State) -> Result<(), String> {
    let backend = unsafe { Backend::from_foreign_display(display.0 as *mut _) };
    let connection = Connection::from_backend(backend);
    let (globals, mut queue) =
        registry_queue_init::<State>(&connection).map_err(|e| e.to_string())?;
    let handle = queue.handle();
    let gestures: ZwpPointerGesturesV1 = globals
        .bind(&handle, GESTURES_VERSION_RANGE, ())
        .map_err(|_| "композитор не знает pointer-gestures".to_owned())?;
    let seat: wl_seat::WlSeat = globals
        .bind(&handle, SEAT_VERSION_RANGE, ())
        .map_err(|e| e.to_string())?;
    let pointer = seat.get_pointer(&handle, ());
    let _pinch = gestures.get_pinch_gesture(&pointer, &handle, ());
    let _swipe = gestures.get_swipe_gesture(&pointer, &handle, ());
    loop {
        queue
            .blocking_dispatch(&mut state)
            .map_err(|e| e.to_string())?;
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_pointer::WlPointer,
        _: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpPointerGesturesV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpPointerGesturesV1,
        _: <ZwpPointerGesturesV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpPointerGesturePinchV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpPointerGesturePinchV1,
        event: zwp_pointer_gesture_pinch_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_pointer_gesture_pinch_v1::Event::Begin { .. } => {
                state.pinch_scale = 1.0;
                state.pinch_rotation = 0.0;
            }
            zwp_pointer_gesture_pinch_v1::Event::Update {
                dx,
                dy,
                scale,
                rotation,
                ..
            } => {
                if scale > 0.0 && state.pinch_scale > 0.0 {
                    state.send(GestureEvent::Pinch {
                        factor: (scale / state.pinch_scale) as f32,
                    });
                }
                state.send(GestureEvent::Rotate {
                    radians: (rotation - state.pinch_rotation).to_radians() as f32,
                });
                state.send(GestureEvent::Pan {
                    delta: vec2(dx as f32, dy as f32),
                });
                state.pinch_scale = scale;
                state.pinch_rotation = rotation;
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpPointerGestureSwipeV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpPointerGestureSwipeV1,
        event: zwp_pointer_gesture_swipe_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_pointer_gesture_swipe_v1::Event::Begin { fingers, .. } => {
                state.swipe_total = (0.0, 0.0);
                state.swipe_fingers = fingers;
            }
            zwp_pointer_gesture_swipe_v1::Event::Update { dx, dy, .. } => {
                state.swipe_total.0 += dx;
                state.swipe_total.1 += dy;
            }
            zwp_pointer_gesture_swipe_v1::Event::End { cancelled, .. } if cancelled == 0 => {
                if let Some(direction) = swipe_direction(state.swipe_total) {
                    state.send(GestureEvent::Swipe {
                        fingers: state.swipe_fingers,
                        direction,
                    });
                }
            }
            _ => {}
        }
    }
}

fn swipe_direction((dx, dy): (f64, f64)) -> Option<SwipeDirection> {
    if dx.abs().max(dy.abs()) < SWIPE_MIN_DISTANCE {
        return None;
    }
    Some(match (dx.abs() >= dy.abs(), dx > 0.0, dy > 0.0) {
        (true, true, _) => SwipeDirection::Right,
        (true, false, _) => SwipeDirection::Left,
        (false, _, true) => SwipeDirection::Down,
        (false, _, false) => SwipeDirection::Up,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_swipe_is_ignored_and_long_one_follows_the_dominant_axis() {
        assert_eq!(swipe_direction((10.0, 20.0)), None);
        assert_eq!(swipe_direction((-120.0, 30.0)), Some(SwipeDirection::Left));
        assert_eq!(swipe_direction((50.0, 200.0)), Some(SwipeDirection::Down));
        assert_eq!(swipe_direction((90.0, -10.0)), Some(SwipeDirection::Right));
    }
}
