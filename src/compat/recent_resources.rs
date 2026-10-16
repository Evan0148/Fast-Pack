use super::assets::{AssetReport, backup};
use crate::packfs as fs;
use crate::packfs::PathExt;
use crate::packfs::WalkDir;
use anyhow::Result;
use serde_json::{Value, json};
use std::path::Path;

fn palette(reference: &str) -> Option<String> {
    let (namespace, name) = reference
        .split_once(':')
        .unwrap_or(("minecraft", reference));
    let name = name.strip_prefix("trims/color_palettes/")?;
    if name.contains('/') {
        return None;
    }
    Some(format!(
        "{namespace}:{}",
        if name == "trim_palette" {
            "trim_base".into()
        } else {
            format!("trim/{name}")
        }
    ))
}

fn rewrite(value: &mut Value, farmland: bool) -> bool {
    let old = value.clone();
    match value {
        Value::String(text) => {
            if let Some(mapped) = palette(text) {
                *text = mapped;
            }
        }
        Value::Array(values) => {
            for value in values {
                rewrite(value, farmland);
            }
        }
        Value::Object(values) => {
            if farmland
                && values
                    .get("parent")
                    .and_then(Value::as_str)
                    .is_some_and(|name| {
                        matches!(
                            name,
                            "block/template_farmland" | "minecraft:block/template_farmland"
                        )
                    })
            {
                values.insert(
                    "parent".into(),
                    json!("minecraft:block/template_cube_bottom_top_indented"),
                );
                if let Some(textures) = values.get_mut("textures").and_then(Value::as_object_mut) {
                    let dirt = textures
                        .get("dirt")
                        .cloned()
                        .unwrap_or_else(|| json!("minecraft:block/dirt"));
                    textures.entry("side").or_insert_with(|| dirt.clone());
                    textures.entry("bottom").or_insert(dirt);
                }
            }
            for value in values.values_mut() {
                rewrite(value, farmland);
            }
        }
        _ => {}
    }
    *value != old
}

pub(super) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    if target < 97 || source >= 97 {
        return Ok(());
    }
    let palettes = root.join("assets/minecraft/textures/trims/color_palettes");
    let mut count = 0;
    if palettes.pack_is_dir() {
        for entry in fs::read_dir(palettes)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let (stem, suffix) = if let Some(stem) = name.strip_suffix(".png.mcmeta") {
                (stem, ".png.mcmeta")
            } else if let Some(stem) = name.strip_suffix(".png") {
                (stem, ".png")
            } else {
                continue;
            };
            let destination = root.join(format!(
                "assets/minecraft/textures/{}{}",
                if stem == "trim_palette" {
                    "trim_base".into()
                } else {
                    format!("trim/{stem}")
                },
                suffix
            ));
            if destination.pack_is_file() {
                continue;
            }
            fs::create_dir_all(destination.parent().unwrap())?;
            fs::copy(entry.path(), destination)?;
            count += 1;
        }
    }
    let farmland = !root
        .join("assets/minecraft/models/block/template_farmland.json")
        .pack_is_file();
    let mut json_count = 0;
    let assets = root.join("assets");
    if assets.pack_is_dir() {
        for entry in WalkDir::new(assets).min_depth(1) {
            let entry = entry?;
            if !entry.file_type().is_file()
                || entry
                    .path()
                    .extension()
                    .is_none_or(|extension| extension != "json")
            {
                continue;
            }
            let Ok(mut value) = serde_json::from_slice::<Value>(&fs::read(entry.path())?) else {
                continue;
            };
            if rewrite(&mut value, farmland) {
                backup(root, entry.path())?;
                fs::write(entry.path(), serde_json::to_vec_pretty(&value)?)?;
                json_count += 1;
            }
        }
    }
    if count + json_count > 0 {
        report.converted.push(format!(
            "26.3+ 资源更新：迁移 {count} 个纹饰调色板资源并修正 {json_count} 个图集 / 耕地模板引用"
        ));
    }
    Ok(())
}
