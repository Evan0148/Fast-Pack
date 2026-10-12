use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{
    FOLDERID_LocalAppData, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath,
};

#[derive(Deserialize, Serialize)]
struct Preferences {
    target_version: String,
}

fn config_path() -> Result<PathBuf> {
    let native = unsafe {
        let mut path = std::ptr::null_mut();
        let status = SHGetKnownFolderPath(
            &FOLDERID_LocalAppData,
            KF_FLAG_DONT_VERIFY as u32,
            std::ptr::null_mut(),
            &mut path,
        );
        let folder = if status >= 0 && !path.is_null() {
            let mut length = 0;
            while *path.add(length) != 0 {
                length += 1;
            }
            Some(PathBuf::from(OsString::from_wide(
                std::slice::from_raw_parts(path, length),
            )))
        } else {
            None
        };
        CoTaskMemFree(path.cast());
        folder
    };
    let folder = native
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        })
        .context("无法找到用户配置目录")?;
    Ok(folder.join("Fast Pack").join("settings.json"))
}

fn supported(version: &str) -> bool {
    fast_pack::tables()
        .pack_formats
        .iter()
        .any(|(label, _)| label == version)
}

pub fn load_target() -> Option<String> {
    let data = fs::read(config_path().ok()?).ok()?;
    let preferences = serde_json::from_slice::<Preferences>(&data).ok()?;
    supported(&preferences.target_version).then_some(preferences.target_version)
}

pub fn save_target(version: &str) -> Result<()> {
    ensure!(supported(version), "目标版本无效");
    let path = config_path()?;
    let directory = path.parent().unwrap();
    fs::create_dir_all(directory).context("无法创建用户配置目录")?;
    let mut temporary = tempfile::Builder::new()
        .prefix("settings-")
        .suffix(".tmp")
        .tempfile_in(directory)
        .context("无法保存目标版本")?;
    temporary.write_all(&serde_json::to_vec_pretty(&Preferences {
        target_version: version.into(),
    })?)?;
    temporary.as_file().sync_all()?;
    temporary.persist(&path).context("无法更新目标版本设置")?;
    Ok(())
}
