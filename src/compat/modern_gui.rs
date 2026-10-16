use super::assets::{AssetReport, save};
use crate::packfs::PathExt;
use anyhow::{Result, ensure};
use image::{Rgba, RgbaImage, imageops};
use std::collections::BTreeMap;
use std::path::Path;

const EFFECTS: [(&str, u32, u32); 20] = [
    ("speed", 0, 0),
    ("slowness", 1, 0),
    ("haste", 2, 0),
    ("mining_fatigue", 3, 0),
    ("strength", 4, 0),
    ("weakness", 5, 0),
    ("poison", 6, 0),
    ("regeneration", 7, 0),
    ("invisibility", 0, 1),
    ("hunger", 1, 1),
    ("jump_boost", 2, 1),
    ("nausea", 3, 1),
    ("night_vision", 4, 1),
    ("blindness", 5, 1),
    ("resistance", 6, 1),
    ("fire_resistance", 7, 1),
    ("water_breathing", 0, 2),
    ("wither", 1, 2),
    ("health_boost", 2, 2),
    ("absorption", 2, 2),
];

fn normalized(image: RgbaImage) -> RgbaImage {
    if image.width() > 0 && image.width() < 256 {
        imageops::resize(
            &image,
            256,
            (image.height() * 256 / image.width()).max(1),
            imageops::FilterType::Nearest,
        )
    } else {
        image
    }
}

pub(super) fn prepare(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    if source >= 4 || target < 4 {
        return Ok(());
    }
    let textures = root.join("assets/minecraft/textures");
    let path = textures.join("gui/container/inventory.png");
    if !path.pack_is_file() {
        return Ok(());
    }
    let image = normalized(crate::packfs::open_image(path)?.to_rgba8());
    let scale = image.width() / 256;
    if scale == 0 || !image.width().is_multiple_of(256) || image.height() < 252 * scale {
        return Ok(());
    }
    let mut count = 0;
    for (name, x, y) in EFFECTS {
        let output = textures.join(format!("mob_effect/{name}.png"));
        if output.pack_is_file() {
            continue;
        }
        let tile = imageops::crop_imm(
            &image,
            x * 18 * scale,
            (198 + y * 18) * scale,
            18 * scale,
            18 * scale,
        )
        .to_image();
        if tile.pixels().all(|pixel| pixel[3] == 0) {
            continue;
        }
        save(root, &output, &tile)?;
        count += 1;
    }
    if count > 0 {
        report.converted.push(format!(
            "药水效果升级：在旧图集区域被重排前拆出 {count} 张独立图标"
        ));
    }
    Ok(())
}

fn inventory(root: &Path, source: u32, target: u32, report: &mut AssetReport) -> Result<()> {
    if source >= 2 || target < 2 {
        return Ok(());
    }
    let path =
        root.join("assets/minecraft/textures/gui/container/creative_inventory/tab_inventory.png");
    if !path.pack_is_file() {
        return Ok(());
    }
    let image = normalized(crate::packfs::open_image(&path)?.to_rgba8());
    let scale = image.width() / 256;
    if scale == 0 || !image.width().is_multiple_of(256) || image.height() < 50 * scale {
        return Ok(());
    }
    let darkness = |left: u32| {
        let mut sum = 0_u64;
        for y in 8 * scale..46 * scale {
            for x in (left + 4) * scale..(left + 28) * scale {
                let pixel = image.get_pixel(x, y);
                sum += u64::from(pixel[0].max(pixel[1]).max(pixel[2])) * u64::from(pixel[3]) / 255;
            }
        }
        sum as f64 / f64::from(24 * 38 * scale * scale)
    };
    if darkness(73) + 6.0 < darkness(28) {
        return Ok(());
    }
    let mut output = image.clone();
    super::assets::clear_inventory_region(
        &mut output,
        &image,
        7 * scale,
        4 * scale,
        118 * scale,
        46 * scale,
    );
    let group = imageops::crop_imm(&image, 8 * scale, 5 * scale, 72 * scale, 45 * scale).to_image();
    imageops::replace(
        &mut output,
        &group,
        53 * i64::from(scale),
        5 * i64::from(scale),
    );
    let offhand =
        imageops::crop_imm(&image, 8 * scale, 5 * scale, 18 * scale, 18 * scale).to_image();
    imageops::replace(
        &mut output,
        &offhand,
        34 * i64::from(scale),
        18 * i64::from(scale),
    );
    save(root, &path, &output)?;
    report
        .converted
        .push("创造背包升级：人物窗口与装备格右移 45px，生成新版副手槽".into());
    Ok(())
}

fn markers(image: &RgbaImage, selected: bool) -> bool {
    image
        .pixels()
        .filter(|pixel| {
            pixel[3] > 240
                && if selected {
                    pixel[0] < 16 && pixel[1] <= 150 && pixel[2] > 240
                } else {
                    pixel[0] > 240 && pixel[1] < 16 && pixel[2] < 16
                }
        })
        .count()
        * 5
        > image.width() as usize * image.height() as usize
}

fn tabs(root: &Path, source: u32, target: u32, report: &mut AssetReport) -> Result<()> {
    if target < 12 || source > target {
        return Ok(());
    }
    let textures = root.join("assets/minecraft/textures");
    let path = textures.join("gui/container/creative_inventory/tabs.png");
    let atlas = if path.pack_is_file() {
        Some(normalized(crate::packfs::open_image(&path)?.to_rgba8()))
    } else {
        None
    };
    let directory = textures.join("gui/sprites/container/creative_inventory");
    let mut provided = BTreeMap::new();
    for side in ["top", "bottom"] {
        for state in ["unselected", "selected"] {
            for index in 1..=7 {
                let name = format!("tab_{side}_{state}_{index}");
                let path = directory.join(format!("{name}.png"));
                if path.pack_is_file() {
                    provided.insert(name, crate::packfs::open_image(path)?.to_rgba8());
                }
            }
        }
    }
    let corrupted = ["top", "bottom"].into_iter().any(|side| {
        provided
            .get(&format!("tab_{side}_selected_7"))
            .is_some_and(|image| markers(image, true))
            && provided
                .get(&format!("tab_{side}_unselected_7"))
                .is_some_and(|image| markers(image, false))
    });
    let scale = atlas
        .as_ref()
        .map(|image| image.width() / 256)
        .or_else(|| {
            provided
                .values()
                .next()
                .map(|image| (image.height() / 32).max(1))
        })
        .unwrap_or(0);
    if scale == 0 {
        return Ok(());
    }
    if let Some(atlas) = &atlas {
        ensure!(
            atlas.width().is_multiple_of(256) && atlas.height() >= 128 * scale,
            "创造标签页图集比例无效"
        );
    }
    let source_columns = if source < 12 { 6 } else { 7 };
    let source_width = if source < 12 { 28 } else { 26 };
    let mut rebuilt = atlas
        .clone()
        .unwrap_or_else(|| RgbaImage::new(256 * scale, 256 * scale));
    let mut count = 0;
    for (side, state, y) in [
        ("top", "unselected", 0),
        ("top", "selected", 32),
        ("bottom", "unselected", 64),
        ("bottom", "selected", 96),
    ] {
        for index in 1..=7 {
            let name = format!("tab_{side}_{state}_{index}");
            let output = directory.join(format!("{name}.png"));
            if target >= 18 && output.pack_is_file() && !corrupted {
                continue;
            }
            let tile = if corrupted {
                let Some(left) = provided.get(&format!("tab_{side}_{state}_1")) else {
                    continue;
                };
                let image =
                    imageops::resize(left, 26 * scale, 32 * scale, imageops::FilterType::Nearest);
                RgbaImage::from_fn(26 * scale, 32 * scale, |x, y| {
                    *image.get_pixel(x.min(26 * scale - 1 - x), y)
                })
            } else if let Some(atlas) = &atlas {
                if source >= 18 {
                    continue;
                }
                let column = if index == 7 {
                    source_columns - 1
                } else {
                    (index - 1).min(source_columns - 2)
                };
                let tile = imageops::crop_imm(
                    atlas,
                    column * source_width * scale,
                    y * scale,
                    source_width * scale,
                    32 * scale,
                )
                .to_image();
                super::assets::nine_slice(&tile, 26 * scale, 32 * scale, 2 * scale)
            } else {
                continue;
            };
            if target >= 18 {
                save(root, &output, &tile)?;
            } else if source < 12 {
                imageops::replace(
                    &mut rebuilt,
                    &tile,
                    i64::from((index - 1) * 26 * scale),
                    i64::from(y * scale),
                );
            }
            count += 1;
        }
    }
    if let Some(atlas) = &atlas {
        if target >= 18 && source < 18 {
            for (name, x) in [("scroller", 232), ("scroller_disabled", 244)] {
                let output = directory.join(format!("{name}.png"));
                if output.pack_is_file() {
                    continue;
                }
                let tile =
                    imageops::crop_imm(atlas, x * scale, 0, 12 * scale, 15 * scale).to_image();
                save(root, &output, &tile)?;
                count += 1;
            }
        }
    }
    if target < 18 && source < 12 && count > 0 {
        for y in 0..128 * scale {
            for x in 182 * scale..232 * scale {
                rebuilt.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            }
        }
        save(root, &path, &rebuilt)?;
    }
    if count > 0 {
        report.converted.push(format!(
            "创造标签页升级：按旧图集真实列宽重排 {count} 个资源，适配七列 26px 与独立精灵"
        ));
    }
    if corrupted && count > 0 {
        report
            .converted
            .push("高版本创造背包：修复上传包内误切的右侧占位精灵".into());
    }
    Ok(())
}

pub(super) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    inventory(root, source, target, report)?;
    tabs(root, source, target, report)?;
    Ok(())
}
