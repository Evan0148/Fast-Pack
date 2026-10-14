use super::assets::{AssetReport, open, save};
use anyhow::Result;
use image::{Rgba, RgbaImage, imageops};
use std::fs;
use std::path::Path;

const REGIONS: [(&str, u32, u32, u32, u32); 5] = [
    ("crosshair_attack_indicator_background", 36, 94, 16, 4),
    ("crosshair_attack_indicator_progress", 52, 94, 16, 4),
    ("crosshair_attack_indicator_full", 68, 94, 16, 16),
    ("hotbar_attack_indicator_background", 0, 94, 18, 18),
    ("hotbar_attack_indicator_progress", 18, 94, 18, 18),
];

fn visible(image: &RgbaImage) -> bool {
    image
        .pixels()
        .any(|pixel| pixel[3] > 0 && pixel[0].max(pixel[1]).max(pixel[2]) > 0)
}

pub(crate) fn apply(root: &Path, target: u32, show: bool, report: &mut AssetReport) -> Result<()> {
    if target < 2 {
        return Ok(());
    }
    let icons = root.join("assets/minecraft/textures/gui/icons.png");
    let native =
        image::load_from_memory(include_bytes!("../../assets/crosshair/legacy-icons.png"))?
            .to_rgba8();
    let mut atlas = if target < 18 {
        Some(if icons.is_file() {
            open(&icons)?
        } else {
            native.clone()
        })
    } else {
        None
    };
    let scale = if let Some(atlas) = &atlas {
        if !atlas.width().is_multiple_of(256) || atlas.height() < 112 * (atlas.width() / 256) {
            report
                .warnings
                .push("攻击冷却指示：HUD 图集尺寸非常规，保持原图以保留其他图标".into());
            return Ok(());
        }
        (atlas.width() / 256).max(1)
    } else {
        1
    };
    let originals = root.join("fastpack/original-attack-indicator");
    let mut changed = false;
    for (name, x, y, width, height) in REGIONS {
        let path = root.join(format!(
            "assets/minecraft/textures/gui/sprites/hud/{name}.png"
        ));
        let original = originals.join(format!("{name}.png"));
        let current = if let Some(atlas) = &atlas {
            Some(
                imageops::crop_imm(atlas, x * scale, y * scale, width * scale, height * scale)
                    .to_image(),
            )
        } else if path.is_file() {
            Some(open(&path)?)
        } else {
            None
        };
        let tile = if show {
            if let Some(current) = current.as_ref().filter(|image| visible(image)) {
                current.clone()
            } else {
                let restored = if original.is_file() {
                    Some(open(&original)?)
                } else {
                    None
                };
                restored
                    .filter(visible)
                    .unwrap_or_else(|| imageops::crop_imm(&native, x, y, width, height).to_image())
            }
        } else {
            if let Some(current) = &current
                && visible(current)
                && !original.is_file()
            {
                fs::create_dir_all(&originals)?;
                current.save(&original)?;
            }
            let size = current
                .as_ref()
                .map(|image| (image.width(), image.height()))
                .unwrap_or((width, height));
            RgbaImage::from_pixel(size.0, size.1, Rgba([0; 4]))
        };
        if let Some(atlas) = &mut atlas {
            let tile = imageops::resize(
                &tile,
                width * scale,
                height * scale,
                imageops::FilterType::Nearest,
            );
            if current.as_ref() != Some(&tile) {
                changed = true;
            }
            imageops::replace(atlas, &tile, i64::from(x * scale), i64::from(y * scale));
        } else if current.as_ref() != Some(&tile) {
            save(root, &path, &tile)?;
            let metadata = path.with_extension("png.mcmeta");
            if metadata.is_file() {
                let original_metadata = original.with_extension("png.mcmeta");
                if !original_metadata.is_file() {
                    fs::create_dir_all(&originals)?;
                    fs::copy(&metadata, &original_metadata)?;
                }
                super::assets::backup(root, &metadata)?;
                fs::remove_file(&metadata)?;
            }
            if show {
                let original_metadata = original.with_extension("png.mcmeta");
                if original_metadata.is_file() {
                    fs::copy(original_metadata, &metadata)?;
                }
            }
            changed = true;
        }
    }
    if changed {
        if let Some(atlas) = atlas {
            save(root, &icons, &atlas)?;
        }
        report.converted.push(if show {
            "攻击冷却指示：恢复准星与快捷栏指示资源，显示位置沿用游戏设置".into()
        } else {
            "攻击冷却指示：透明化准星与快捷栏指示区域，原始资源已保留".into()
        });
    }
    Ok(())
}
