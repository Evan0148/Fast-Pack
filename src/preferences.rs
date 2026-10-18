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

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Preferences {
    pub overlay: fast_pack::overlay::Draft,
    pub dark: bool,
    pub fishing_line: Option<[u8; 3]>,
    pub block_outline: Option<[u8; 3]>,
    pub crosshair_tint: Option<[u8; 3]>,
    pub fishing_line_color: [u8; 3],
    pub block_outline_color: [u8; 3],
    pub line_widths: fast_pack::compat::line_colors::Widths,
    pub fishing_line_width: f32,
    pub block_outline_width: f32,
    pub rainbow: fast_pack::compat::rainbow::Settings,
    pub builtin_overlays: fast_pack::builtin_overlays::Settings,
    pub auto_save: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_version: Option<String>,
    pub auto_save_path: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export_directory: Option<PathBuf>,
    #[serde(skip)]
    pub custom_glint: bool,
    pub glint_color: [u8; 3],
    pub heypixel_compat: bool,
    #[serde(default)]
    pub heypixel_settings_version: u8,
    pub crosshair_selection: Option<String>,
    pub crosshair_drawing: bool,
    pub crosshair_canvas: fast_pack::crosshair::Canvas,
    pub crosshair_pen_color: [u8; 3],
    pub crosshair_name: String,
    pub show_attack_indicator: bool,
    pub custom_item_rules: Vec<fast_pack::compat::custom_items::Rule>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            overlay: Default::default(),
            dark: true,
            fishing_line: None,
            block_outline: None,
            crosshair_tint: None,
            fishing_line_color: [0; 3],
            block_outline_color: [0; 3],
            line_widths: Default::default(),
            fishing_line_width: fast_pack::compat::line_colors::DEFAULT_WIDTH,
            block_outline_width: fast_pack::compat::line_colors::DEFAULT_WIDTH,
            rainbow: Default::default(),
            builtin_overlays: Default::default(),
            auto_save: true,
            target_version: None,
            auto_save_path: true,
            export_directory: None,
            custom_glint: false,
            glint_color: [255, 255, 255],
            heypixel_compat: fast_pack::client_audio::ENABLED,
            heypixel_settings_version: 1,
            crosshair_selection: None,
            crosshair_drawing: false,
            crosshair_canvas: fast_pack::crosshair::Canvas::default(),
            crosshair_pen_color: [255; 3],
            crosshair_name: "自绘准星".into(),
            show_attack_indicator: true,
            custom_item_rules: Vec::new(),
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

pub fn crosshair_store() -> Result<fast_pack::crosshair::Store> {
    Ok(fast_pack::crosshair::Store::new(
        config_path()?.parent().unwrap().join("crosshairs"),
    ))
}

fn supported(version: &str) -> bool {
    fast_pack::tables()
        .pack_formats
        .iter()
        .any(|(label, _)| label == version)
}

fn restore_heypixel(preferences: &mut Preferences) {
    if fast_pack::client_audio::ENABLED && preferences.heypixel_settings_version == 0 {
        preferences.heypixel_compat = true;
        preferences.heypixel_settings_version = 1;
    }
    preferences.heypixel_compat &= fast_pack::client_audio::ENABLED;
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
    restore_heypixel(&mut preferences);
    preferences.line_widths = preferences.line_widths.sanitized();
    for width in [
        &mut preferences.fishing_line_width,
        &mut preferences.block_outline_width,
    ] {
        if !width.is_finite()
            || !(fast_pack::compat::line_colors::MIN_WIDTH
                ..=fast_pack::compat::line_colors::MAX_WIDTH)
                .contains(width)
        {
            *width = fast_pack::compat::line_colors::DEFAULT_WIDTH;
        }
    }
    preferences.export_directory = preferences
        .export_directory
        .filter(|path| path.is_absolute());
    preferences.crosshair_canvas = fast_pack::crosshair::Canvas::from_image(
        &preferences
            .crosshair_canvas
            .render(&preferences.crosshair_name)
            .image,
        preferences.crosshair_canvas.size,
    );
    preferences
}

pub fn save(preferences: &Preferences) -> Result<()> {
    ensure!(
        preferences.target_version.as_deref().is_none_or(supported),
        "目标版本无效"
    );
    ensure!(
        preferences
            .export_directory
            .as_ref()
            .is_none_or(|path| path.is_absolute()),
        "导出目录必须是完整路径"
    );
    let path = config_path()?;
    let directory = path.parent().unwrap();
    fs::create_dir_all(directory).context("无法创建用户配置目录")?;
    let mut temporary = tempfile::Builder::new()
        .prefix("settings-")
        .suffix(".tmp")
        .tempfile_in(directory)
        .context("无法保存偏好设置")?;
    temporary.write_all(&serde_json::to_vec_pretty(preferences)?)?;
    temporary.as_file().sync_all()?;
    temporary.persist(&path).context("无法更新偏好设置")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_appearance_option_does_not_reset_saved_preferences() {
        use fast_pack::builtin_overlays::Feature;
        let restored: Preferences = serde_json::from_value(serde_json::json!({
            "dark":false,
            "target_version":"1.20.1",
            "export_directory":"D:\\My Packs",
            "block_outline_color":[12,34,56],
            "fishing_line_width":3.5,
            "heypixel_compat":true,
            "heypixel_settings_version":1,
            "builtin_overlays":{"features":["sharpness","heypixel-pack","highlight-names"]}
        }))
        .unwrap();
        assert!(!restored.dark);
        assert_eq!(restored.target_version.as_deref(), Some("1.20.1"));
        assert_eq!(
            restored.export_directory,
            Some(PathBuf::from(r"D:\My Packs"))
        );
        assert_eq!(restored.block_outline_color, [12, 34, 56]);
        assert_eq!(restored.fishing_line_width, 3.5);
        assert!(restored.heypixel_compat);
        assert_eq!(restored.heypixel_settings_version, 1);
        assert_eq!(
            restored.builtin_overlays.features,
            vec![Feature::Sharpness, Feature::HighlightNames]
        );
        assert!(
            !serde_json::to_string(&restored)
                .unwrap()
                .contains("heypixel-pack")
        );
    }

    #[test]
    fn line_widths_restore_enabled_options_and_remember_disabled_values() {
        let old: Preferences = serde_json::from_str("{}").unwrap();
        assert!(!old.line_widths.active());
        assert_eq!(old.fishing_line_width, 2.0);
        let saved = Preferences {
            line_widths: fast_pack::compat::line_colors::Widths {
                fishing: Some(2.7),
                outline: None,
            },
            fishing_line_width: 2.7,
            block_outline_width: 4.2,
            ..Default::default()
        };
        let restored: Preferences =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(restored.line_widths, saved.line_widths);
        assert_eq!(restored.fishing_line_width, 2.7);
        assert_eq!(restored.block_outline_width, 4.2);
        assert!(restored.fishing_line.is_none() && restored.block_outline.is_none());
    }

    #[test]
    fn restored_compatibility_defaults_on_once_then_preserves_the_user_choice() {
        let mut old: Preferences = serde_json::from_str(r#"{"heypixel_compat":false}"#).unwrap();
        assert_eq!(old.heypixel_settings_version, 0);
        restore_heypixel(&mut old);
        assert!(old.heypixel_compat);
        old.heypixel_compat = false;
        let mut saved: Preferences =
            serde_json::from_slice(&serde_json::to_vec(&old).unwrap()).unwrap();
        restore_heypixel(&mut saved);
        assert!(!saved.heypixel_compat);
        assert_eq!(saved.heypixel_settings_version, 1);
    }
}
