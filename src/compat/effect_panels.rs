use super::assets::{AssetReport, save};
use anyhow::Result;
use image::{Rgba, RgbaImage, imageops};
use std::path::Path;

fn visible(image: &RgbaImage) -> bool {
    image.pixels().any(|pixel| pixel[3] > 8)
}

fn fallback(scale: u32) -> RgbaImage {
    RgbaImage::from_fn(120 * scale, 32 * scale, |x, y| {
        let x = x / scale;
        let y = y / scale;
        if (x == 0 || x == 119) && (y == 0 || y == 31) {
            Rgba([0, 0, 0, 0])
        } else if x == 0 || x == 119 || y == 0 || y == 31 {
            Rgba([0, 0, 0, 255])
        } else if x == 1 || x == 118 || y == 1 || y == 30 {
            Rgba([85, 85, 85, 255])
        } else {
            Rgba([33, 33, 33, 255])
        }
    })
}

pub(super) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    if source < 2 || target >= 2 {
        return Ok(());
    }
    let textures = root.join("assets/minecraft/textures");
    let path = textures.join("gui/container/inventory.png");
    if !path.is_file() {
        return Ok(());
    }
    let mut atlas = image::open(&path)?.to_rgba8();
    let scale = atlas.width() / 256;
    if scale == 0 || !atlas.width().is_multiple_of(256) || atlas.height() < 198 * scale {
        return Ok(());
    }
    let mut panel = imageops::crop_imm(&atlas, 0, 166 * scale, 120 * scale, 32 * scale).to_image();
    let mut restored = false;
    if !visible(&panel) {
        for candidate in [
            textures.join("gui/sprites/container/inventory/effect_background_large.png"),
            root.join(
                "fastpack/original-assets/assets/minecraft/textures/gui/container/inventory.png",
            ),
        ] {
            if !candidate.is_file() {
                continue;
            }
            let image = image::open(candidate)?.to_rgba8();
            let image = if image.width().is_multiple_of(256)
                && image.height() >= 198 * (image.width() / 256)
            {
                let factor = image.width() / 256;
                imageops::crop_imm(&image, 0, 166 * factor, 120 * factor, 32 * factor).to_image()
            } else {
                image
            };
            if visible(&image) {
                panel = imageops::resize(
                    &image,
                    120 * scale,
                    32 * scale,
                    imageops::FilterType::Nearest,
                );
                break;
            }
        }
        if !visible(&panel) {
            panel = fallback(scale);
        }
        restored = true;
    }
    for pixel in panel.pixels_mut() {
        if pixel[3] == 0 {
            *pixel = Rgba([0, 0, 0, 0]);
        }
    }
    let old = atlas.clone();
    imageops::replace(&mut atlas, &panel, 0, 166 * i64::from(scale));
    for y in 166 * scale..198 * scale {
        for x in 120 * scale..140 * scale {
            atlas.put_pixel(x, y, Rgba([0, 0, 0, 0]));
        }
    }
    if atlas != old {
        save(root, &path, &atlas)?;
        report.converted.push(if restored {
            "药水效果背景：自动恢复缺失面板，适配旧版 140px 取样并清理透明区域的白色 RGB".into()
        } else {
            "药水效果背景：保留原面板图案，清理旧版 140px 取样范围内透明像素的白色 RGB".into()
        });
    }
    Ok(())
}
