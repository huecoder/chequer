
use std::path::Path;

#[cfg(not(target_os = "android"))]
pub use rfd::FileDialog;

#[cfg(target_os = "android")]
pub use android::FileDialog;

#[cfg(not(target_os = "android"))]
mod parent {
    use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle};
    use std::cell::Cell;

    #[derive(Clone, Copy)]
    pub struct ParentWindow {
        window: RawWindowHandle,
        display: RawDisplayHandle,
    }

    impl ParentWindow {
        pub fn of(frame: &eframe::Frame) -> Option<Self> {
            Some(Self { window: frame.window_handle().ok()?.as_raw(), display: frame.display_handle().ok()?.as_raw() })
        }
    }

    impl HasWindowHandle for ParentWindow {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            Ok(unsafe { WindowHandle::borrow_raw(self.window) })
        }
    }

    impl HasDisplayHandle for ParentWindow {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            Ok(unsafe { DisplayHandle::borrow_raw(self.display) })
        }
    }

    thread_local! {
        pub static CURRENT: Cell<Option<ParentWindow>> = const { Cell::new(None) };
    }
}

#[cfg(not(target_os = "android"))]
pub fn remember_window(frame: &eframe::Frame) {
    parent::CURRENT.set(parent::ParentWindow::of(frame));
}

#[cfg(target_os = "android")]
pub fn remember_window(_frame: &eframe::Frame) {}

#[cfg(not(target_os = "android"))]
pub fn new_dialog() -> FileDialog {
    match parent::CURRENT.get() {
        Some(window) => FileDialog::new().set_parent(&window),
        None => FileDialog::new(),
    }
}

#[cfg(target_os = "android")]
pub fn new_dialog() -> FileDialog {
    FileDialog::new()
}

pub fn stylus_device_ids() -> Vec<i32> {
    #[cfg(target_os = "android")]
    return android::stylus_device_ids();
    #[cfg(not(target_os = "android"))]
    Vec::new()
}

pub fn sync_back(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return android::sync_back(path);
    #[cfg(not(target_os = "android"))]
    {
        let _ = path;
        Ok(())
    }
}

pub fn share_image(raster: &chequer_core::raster::Raster, name: &str) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return android::share_image(raster, name);
    #[cfg(not(target_os = "android"))]
    {
        let _ = (raster, name);
        Err("доступно только на Android".to_owned())
    }
}

pub fn take_failure() -> Option<String> {
    #[cfg(target_os = "android")]
    return android::take_failure();
    #[cfg(not(target_os = "android"))]
    None
}

pub fn watch_view_requests(on_opened: impl Fn(Vec<(std::path::PathBuf, bool)>) + Send + 'static) {
    #[cfg(target_os = "android")]
    android::watch_view_requests(on_opened);
    #[cfg(not(target_os = "android"))]
    let _ = on_opened;
}

#[cfg(target_os = "android")]
mod android {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};
    use std::time::Duration;

    use crate::bridge::{request, request_within};

    const ANY_MIME: &str = "*/*";
    const IMAGE_MIME: &str = "image/*";
    const IMAGE_FILTER_LABEL: &str = "Images";
    const UNAVAILABLE_REPLY: &str = "UNAVAILABLE";
    const SYSTEM_PICKER_MISSING: &str = "Системный выбор файлов не открылся";
    const ERROR_REPLY_PREFIX: &str = "ERR";
    const SHARE_DIR: &str = "share";
    const SHARE_EXTENSION: &str = "png";
    const SYNC_OK: &str = "OK";
    const SYNC_UNMAPPED: &str = "NONE";
    const VIEW_POLL: &str = "VIEW_POLL";
    const VIEW_POLL_INTERVAL: Duration = Duration::from_millis(500);
    const VIEW_POLL_TIMEOUT: Duration = Duration::from_millis(200);
    const WRITABLE_MARK: &str = "1";
    const STYLUS_QUERY: &str = "STYLUS";
    const STYLUS_QUERY_TIMEOUT: Duration = Duration::from_millis(500);

    static FAILURE: Mutex<Option<String>> = Mutex::new(None);

    pub fn take_failure() -> Option<String> {
        FAILURE.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    fn report_failure(message: String) {
        *FAILURE.lock().unwrap_or_else(PoisonError::into_inner) = Some(message);
    }

    fn ask_system(command: &str) -> Vec<PathBuf> {
        match request(command) {
            Err(message) => report_failure(message),
            Ok(lines) => match lines.first() {
                Some(line) if line == UNAVAILABLE_REPLY => report_failure(SYSTEM_PICKER_MISSING.to_owned()),
                Some(line) if line.starts_with(ERROR_REPLY_PREFIX) => report_failure(line.clone()),
                _ => return paths(Ok(lines)),
            },
        }
        Vec::new()
    }

    fn paths(reply: Result<Vec<String>, String>) -> Vec<PathBuf> {
        reply.unwrap_or_default().iter().filter_map(|l| l.split_once('\t')).map(|(_, path)| PathBuf::from(path)).collect()
    }

    pub fn watch_view_requests(on_opened: impl Fn(Vec<(PathBuf, bool)>) + Send + 'static) {
        std::thread::spawn(move || {
            loop {
                let reply = request_within(VIEW_POLL, Some(VIEW_POLL_TIMEOUT)).unwrap_or_default();
                let opened: Vec<(PathBuf, bool)> = reply.iter().filter_map(|line| line.split_once('\t')).map(|(path, writable)| (PathBuf::from(path), writable == WRITABLE_MARK)).collect();
                if !opened.is_empty() {
                    on_opened(opened);
                }
                std::thread::sleep(VIEW_POLL_INTERVAL);
            }
        });
    }

    pub fn stylus_device_ids() -> Vec<i32> {
        let reply = request_within(STYLUS_QUERY, Some(STYLUS_QUERY_TIMEOUT)).unwrap_or_default();
        reply.iter().filter_map(|line| line.parse().ok()).collect()
    }

    pub fn share_image(raster: &chequer_core::raster::Raster, name: &str) -> Result<(), String> {
        let dir = crate::bridge::dir().ok_or("мост к системным службам не настроен")?.join(SHARE_DIR);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{name}.{SHARE_EXTENSION}"));
        let options = chequer_core::io::SaveOptions::for_extension(SHARE_EXTENSION).ok_or("формат для отправки не найден")?;
        chequer_core::io::save_raster(&path, raster, &options).map_err(|e| e.to_string())?;
        match request(&format!("SHARE\t{}", path.display()))?.first().map(String::as_str) {
            Some(reply) if reply.starts_with(ERROR_REPLY_PREFIX) => Err(reply.to_owned()),
            _ => Ok(()),
        }
    }

    pub fn sync_back(path: &Path) -> Result<(), String> {
        let reply = request(&format!("SYNC\t{}", path.display()))?;
        match reply.first().map(String::as_str) {
            Some(SYNC_OK | SYNC_UNMAPPED) => Ok(()),
            Some(other) => Err(other.to_owned()),
            None => Err("мост не ответил".to_owned()),
        }
    }

    fn mime_for_name(name: &str) -> &'static str {
        match Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("png") => "image/png",
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("bmp") => "image/bmp",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            Some("tif" | "tiff") => "image/tiff",
            _ => "application/octet-stream",
        }
    }

    pub struct FileDialog {
        labels: Vec<String>,
        file_name: String,
    }

    impl FileDialog {
        pub fn new() -> Self {
            Self { labels: Vec::new(), file_name: String::new() }
        }

        pub fn add_filter(mut self, label: &str, _extensions: &[impl AsRef<str>]) -> Self {
            self.labels.push(label.to_owned());
            self
        }

        pub fn set_file_name(mut self, name: impl Into<String>) -> Self {
            self.file_name = name.into();
            self
        }

        fn open_mime(&self) -> &'static str {
            if !self.labels.is_empty() && self.labels.iter().all(|l| l == IMAGE_FILTER_LABEL) { IMAGE_MIME } else { ANY_MIME }
        }

        pub fn pick_file(self) -> Option<PathBuf> {
            self.open(false).and_then(|files| files.into_iter().next())
        }

        pub fn pick_files(self) -> Option<Vec<PathBuf>> {
            self.open(true)
        }

        fn open(self, multiple: bool) -> Option<Vec<PathBuf>> {
            Some(ask_system(&format!("OPEN\t{}\t{}", self.open_mime(), if multiple { "1" } else { "0" }))).filter(|files| !files.is_empty())
        }

        pub fn save_file(self) -> Option<PathBuf> {
            ask_system(&format!("SAVE\t{}\t{}", mime_for_name(&self.file_name), self.file_name)).into_iter().next()
        }
    }
}
