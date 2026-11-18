#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Haptic {
    Tick,
    Click,
    LongPress,
    Reject,
}

static REPAINT: std::sync::OnceLock<eframe::egui::Context> = std::sync::OnceLock::new();
static VOLUME_KEYS: std::sync::Mutex<Vec<VolumeKey>> = std::sync::Mutex::new(Vec::new());

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeKey {
    Up,
    Down,
}

pub fn set_repaint(ctx: eframe::egui::Context) {
    let _ = REPAINT.set(ctx);
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn request_repaint() {
    if let Some(ctx) = REPAINT.get() {
        ctx.request_repaint();
    }
}

pub fn take_volume_keys() -> Vec<VolumeKey> {
    std::mem::take(&mut *VOLUME_KEYS.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

static HAPTICS_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_haptics_enabled(enabled: bool) {
    HAPTICS_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

pub const AUTO_FOCUS_TEXT: bool = !cfg!(target_os = "android");

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeEvent {
    Text(String),
    Backspace,
    Enter,
    Hide,
}

#[cfg(target_os = "android")]
mod android {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;
    use std::sync::mpsc::{Sender, channel};
    use std::time::Duration;

    use super::{Haptic, ImeEvent, VolumeKey};

    const VIBRATE_COMMAND: &str = "VIBRATE";
    const KEYBOARD_COMMAND: &str = "KEYBOARD";
    const IME_POLL_COMMAND: &str = "IME_POLL";
    const IME_POLL_TIMEOUT: Duration = Duration::from_millis(50);
    const IME_TEXT: &str = "TEXT";
    const IME_KEY: &str = "KEY";
    const KEY_BACKSPACE: &str = "BACKSPACE";
    const KEY_ENTER: &str = "ENTER";
    const KEY_HIDE: &str = "HIDE";
    const COMMAND_TIMEOUT: Duration = Duration::from_millis(500);
    const TICK_MILLIS: u32 = 12;
    const CLICK_MILLIS: u32 = 20;
    const LONG_PRESS_MILLIS: u32 = 35;
    const REJECT_MILLIS: u32 = 60;

    static COMMANDS: OnceLock<Sender<String>> = OnceLock::new();
    static VOLUME_KEYS_CAPTURED: AtomicBool = AtomicBool::new(false);

    const BACK_EXIT_COMMAND: &str = "BACK_EXIT";
    const VOLUME_KEYS_COMMAND: &str = "VOLUME_KEYS";
    const KEY_UP: i32 = 1;

    pub fn move_to_back() {
        send(BACK_EXIT_COMMAND.to_owned());
    }

    pub fn capture_volume_keys(captured: bool) {
        if VOLUME_KEYS_CAPTURED.swap(captured, Ordering::Relaxed) != captured {
            send(format!("{VOLUME_KEYS_COMMAND}\t{}", u8::from(captured)));
        }
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_huecoder_chequer_MainActivity_nativeVolumeKey(_env: *mut std::ffi::c_void, _class: *mut std::ffi::c_void, up: i32) {
        let key = if up == KEY_UP { VolumeKey::Up } else { VolumeKey::Down };
        super::VOLUME_KEYS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(key);
        super::request_repaint();
    }

    fn millis(haptic: Haptic) -> u32 {
        match haptic {
            Haptic::Tick => TICK_MILLIS,
            Haptic::Click => CLICK_MILLIS,
            Haptic::LongPress => LONG_PRESS_MILLIS,
            Haptic::Reject => REJECT_MILLIS,
        }
    }

    fn send(line: String) {
        let sender = COMMANDS.get_or_init(|| {
            let (sender, receiver) = channel::<String>();
            std::thread::spawn(move || {
                for line in receiver {
                    let _ = crate::bridge::request_within(&line, Some(COMMAND_TIMEOUT));
                }
            });
            sender
        });
        let _ = sender.send(line);
    }

    pub fn vibrate(haptic: Haptic) {
        send(format!("{VIBRATE_COMMAND}\t{}", millis(haptic)));
    }

    pub fn set_keyboard(visible: bool) {
        send(format!("{KEYBOARD_COMMAND}\t{}", u8::from(visible)));
    }

    fn decode_hex(hex: &str) -> Option<String> {
        let bytes: Option<Vec<u8>> = (0..hex.len()).step_by(2).map(|at| hex.get(at..at + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok())).collect();
        String::from_utf8(bytes?).ok()
    }

    fn parse_ime_event(line: &str) -> Option<ImeEvent> {
        match line.split_once('\t')? {
            (IME_TEXT, hex) => decode_hex(hex).map(ImeEvent::Text),
            (IME_KEY, KEY_BACKSPACE) => Some(ImeEvent::Backspace),
            (IME_KEY, KEY_ENTER) => Some(ImeEvent::Enter),
            (IME_KEY, KEY_HIDE) => Some(ImeEvent::Hide),
            _ => None,
        }
    }

    pub fn take_ime_events() -> Vec<ImeEvent> {
        let reply = crate::bridge::request_within(IME_POLL_COMMAND, Some(IME_POLL_TIMEOUT)).unwrap_or_default();
        reply.iter().filter_map(|line| parse_ime_event(line)).collect()
    }
}

pub fn vibrate(haptic: Haptic) {
    #[cfg(target_os = "android")]
    if HAPTICS_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        android::vibrate(haptic);
    }
    #[cfg(not(target_os = "android"))]
    let _ = haptic;
}

pub fn set_keyboard(visible: bool) {
    #[cfg(target_os = "android")]
    android::set_keyboard(visible);
    #[cfg(not(target_os = "android"))]
    let _ = visible;
}

pub fn take_ime_events() -> Vec<ImeEvent> {
    #[cfg(target_os = "android")]
    return android::take_ime_events();
    #[cfg(not(target_os = "android"))]
    Vec::new()
}

pub fn move_to_back() {
    #[cfg(target_os = "android")]
    android::move_to_back();
}

pub fn capture_volume_keys(captured: bool) {
    #[cfg(target_os = "android")]
    android::capture_volume_keys(captured);
    #[cfg(not(target_os = "android"))]
    let _ = captured;
}
