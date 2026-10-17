//! Pig/cow atlases doubled in height in 1.21.5. Their pixels keep their
//! coordinates: resizing the image would move every face to the wrong UV.
use super::assets::{AssetReport, backup, open, save};
use crate::packfs::{self as fs, PathExt};
use anyhow::{Result, ensure};
use image::{RgbaImage, imageops};
use serde_json::Value;
use std::path::Path;

struct Family {
    legacy: &'static [&'static str],
    modern: &'static str,
    recent: &'static str,
    muzzle: bool,
}

const FAMILIES: &[Family] = &[
    Family {
        legacy: &["pig/pig"],
        modern: "pig/temperate_pig",
        recent: "pig/pig_temperate",
        muzzle: false,
    },
    Family {
        legacy: &["cow/cow"],
        modern: "cow/temperate_cow",
        recent: "cow/cow_temperate",
        muzzle: true,
    },
    Family {
        legacy: &["cow/mooshroom", "cow/red_mooshroom"],
        modern: "cow/red_mooshroom",
        recent: "cow/mooshroom_red",
        muzzle: true,
    },
    Family {
        legacy: &["cow/brown_mooshroom"],
        modern: "cow/brown_mooshroom",
        recent: "cow/mooshroom_brown",
        muzzle: true,
    },
    Family {
        legacy: &["pig/pig_saddle"],
        modern: "equipment/pig_saddle/saddle",
        recent: "equipment/pig_saddle/saddle",
        muzzle: false,
    },
];

impl Family {
    fn target(&self, format: u32) -> &'static str {
        if format >= 84 {
            self.recent
        } else if format >= 55 {
            self.modern
        } else if format >= 4 && self.legacy.len() > 1 {
            self.legacy[1]
        } else {
            self.legacy[0]
        }
    }
}

fn frame(image: &RgbaImage, modern: bool, muzzle: bool) -> RgbaImage {
    let scale = image.width() / 64;
    let mut output = RgbaImage::new(image.width(), if modern { 64 } else { 32 } * scale);
    imageops::replace(&mut output, image, 0, 0);
    if muzzle {
        // The new 6x3x1 muzzle occupies UV (1,33). Project the old head's
        // front-face patch (7,11)..(13,14) onto its six faces, using edge
        // pixels for the thin sides. Never insert unrelated vanilla colors.
        if modern {
            for y in 0..3 * scale {
                for x in 0..6 * scale {
                    let pixel = *image.get_pixel(7 * scale + x, 11 * scale + y);
                    output.put_pixel(2 * scale + x, 34 * scale + y, pixel);
                    output.put_pixel(9 * scale + x, 34 * scale + y, pixel);
                }
                for x in 0..scale {
                    output.put_pixel(
                        scale + x,
                        34 * scale + y,
                        *image.get_pixel(7 * scale, 11 * scale + y),
                    );
                    output.put_pixel(
                        8 * scale + x,
                        34 * scale + y,
                        *image.get_pixel(13 * scale - 1, 11 * scale + y),
                    );
                }
            }
            for y in 0..scale {
                for x in 0..6 * scale {
                    output.put_pixel(
                        2 * scale + x,
                        33 * scale + y,
                        *image.get_pixel(7 * scale + x, 11 * scale),
                    );
                    output.put_pixel(
                        8 * scale + x,
                        33 * scale + y,
                        *image.get_pixel(7 * scale + x, 14 * scale - 1),
                    );
                }
            }
        } else {
            let face =
                imageops::crop_imm(image, 2 * scale, 34 * scale, 6 * scale, 3 * scale).to_image();
            imageops::replace(
                &mut output,
                &face,
                i64::from(7 * scale),
                i64::from(11 * scale),
            );
        }
    }
    output
}

fn frame_height(width: u32, image_height: u32, metadata: &Option<Value>) -> Result<u32> {
    ensure!(
        width >= 64 && width.is_multiple_of(64),
        "动物图集宽度须为 64 的整数倍"
    );
    let animation = metadata.as_ref().and_then(|v| v.get("animation"));
    let explicit_height = animation
        .and_then(|v| v.get("height"))
        .and_then(Value::as_u64);
    let height = explicit_height.unwrap_or(u64::from(image_height));
    ensure!(
        height > 0
            && height <= u64::from(image_height)
            && u64::from(image_height).is_multiple_of(height),
        "动物动画帧高度无效"
    );
    let height = height as u32;
    ensure!(
        animation
            .and_then(|v| v.get("width"))
            .and_then(Value::as_u64)
            .is_none_or(|v| v == u64::from(width)),
        "动物动画包含不支持的横向帧布局"
    );
    ensure!(
        height == width / 2 || height == width,
        "动物图集须为 64×32 或 64×64 布局，按分辨率等比缩放"
    );
    Ok(height)
}

fn convert(
    image: &RgbaImage,
    metadata: &mut Option<Value>,
    modern: bool,
    muzzle: bool,
) -> Result<Option<RgbaImage>> {
    let width = image.width();
    let height = frame_height(width, image.height(), metadata)?;
    if height == if modern { width } else { width / 2 } {
        return Ok(None);
    }
    let target_height = if modern { width } else { width / 2 };
    let count = image.height() / height;
    let mut output = RgbaImage::new(width, target_height * count);
    for index in 0..count {
        let source = imageops::crop_imm(image, 0, index * height, width, height).to_image();
        imageops::replace(
            &mut output,
            &frame(&source, modern, muzzle),
            0,
            i64::from(index * target_height),
        );
    }
    if metadata
        .as_ref()
        .is_some_and(|v| v["animation"].get("height").is_some())
    {
        metadata.as_mut().unwrap()["animation"]["height"] = Value::from(target_height);
    }
    Ok(Some(output))
}

pub(super) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    let textures = root.join("assets/minecraft/textures/entity");
    for family in FAMILIES {
        let model = if family.legacy[0].contains("mooshroom") {
            "mooshroom"
        } else if family.legacy[0].starts_with("pig/") {
            "pig"
        } else {
            "cow"
        };
        if ["optifine", "mcpatcher"].iter().any(|directory| {
            root.join(format!("assets/minecraft/{directory}/cem/{model}.jem"))
                .pack_is_file()
        }) {
            // A CEM textureSize overrides the native atlas dimensions.
            continue;
        }
        for suffix in ["", "_e", "_n", "_s"] {
            let output = textures.join(format!("{}{suffix}.png", family.target(target)));
            // Existing target textures take priority in multi-version packs.
            let input = std::iter::once(family.target(target))
                .chain(std::iter::once(family.target(source)))
                .chain(family.legacy.iter().copied())
                .chain([family.modern, family.recent])
                .map(|name| textures.join(format!("{name}{suffix}.png")))
                .find(|path| path.pack_is_file());
            let Some(input) = input else { continue };
            let metadata_path = input.with_extension("png.mcmeta");
            let metadata_bytes = metadata_path
                .pack_is_file()
                .then(|| fs::read(&metadata_path))
                .transpose()?;
            let mut metadata = match metadata_bytes
                .as_deref()
                .map(serde_json::from_slice::<Value>)
                .transpose()
            {
                Ok(value) => value,
                Err(_) => {
                    report.warnings.push(format!(
                        "动物贴图 {}：动画元数据无效，保留原资源",
                        input.strip_prefix(root)?.display()
                    ));
                    continue;
                }
            };
            let (width, height) = fs::image_reader(&input)?
                .with_guessed_format()?
                .into_dimensions()?;
            let height = match frame_height(width, height, &metadata) {
                Ok(height) => height,
                Err(error) => {
                    report.warnings.push(format!(
                        "动物贴图 {}：{error}，保留原资源",
                        input.strip_prefix(root)?.display()
                    ));
                    continue;
                }
            };
            // Read PNG headers only when the atlas already has the target layout.
            let image = if height == if target >= 55 { width } else { width / 2 } {
                None
            } else {
                convert(&open(&input)?, &mut metadata, target >= 55, family.muzzle)?
            };
            if let Some(image) = image {
                save(root, &output, &image)?;
                if let Some(metadata) = metadata {
                    let path = output.with_extension("png.mcmeta");
                    if path.pack_is_file() {
                        backup(root, &path)?;
                    }
                    fs::write_atomic(&path, &serde_json::to_vec_pretty(&metadata)?)?;
                }
                report.converted.push(format!(
                    "动物 UV：{} → {}，{}；保留像素比例、透明度与动画{}",
                    input.strip_prefix(root)?.display(),
                    output.strip_prefix(root)?.display(),
                    if target >= 55 {
                        "扩展为 64×64 布局"
                    } else {
                        "还原为 64×32 布局"
                    },
                    if family.muzzle {
                        "，适配牛与蘑菇牛口鼻"
                    } else {
                        ""
                    }
                ));
            } else if input != output {
                fs::create_dir_all(output.parent().unwrap())?;
                fs::copy(&input, &output)?;
                if let Some(bytes) = metadata_bytes {
                    fs::write_atomic(&output.with_extension("png.mcmeta"), &bytes)?;
                }
                report.converted.push(format!(
                    "动物路径：{} → {}",
                    input.strip_prefix(root)?.display(),
                    output.strip_prefix(root)?.display()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "animals_tests.rs"]
mod tests;
