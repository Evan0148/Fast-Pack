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
#[serde(default)]
pub struct Preferences {
    pub auto_save: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_version: Option<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            auto_save: true,
            target_version: None,
        }
    }
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

pub fn load() -> Preferences {
    let mut preferences = config_path()
        .ok()
        .and_then(|path| fs::read(path).ok())
        .and_then(|data| serde_json::from_slice::<Preferences>(&data).ok())
        .unwrap_or_default();
    preferences.target_version = preferences
        .target_version
        .filter(|version| supported(version));
    preferences
}

pub fn save(auto_save: bool, version: Option<&str>) -> Result<()> {
    ensure!(version.is_none_or(supported), "目标版本无效");
    let path = config_path()?;
    let directory = path.parent().unwrap();
    fs::create_dir_all(directory).context("无法创建用户配置目录")?;
    let mut temporary = tempfile::Builder::new()
        .prefix("settings-")
        .suffix(".tmp")
        .tempfile_in(directory)
        .context("无法保存目标版本")?;
    temporary.write_all(&serde_json::to_vec_pretty(&Preferences {
        auto_save,
        target_version: version.map(str::to_owned),
    })?)?;
    temporary.as_file().sync_all()?;
    temporary.persist(&path).context("无法更新目标版本设置")?;
    Ok(())
}
