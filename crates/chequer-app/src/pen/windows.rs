use std::sync::Arc;
use std::sync::atomic::Ordering;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::Pointer::{GetPointerPenInfo, GetPointerType, POINTER_FLAG_INCONTACT, POINTER_FLAG_SECONDBUTTON, POINTER_PEN_INFO};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{PEN_FLAG_BARREL, PEN_MASK_PRESSURE, PEN_MASK_TILT_X, PEN_MASK_TILT_Y, PT_PEN, WM_NCDESTROY, WM_POINTERDOWN, WM_POINTERLEAVE, WM_POINTERUP, WM_POINTERUPDATE};

use super::PenShared;

const SUBCLASS_ID: usize = 1;
const MAX_RAW_PRESSURE: f32 = 1024.0;
const POINTER_ID_MASK: usize = 0xFFFF;

pub fn install(window: usize, shared: Arc<PenShared>) {
    let refdata = Arc::into_raw(shared) as usize;
    let installed = unsafe { SetWindowSubclass(window as HWND, Some(subclass_proc), SUBCLASS_ID, refdata) };
    if installed == 0 {
        drop(unsafe { Arc::from_raw(refdata as *const PenShared) });
        eprintln!("chequer: перо Windows Ink недоступно");
    }
}

unsafe extern "system" fn subclass_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, _subclass_id: usize, refdata: usize) -> LRESULT {
    let shared = unsafe { &*(refdata as *const PenShared) };
    match message {
        WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP => read_pen(shared, (wparam & POINTER_ID_MASK) as u32),
        WM_POINTERLEAVE => release(shared),
        WM_NCDESTROY => {
            unsafe { RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID) };
            drop(unsafe { Arc::from_raw(refdata as *const PenShared) });
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

fn read_pen(shared: &PenShared, pointer_id: u32) {
    let mut kind = 0;
    if unsafe { GetPointerType(pointer_id, &mut kind) } == 0 || kind != PT_PEN {
        return;
    }
    let mut info: POINTER_PEN_INFO = unsafe { std::mem::zeroed() };
    if unsafe { GetPointerPenInfo(pointer_id, &mut info) } == 0 {
        return;
    }
    let pointer_flags = info.pointerInfo.pointerFlags;
    let pressure = if info.penMask & PEN_MASK_PRESSURE != 0 { info.pressure as f32 / MAX_RAW_PRESSURE } else { 1.0 };
    let tilt_x = if info.penMask & PEN_MASK_TILT_X != 0 { info.tiltX as f32 } else { 0.0 };
    let tilt_y = if info.penMask & PEN_MASK_TILT_Y != 0 { info.tiltY as f32 } else { 0.0 };
    shared.pressure_bits.store(pressure.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    shared.tilt_x_bits.store(tilt_x.to_bits(), Ordering::Relaxed);
    shared.tilt_y_bits.store(tilt_y.to_bits(), Ordering::Relaxed);
    shared.barrel.store(info.penFlags & PEN_FLAG_BARREL != 0 || pointer_flags & POINTER_FLAG_SECONDBUTTON != 0, Ordering::Relaxed);
    shared.down.store(pointer_flags & POINTER_FLAG_INCONTACT != 0, Ordering::Relaxed);
}

fn release(shared: &PenShared) {
    shared.down.store(false, Ordering::Relaxed);
    shared.barrel.store(false, Ordering::Relaxed);
}
