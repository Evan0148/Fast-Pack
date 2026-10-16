use super::assets::{AssetReport, backup};
use crate::packfs as fs;
use crate::packfs::PathExt;
use anyhow::Result;
use serde_json::Value;
use std::path::Path;

fn copy(textures: &Path, from: &str, to: &str) -> Result<bool> {
    let from = textures.join(format!("{from}.png"));
    let to = textures.join(format!("{to}.png"));
    if !from.pack_is_file() || to.pack_is_file() {
        return Ok(false);
    }
    fs::create_dir_all(to.parent().unwrap())?;
    fs::copy(&from, &to)?;
    if from.with_extension("png.mcmeta").pack_is_file() {
        fs::copy(
            from.with_extension("png.mcmeta"),
            to.with_extension("png.mcmeta"),
        )?;
    }
    Ok(true)
}

fn grass(value: &mut Value, upgrade: bool) {
    match value {
        Value::String(text) => {
            let (namespace, name) = text.split_once(':').unwrap_or(("minecraft", text));
            if namespace != "minecraft" {
                return;
            }
            let (before, after) = if upgrade {
                ("grass", "short_grass")
            } else {
                ("short_grass", "grass")
            };
            if matches!(name, n if n == format!("block/{before}") || n == format!("item/{before}"))
            {
                let prefix = name.split_once('/').unwrap().0;
                *text = format!("{namespace}:{prefix}/{after}");
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                grass(value, upgrade);
            }
        }
        Value::Array(values) => {
            for value in values {
                grass(value, upgrade);
            }
        }
        _ => {}
    }
}

fn trim_refs(value: &mut Value, upgrade: bool) {
    match value {
        Value::String(text) => {
            let (namespace, name) = text.split_once(':').unwrap_or(("minecraft", text));
            if namespace != "minecraft" {
                return;
            }
            let mapped = if upgrade {
                name.strip_prefix("trims/models/armor/").map(|name| {
                    if let Some(name) = name.strip_suffix("_leggings") {
                        format!("trims/entity/humanoid_leggings/{name}")
                    } else {
                        format!("trims/entity/humanoid/{name}")
                    }
                })
            } else {
                name.strip_prefix("trims/entity/humanoid/")
                    .map(|n| format!("trims/models/armor/{n}"))
                    .or_else(|| {
                        name.strip_prefix("trims/entity/humanoid_leggings/")
                            .map(|n| format!("trims/models/armor/{n}_leggings"))
                    })
            };
            if let Some(name) = mapped {
                *text = format!("{namespace}:{name}");
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                trim_refs(value, upgrade);
            }
        }
        Value::Array(values) => {
            for value in values {
                trim_refs(value, upgrade);
            }
        }
        _ => {}
    }
}

pub(super) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    let textures = root.join("assets/minecraft/textures");
    let mut count = 0;
    let mut mappings = vec![
        (
            55,
            "entity/pig/pig".into(),
            "entity/pig/temperate_pig".into(),
        ),
        (
            55,
            "entity/cow/cow".into(),
            "entity/cow/temperate_cow".into(),
        ),
        (
            55,
            "entity/chicken".into(),
            "entity/chicken/temperate_chicken".into(),
        ),
        (
            55,
            "entity/sheep/sheep_fur".into(),
            "entity/sheep/sheep_wool".into(),
        ),
        (
            42,
            "entity/elytra".into(),
            "entity/equipment/wings/elytra".into(),
        ),
        (
            55,
            "entity/pig/pig_saddle".into(),
            "entity/equipment/pig_saddle/saddle".into(),
        ),
        (
            55,
            "entity/strider/strider_saddle".into(),
            "entity/equipment/strider_saddle/saddle".into(),
        ),
    ];
    for material in ["leather", "iron", "gold", "diamond"] {
        mappings.push((
            42,
            format!("entity/horse/armor/horse_armor_{material}"),
            format!("entity/equipment/horse_body/{material}"),
        ));
    }
    for color in crate::tables()
        .colors
        .iter()
        .map(String::as_str)
        .chain(["trader_llama"])
    {
        mappings.push((
            42,
            format!("entity/llama/decor/{color}"),
            format!("entity/equipment/llama_body/{color}"),
        ));
    }
    let old_trims = textures.join("trims/models/armor");
    let modern_trims = textures.join("trims/entity");
    let mut trim_names = std::collections::BTreeSet::new();
    for directory in [
        &old_trims,
        &modern_trims.join("humanoid"),
        &modern_trims.join("humanoid_leggings"),
    ] {
        if directory.pack_is_dir() {
            for entry in fs::read_dir(directory)? {
                let path = entry?.path();
                if path.extension().is_some_and(|s| s == "png") {
                    trim_names.insert(
                        path.file_stem()
                            .unwrap()
                            .to_string_lossy()
                            .trim_end_matches("_leggings")
                            .to_string(),
                    );
                }
            }
        }
    }
    for name in trim_names {
        mappings.push((
            42,
            format!("trims/models/armor/{name}"),
            format!("trims/entity/humanoid/{name}"),
        ));
        mappings.push((
            42,
            format!("trims/models/armor/{name}_leggings"),
            format!("trims/entity/humanoid_leggings/{name}"),
        ));
    }
    for (boundary, old, new) in mappings {
        let (from, to) = if source < boundary && target >= boundary {
            (&old, &new)
        } else if source >= boundary && target < boundary {
            (&new, &old)
        } else {
            continue;
        };
        count += usize::from(copy(&textures, from, to)?);
    }
    if source < 42 && target >= 42 || source >= 42 && target < 42 {
        let path = root.join("assets/minecraft/atlases/armor_trims.json");
        if path.pack_is_file() {
            let mut value: Value = serde_json::from_slice(&fs::read(&path)?)?;
            let previous = value.clone();
            trim_refs(&mut value, target >= 42);
            if value != previous {
                backup(root, &path)?;
                fs::write(path, serde_json::to_vec_pretty(&value)?)?;
                count += 1;
            }
        }
    }
    if source < 22 && target >= 22 || source >= 22 && (4..22).contains(&target) {
        let upgrade = target >= 22;
        let (from, to) = if upgrade {
            ("grass", "short_grass")
        } else {
            ("short_grass", "grass")
        };
        count += usize::from(copy(
            &textures,
            &format!("block/{from}"),
            &format!("block/{to}"),
        )?);
        let directory = root.join("assets");
        let mut writes = Vec::new();
        for entry in crate::packfs::WalkDir::new(&directory).min_depth(1) {
            let entry = entry?;
            if !entry.file_type().is_file() || entry.path().extension().is_none_or(|s| s != "json")
            {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&directory)?
                .to_string_lossy()
                .replace('\\', "/");
            let Some((namespace, relative)) = relative.split_once('/') else {
                continue;
            };
            if !relative.starts_with("models/")
                && !relative.starts_with("items/")
                && !relative.starts_with("blockstates/")
            {
                continue;
            }
            let Ok(mut value) = serde_json::from_slice::<Value>(&fs::read(entry.path())?) else {
                continue;
            };
            let old = value.clone();
            grass(&mut value, upgrade);
            if old != value {
                writes.push((entry.path().to_path_buf(), value.clone()));
            }
            if namespace == "minecraft"
                && (relative == format!("models/block/{from}.json")
                    || relative == format!("models/item/{from}.json")
                    || relative == format!("items/{from}.json"))
            {
                let output = entry.path().with_file_name(format!("{to}.json"));
                if !output.pack_is_file() {
                    writes.push((output, value));
                }
            }
        }
        for (path, value) in writes {
            if path.pack_is_file() {
                backup(root, &path)?;
            }
            fs::write(path, serde_json::to_vec_pretty(&value)?)?;
            count += 1;
        }
    }
    if count > 0 {
        report.converted.push(format!("资源代际路径：适配 {count} 个短草、动物、鞘翅、马铠、鞍具及实体盔甲纹饰资源，保留已有目标纹理与动画"));
    }
    Ok(())
}
