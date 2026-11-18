use std::sync::Arc;
use std::sync::atomic::Ordering;

use eframe::egui::Context;
use wayland_backend::client::Backend;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use wayland_protocols::wp::tablet::zv2::client::{
    zwp_tablet_manager_v2::ZwpTabletManagerV2,
    zwp_tablet_pad_group_v2::{self, ZwpTabletPadGroupV2},
    zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
    zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2,
    zwp_tablet_pad_v2::{self, ZwpTabletPadV2},
    zwp_tablet_seat_v2::{self, ZwpTabletSeatV2},
    zwp_tablet_tool_v2::{self, ZwpTabletToolV2},
    zwp_tablet_v2::ZwpTabletV2,
};

use super::PenShared;

const SEAT_VERSION_RANGE: std::ops::RangeInclusive<u32> = 1..=7;
const TABLET_VERSION_RANGE: std::ops::RangeInclusive<u32> = 1..=1;
const MAX_RAW_PRESSURE: f32 = 65535.0;

struct DisplayAddress(usize);

unsafe impl Send for DisplayAddress {}

struct State {
    shared: Arc<PenShared>,
    ctx: Context,
}

pub fn spawn(display: usize, shared: Arc<PenShared>, ctx: Context) {
    let display = DisplayAddress(display);
    let spawned = std::thread::Builder::new()
        .name("pen-wayland".to_owned())
        .spawn(move || {
            if let Err(reason) = run(display, State { shared, ctx }) {
                eprintln!("chequer: нажим пера через Wayland недоступен ({reason})");
            }
        });
    if let Err(e) = spawned {
        eprintln!("chequer: поток нажима пера не запустился ({e})");
    }
}

fn run(display: DisplayAddress, mut state: State) -> Result<(), String> {
    let backend = unsafe { Backend::from_foreign_display(display.0 as *mut _) };
    let connection = Connection::from_backend(backend);
    let (globals, mut queue) =
        registry_queue_init::<State>(&connection).map_err(|e| e.to_string())?;
    let handle = queue.handle();
    let manager: ZwpTabletManagerV2 = globals
        .bind(&handle, TABLET_VERSION_RANGE, ())
        .map_err(|_| "композитор не знает tablet-v2".to_owned())?;
    let seat: wl_seat::WlSeat = globals
        .bind(&handle, SEAT_VERSION_RANGE, ())
        .map_err(|e| e.to_string())?;
    let _tablet_seat = manager.get_tablet_seat(&seat, &handle, ());
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

impl Dispatch<ZwpTabletManagerV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletManagerV2,
        _: <ZwpTabletManagerV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletSeatV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletSeatV2,
        _: zwp_tablet_seat_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(State, ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ()),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
    ]);
}

impl Dispatch<ZwpTabletV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletV2,
        _: <ZwpTabletV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletPadV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadV2,
        _: zwp_tablet_pad_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(State, ZwpTabletPadV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadGroupV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadGroupV2,
        _: zwp_tablet_pad_group_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(State, ZwpTabletPadGroupV2, [
        zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (ZwpTabletPadRingV2, ()),
        zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadRingV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadRingV2,
        _: <ZwpTabletPadRingV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletPadStripV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadStripV2,
        _: <ZwpTabletPadStripV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletToolV2, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpTabletToolV2,
        event: zwp_tablet_tool_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_tablet_tool_v2::Event::Pressure { pressure } => {
                let normalized = pressure as f32 / MAX_RAW_PRESSURE;
                state
                    .shared
                    .pressure_bits
                    .store(normalized.to_bits(), Ordering::Relaxed);
                state.ctx.request_repaint();
            }
            zwp_tablet_tool_v2::Event::Tilt { tilt_x, tilt_y } => {
                state.shared.tilt_x_bits.store((tilt_x as f32).to_bits(), Ordering::Relaxed);
                state.shared.tilt_y_bits.store((tilt_y as f32).to_bits(), Ordering::Relaxed);
            }
            zwp_tablet_tool_v2::Event::Down { .. } => {
                state.shared.down.store(true, Ordering::Relaxed)
            }
            zwp_tablet_tool_v2::Event::Up | zwp_tablet_tool_v2::Event::ProximityOut => {
                state.shared.down.store(false, Ordering::Relaxed)
            }
            _ => {}
        }
    }
}
