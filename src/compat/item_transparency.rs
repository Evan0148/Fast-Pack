use super::assets::{AssetReport, open, save};
use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

fn model_path(root: &Path, reference: &str) -> Option<PathBuf> {
    let (namespace, name) = reference
        .split_once(':')
        .unwrap_or(("minecraft", reference));
    if namespace.is_empty()
        || name.is_empty()
        || !namespace.chars().all(|value| {
            value.is_ascii_lowercase() || value.is_ascii_digit() || "._-".contains(value)
        })
        || !name.chars().all(|value| {
            value.is_ascii_lowercase() || value.is_ascii_digit() || "._-/".contains(value)
        })
        || name
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return None;
    }
    Some(root.join(format!("assets/{namespace}/models/{name}.json")))
}

fn generated(root: &Path, name: &str) -> bool {
    let mut next = format!("minecraft:item/{name}");
    let mut visited = BTreeSet::new();
    for _ in 0..64 {
        if !visited.insert(next.clone()) {
            return false;
        }
        let Some(path) = model_path(root, &next) else {
            return false;
        };
        let value = if path.is_file() {
            let Ok(bytes) = fs::read(&path) else {
                return false;
            };
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                return false;
            };
            value
        } else {
            let vanilla = next.strip_prefix("minecraft:").unwrap_or(&next);
            if matches!(
                vanilla,
                "builtin/generated" | "item/generated" | "item/handheld" | "item/handheld_rod"
            ) {
                return true;
            }
            let Some(reference) = next
                .strip_prefix("minecraft:item/")
                .and_then(super::item_bindings::reference)
            else {
                return false;
            };
            reference.clone()
        };
        if value.get("elements").is_some() {
            return false;
        }
        let Some(parent) = value.get("parent").and_then(Value::as_str) else {
            return false;
        };
        next = if parent.contains(':') {
            parent.into()
        } else {
            format!("minecraft:{parent}")
        };
    }
    false
}

pub(super) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    if source >= 2 || target < 2 {
        return Ok(());
    }
    let directory = root.join(if target >= 4 {
        "assets/minecraft/textures/item"
    } else {
        "assets/minecraft/textures/items"
    });
    if !directory.is_dir() {
        return Ok(());
    }
    let mut count = 0;
    for entry in WalkDir::new(&directory).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file()
            || entry
                .path()
                .extension()
                .is_none_or(|extension| extension != "png")
        {
            continue;
        }
        let name = entry
            .path()
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let name = crate::tables::legacy_items()
            .models
            .get(name.as_ref())
            .map_or(name.as_ref(), String::as_str);
        if !generated(root, name) {
            continue;
        }
        let Ok(mut image) = open(entry.path()) else {
            continue;
        };
        let total = u64::from(image.width()) * u64::from(image.height());
        let near_zero = image
            .pixels()
            .filter(|pixel| matches!(pixel[3], 1 | 2))
            .count() as u64;
        let opaque = image.pixels().filter(|pixel| pixel[3] >= 128).count() as u64;
        if near_zero * 4 < total || opaque == 0 {
            continue;
        }
        for pixel in image.pixels_mut() {
            if matches!(pixel[3], 1 | 2) {
                pixel[3] = 0;
            }
        }
        save(root, entry.path(), &image)?;
        count += 1;
    }
    if count > 0 {
        report.converted.push(format!(
            "物品模型厚度：修正 {count} 张旧版贴图大面积 alpha 1–2 的背景，让新版按实际轮廓生成侧面；保留正常像素、动画元数据和自定义立体模型"
        ));
    }
    Ok(())
}
