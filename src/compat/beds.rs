use super::assets::{AssetReport, save};
use anyhow::{Result, ensure};
use image::{Rgba, RgbaImage, imageops};
use std::collections::BTreeMap;
use std::path::Path;

fn resize_region(image: &RgbaImage, x: u32, y: u32, w: u32, h: u32, scale: u32) -> RgbaImage {
    imageops::crop_imm(image, x * scale, y * scale, w * scale, h * scale).to_image()
}

pub(super) fn migrate(
    root: &Path,
    target: u32,
    version: &str,
    report: &mut AssetReport,
) -> Result<()> {
    if target < 3 || version.starts_with("1.11") {
        return Ok(());
    }
    let textures = root.join("assets/minecraft/textures");
    let output = textures.join("entity/bed/red.png");
    if output.is_file() {
        return Ok(());
    }
    let mut parts = BTreeMap::new();
    for name in [
        "bed_head_top",
        "bed_feet_top",
        "bed_head_side",
        "bed_feet_side",
        "bed_head_end",
        "bed_feet_end",
    ] {
        let alternate = format!("{}_bed", name.strip_prefix("bed_").unwrap());
        let directory = if target < 4 { "blocks" } else { "block" };
        let path = [
            textures.join(format!("{directory}/{name}.png")),
            textures.join(format!("{directory}/{alternate}.png")),
        ]
        .into_iter()
        .find(|path| path.is_file());
        let Some(path) = path else {
            continue;
        };
        if path.is_file() {
            parts.insert(name, image::open(path)?.to_rgba8());
        }
    }
    if parts.is_empty() {
        return Ok(());
    }
    if !parts.contains_key("bed_head_top") || !parts.contains_key("bed_feet_top") {
        report
            .warnings
            .push("旧床纹理：缺少床头或床尾顶面，保留原图并使用目标原版床渲染".into());
        return Ok(());
    }
    let scale = parts["bed_head_top"].width() / 16;
    ensure!(
        scale > 0
            && parts
                .values()
                .all(|image| image.width() == 16 * scale && image.height() >= 16 * scale),
        "旧床纹理比例不一致"
    );
    let wood = textures.join(if target < 4 {
        "blocks/planks_oak.png"
    } else {
        "block/oak_planks.png"
    });
    let wood = if wood.is_file() {
        image::open(wood)?.to_rgba8()
    } else {
        RgbaImage::from_fn(16 * scale, 16 * scale, |x, y| {
            Rgba([142 + ((x / scale + y / scale) % 4) as u8 * 3, 110, 65, 255])
        })
    };
    let wood = imageops::resize(&wood, 16 * scale, 16 * scale, imageops::FilterType::Nearest);
    let mut atlas = RgbaImage::new(64 * scale, 64 * scale);
    for (prefix, y) in [("head", 0), ("feet", 22)] {
        let top_name = format!("bed_{prefix}_top");
        let top = resize_region(&parts[top_name.as_str()], 0, 0, 16, 16, scale);
        let top = imageops::rotate270(&top);
        imageops::replace(
            &mut atlas,
            &top,
            6 * i64::from(scale),
            i64::from((y + 6) * scale),
        );
        imageops::replace(
            &mut atlas,
            &wood,
            28 * i64::from(scale),
            i64::from((y + 6) * scale),
        );
        let side_name = format!("bed_{prefix}_side");
        let end_name = format!("bed_{prefix}_end");
        let side = parts
            .get(side_name.as_str())
            .map(|image| resize_region(image, 0, 7, 16, 6, scale))
            .unwrap_or_else(|| {
                imageops::resize(&wood, 16 * scale, 6 * scale, imageops::FilterType::Nearest)
            });
        let end = parts
            .get(end_name.as_str())
            .map(|image| resize_region(image, 0, 7, 16, 6, scale))
            .unwrap_or_else(|| side.clone());
        let side = imageops::rotate90(&side);
        imageops::replace(&mut atlas, &side, 0, i64::from((y + 6) * scale));
        imageops::replace(
            &mut atlas,
            &imageops::flip_horizontal(&side),
            22 * i64::from(scale),
            i64::from((y + 6) * scale),
        );
        imageops::replace(&mut atlas, &end, 6 * i64::from(scale), i64::from(y * scale));
        imageops::replace(
            &mut atlas,
            &end,
            22 * i64::from(scale),
            i64::from(y * scale),
        );
    }
    for y in [0, 6, 12, 18] {
        let leg = imageops::resize(&wood, 12 * scale, 6 * scale, imageops::FilterType::Nearest);
        imageops::replace(
            &mut atlas,
            &leg,
            50 * i64::from(scale),
            i64::from(y * scale),
        );
    }
    save(root, &output, &atlas)?;
    report
        .converted
        .push("旧床资源：从上传的床头、床尾与侧面重建红床实体图集，适配 1.12+ 的床渲染".into());
    Ok(())
}
