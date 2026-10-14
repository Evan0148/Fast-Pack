#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod gui;
#[cfg(windows)]
mod motion;
#[cfg(windows)]
mod preferences;
#[cfg(windows)]
mod windows_compat;

fn main() {
    match fast_pack::client_audio::finish_pending() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("Fast Pack: {error:#}");
            std::process::exit(1);
        }
    }
    #[cfg(windows)]
    windows_compat::initialize();
    #[cfg(windows)]
    if let Err(error) = gui::run() {
        gui::show_error(&format!("{error:#}"));
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    if let Err(error) = fast_pack::cli::run() {
        eprintln!("Fast Pack: {error:#}");
        std::process::exit(1);
    }
}
