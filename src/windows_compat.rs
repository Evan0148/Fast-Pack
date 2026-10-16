use std::sync::OnceLock;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

#[repr(C)]
struct VersionInfo {
    size: u32,
    major: u32,
    minor: u32,
    build: u32,
    platform: u32,
    service_pack: [u16; 128],
}

fn legacy() -> bool {
    static LEGACY: OnceLock<bool> = OnceLock::new();
    *LEGACY.get_or_init(|| unsafe {
        let module = GetModuleHandleW(windows_sys::w!("ntdll.dll"));
        let Some(function) = GetProcAddress(module, c"RtlGetVersion".as_ptr().cast()) else {
            return true;
        };
        let get_version: unsafe extern "system" fn(*mut VersionInfo) -> i32 =
            std::mem::transmute(function);
        let mut info = VersionInfo {
            size: std::mem::size_of::<VersionInfo>() as u32,
            major: 0,
            minor: 0,
            build: 0,
            platform: 0,
            service_pack: [0; 128],
        };
        get_version(&mut info) < 0 || info.major < 10
    })
}

pub fn initialize() {
    if std::env::var_os("ICED_BACKEND").is_none() {
        unsafe {
            std::env::set_var("ICED_BACKEND", "tiny-skia");
        }
    }
}

pub fn font_family() -> &'static str {
    if legacy() {
        "Microsoft YaHei"
    } else {
        "Microsoft YaHei UI"
    }
}
