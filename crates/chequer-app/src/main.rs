#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    if let Err(error) = chequer_app::run_desktop() {
        chequer_app::show_startup_error(&error);
        std::process::exit(1);
    }
}
