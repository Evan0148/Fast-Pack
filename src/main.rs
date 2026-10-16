#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod gui;
#[cfg(windows)]
mod motion;
#[cfg(windows)]
mod preferences;
#[cfg(windows)]
mod startup;
#[cfg(windows)]
mod windows_compat;

fn main() {
    #[cfg(windows)]
    startup::begin();
    let helper =
        std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--finish-heypixel"));
    #[cfg(windows)]
    if !helper {
        std::thread::spawn(|| {
            let _ = iced::advanced::graphics::text::font_system();
            startup::record("fonts_ready");
        });
    }
    let pending = if !helper && !fast_pack::client_audio::ENABLED {
        std::thread::spawn(|| {
            let _ = fast_pack::client_audio::finish_pending();
        });
        Ok(false)
    } else {
        fast_pack::client_audio::finish_pending()
    };
    match pending {
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
