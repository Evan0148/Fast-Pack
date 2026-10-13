use super::assets::{AssetReport, save};
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

fn marker_coverage(image: &RgbaImage, selected: bool) -> bool {
    let matches = image
        .pixels()
        .filter(|pixel| {
            pixel[3] > 240
                && if selected {
                    pixel[0] < 16 && pixel[1] <= 150 && pixel[2] > 240
                } else {
                    pixel[0] > 240 && pixel[1] < 16 && pixel[2] < 16
                }
        })
        .count();
    matches * 5 > image.width() as usize * image.height() as usize
}

fn debug_tabs(tiles: &BTreeMap<String, RgbaImage>) -> bool {
    ["top", "bottom"].into_iter().any(|side| {
        tiles
            .get(&format!("tab_{side}_selected_7"))
            .is_some_and(|image| marker_coverage(image, true))
            && tiles
                .get(&format!("tab_{side}_unselected_7"))
                .is_some_and(|image| marker_coverage(image, false))
    })
}

fn symmetric_tab(image: &RgbaImage, width: u32, scale: u32) -> RgbaImage {
    let image = imageops::resize(image, 26 * scale, 32 * scale, imageops::FilterType::Nearest);
    RgbaImage::from_fn(width * scale, 32 * scale, |x, y| {
        let x = x.min(width * scale - 1 - x);
        *image.get_pixel(x, y)
    })
}

fn default_tab(bottom: bool, selected: bool, width: u32, scale: u32) -> RgbaImage {
    RgbaImage::from_fn(width * scale, 32 * scale, |x, y| {
        let x = x / scale;
        let y = if bottom { 31 - y / scale } else { y / scale };
        if !selected && y >= 28 || y == 0 && (x < 2 || x >= width - 2) {
            return Rgba([0, 0, 0, 0]);
        }
        if x == 0 || x == width - 1 || y == 0 || !selected && y == 27 {
            Rgba([32, 32, 32, 255])
        } else if x < 3 || y < 3 {
            Rgba([225, 225, 225, 255])
        } else if x >= width - 3 || !selected && y >= 25 {
            Rgba([100, 100, 100, 255])
        } else {
            Rgba([198, 198, 198, 255])
        }
    })
}

fn tabs(root: &Path, target: u32, report: &mut AssetReport) -> Result<()> {
    if target >= 18 {
        return Ok(());
    }
    let textures = root.join("assets/minecraft/textures");
    let sprites = textures.join("gui/sprites/container/creative_inventory");
    if !sprites.is_dir() {
        return Ok(());
    }
    let mut tiles = BTreeMap::new();
    for side in ["top", "bottom"] {
        for selected in ["unselected", "selected"] {
            for index in 1..=7 {
                let name = format!("tab_{side}_{selected}_{index}");
                let path = sprites.join(format!("{name}.png"));
                if path.is_file() {
                    tiles.insert(name, image::open(path)?.to_rgba8());
                }
            }
        }
    }
    for name in ["scroller", "scroller_disabled"] {
        let path = sprites.join(format!("{name}.png"));
        if path.is_file() {
            tiles.insert(name.into(), image::open(path)?.to_rgba8());
        }
    }
    if tiles.is_empty() {
        return Ok(());
    }
    let output = textures.join("gui/container/creative_inventory/tabs.png");
    let existing = if output.is_file() {
        Some(image::open(&output)?.to_rgba8())
    } else {
        None
    };
    let existing_scale = existing
        .as_ref()
        .map(|image| (image.width() / 256).max(1))
        .unwrap_or(1);
    let scale = existing_scale.max(
        tiles
            .iter()
            .map(|(name, image)| {
                let height = if name.starts_with("scroller") { 15 } else { 32 };
                (image.height() / height).max(1)
            })
            .max()
            .unwrap_or(1),
    );
    let legacy_markers = debug_tabs(&tiles);
    let mut atlas = existing
        .as_ref()
        .map(|image| {
            imageops::resize(
                image,
                256 * scale,
                256 * scale,
                imageops::FilterType::Nearest,
            )
        })
        .unwrap_or_else(|| RgbaImage::new(256 * scale, 256 * scale));
    let columns = if target < 12 { 6 } else { 7 };
    let width = if target < 12 { 28 } else { 26 };
    let mut count = 0;
    for (side, state, y) in [
        ("top", "unselected", 0),
        ("top", "selected", 32),
        ("bottom", "unselected", 64),
        ("bottom", "selected", 96),
    ] {
        let family = format!("tab_{side}_{state}_");
        for column in 0..columns {
            let index = if column == columns - 1 { 7 } else { column + 1 };
            let name = format!("tab_{side}_{state}_{index}");
            let tile = (if legacy_markers {
                tiles.get(&format!("{family}1"))
            } else {
                tiles.get(&name)
            })
            .filter(|image| !legacy_markers || !marker_coverage(image, state == "selected"))
            .or_else(|| {
                tiles
                    .get(&format!("{family}2"))
                    .filter(|image| !legacy_markers || !marker_coverage(image, state == "selected"))
            })
            .or_else(|| {
                tiles
                    .iter()
                    .find(|(name, image)| {
                        name.starts_with(&family)
                            && (!legacy_markers || !marker_coverage(image, state == "selected"))
                    })
                    .map(|(_, image)| image)
            });
            let widened = if let Some(tile) = tile {
                if legacy_markers {
                    symmetric_tab(tile, width, scale)
                } else {
                    let unit = imageops::resize(
                        tile,
                        26 * scale,
                        32 * scale,
                        imageops::FilterType::Nearest,
                    );
                    super::assets::nine_slice(&unit, width * scale, 32 * scale, 2 * scale)
                }
            } else if existing.is_some() && !legacy_markers {
                imageops::crop_imm(
                    &atlas,
                    column * width * scale,
                    y * scale,
                    width * scale,
                    32 * scale,
                )
                .to_image()
            } else {
                default_tab(side == "bottom", state == "selected", width, scale)
            };
            imageops::replace(
                &mut atlas,
                &widened,
                i64::from(column * width * scale),
                i64::from(y * scale),
            );
            count += 1;
        }
    }
    if legacy_markers {
        for y in 0..128 * scale {
            for x in columns * width * scale..232 * scale {
                atlas.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            }
        }
    }
    for (name, x) in [("scroller", 232), ("scroller_disabled", 244)] {
        if let Some(tile) = tiles.get(name) {
            let tile =
                imageops::resize(tile, 12 * scale, 15 * scale, imageops::FilterType::Nearest);
            imageops::replace(&mut atlas, &tile, i64::from(x * scale), 0);
            count += 1;
        }
    }
    save(root, &output, &atlas)?;
    report.converted.push(format!(
        "创造背包：将 {count} 个标签页与滚动条重建为 {width}px 图集，按目标版本列数对齐"
    ));
    if legacy_markers {
        report
            .converted
            .push("创造背包：识别到旧图集误切的红蓝 SEL 占位区，从同组完整左边框重建标签页".into());
    }
    Ok(())
}

fn inventory(root: &Path, source: u32, target: u32, report: &mut AssetReport) -> Result<()> {
    if target >= 2 {
        return Ok(());
    }
    let path =
        root.join("assets/minecraft/textures/gui/container/creative_inventory/tab_inventory.png");
    if !path.is_file() {
        return Ok(());
    }
    let image = image::open(&path)?.to_rgba8();
    let scale = image.width() / 256;
    if scale == 0 || !image.width().is_multiple_of(256) || image.height() < 50 * scale {
        return Ok(());
    }
    let darkness = |x: u32| {
        let mut sum = 0_u64;
        for y in 8 * scale..46 * scale {
            for x in (x + 4) * scale..(x + 28) * scale {
                let pixel = image.get_pixel(x, y);
                sum += u64::from(pixel[0].max(pixel[1]).max(pixel[2])) * u64::from(pixel[3]) / 255;
            }
        }
        sum as f64 / f64::from(24 * 38 * scale * scale)
    };
    let legacy = darkness(28);
    let modern = darkness(73);
    if legacy + 6.0 < modern || source < 2 && modern + 6.0 >= legacy {
        return Ok(());
    }
    let mut converted = image.clone();
    super::assets::clear_inventory_region(
        &mut converted,
        &image,
        7 * scale,
        4 * scale,
        118 * scale,
        46 * scale,
    );
    let group =
        imageops::crop_imm(&image, 53 * scale, 5 * scale, 72 * scale, 45 * scale).to_image();
    imageops::replace(
        &mut converted,
        &group,
        8 * i64::from(scale),
        5 * i64::from(scale),
    );
    save(root, &path, &converted)?;
    report
        .converted
        .push("创造背包：人物背景与四个装备格左移 45px，清理旧版不使用的副手槽".into());
    Ok(())
}

fn effects(root: &Path, source: u32, target: u32, report: &mut AssetReport) -> Result<()> {
    if target >= 4 {
        return Ok(());
    }
    let textures = root.join("assets/minecraft/textures");
    let mut icons = Vec::new();
    for (name, x, y) in EFFECTS {
        let path = textures.join(format!("mob_effect/{name}.png"));
        if path.is_file() {
            icons.push((name, x, y, image::open(path)?.to_rgba8()));
        }
    }
    let output = textures.join("gui/container/inventory.png");
    if !output.is_file() {
        report
            .warnings
            .push("药水图标：输入包没有背包背景，沿用目标游戏原版背景及图标".into());
        return Ok(());
    }
    let mut atlas = image::open(&output)?.to_rgba8();
    let scale = atlas.width() / 256;
    ensure!(
        scale > 0 && atlas.width().is_multiple_of(256),
        "药水图标图集比例无效"
    );
    if atlas.height() < 256 * scale {
        let mut expanded = RgbaImage::new(atlas.width(), 256 * scale);
        imageops::replace(&mut expanded, &atlas, 0, 0);
        atlas = expanded;
    }
    if source >= 8 {
        for (_, x, y) in EFFECTS {
            for row in (198 + y * 18) * scale..(216 + y * 18) * scale {
                for column in x * 18 * scale..(x + 1) * 18 * scale {
                    atlas.put_pixel(column, row, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }
    for (_, x, y, icon) in &icons {
        let icon = imageops::resize(icon, 18 * scale, 18 * scale, imageops::FilterType::Nearest);
        imageops::replace(
            &mut atlas,
            &icon,
            i64::from(x * 18 * scale),
            i64::from((198 + y * 18) * scale),
        );
    }
    let mut restored = 0;
    for (_, x, y) in EFFECTS {
        let supplied = icons
            .iter()
            .any(|(_, column, row, _)| *column == x && *row == y);
        if supplied {
            continue;
        }
        let left = x * 18 * scale;
        let top = (198 + y * 18) * scale;
        let present = (top..top + 18 * scale).any(|row| {
            (left..left + 18 * scale).any(|column| atlas.get_pixel(column, row)[3] > 32)
        });
        if present {
            continue;
        }
        let mask = &super::legacy_effect_icons::MASKS[(y * 8 + x) as usize];
        let icon = RgbaImage::from_fn(18, 18, |column, row| {
            if mask[row as usize] & (1 << column) != 0 {
                Rgba([239, 239, 239, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let icon = imageops::resize(&icon, 18 * scale, 18 * scale, imageops::FilterType::Nearest);
        imageops::replace(&mut atlas, &icon, i64::from(left), i64::from(top));
        restored += 1;
    }
    save(root, &output, &atlas)?;
    report.converted.push(format!(
        "药水效果图标：将 {} 张独立图标合并回旧版背包图集，并补齐 {restored} 个缺失的旧版默认符号",
        icons.len()
    ));
    Ok(())
}

pub(super) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    tabs(root, target, report)?;
    inventory(root, source, target, report)?;
    if source <= target {
        return Ok(());
    }
    effects(root, source, target, report)?;
    Ok(())
}
