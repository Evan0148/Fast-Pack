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
    let scale = existing
        .as_ref()
        .map(|image| (image.width() / 256).max(1))
        .unwrap_or_else(|| {
            tiles
                .values()
                .map(|image| (image.height() / 32).max(1))
                .max()
                .unwrap_or(1)
        });
    let mut atlas = existing
        .map(|image| {
            imageops::resize(
                &image,
                256 * scale,
                256 * scale,
                imageops::FilterType::Nearest,
            )
        })
        .unwrap_or_else(|| RgbaImage::new(256 * scale, 256 * scale));
    let mut count = 0;
    for (side, state, y) in [
        ("top", "unselected", 0),
        ("top", "selected", 32),
        ("bottom", "unselected", 64),
        ("bottom", "selected", 96),
    ] {
        for (column, index) in [1, 2, 3, 4, 5, 7].into_iter().enumerate() {
            let name = format!("tab_{side}_{state}_{index}");
            let tile = tiles
                .get(&name)
                .or_else(|| tiles.get(&format!("tab_{side}_{state}_2")))
                .or_else(|| {
                    tiles
                        .iter()
                        .find(|(name, _)| name.starts_with(&format!("tab_{side}_{state}_")))
                        .map(|(_, image)| image)
                });
            let Some(tile) = tile else {
                continue;
            };
            let unit =
                imageops::resize(tile, 26 * scale, 32 * scale, imageops::FilterType::Nearest);
            let widened = super::assets::nine_slice(&unit, 28 * scale, 32 * scale, 2 * scale);
            imageops::replace(
                &mut atlas,
                &widened,
                column as i64 * 28 * i64::from(scale),
                i64::from(y * scale),
            );
            count += 1;
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
        "创造背包：将 {count} 个新版标签页与滚动条精灵重建为旧版 28px 图集"
    ));
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
    if source <= target {
        return Ok(());
    }
    tabs(root, target, report)?;
    effects(root, source, target, report)?;
    Ok(())
}
