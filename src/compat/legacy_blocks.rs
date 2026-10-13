use super::assets::{AssetReport, backup};
use anyhow::Result;
use serde_json::{Map, Value, json};
use std::fs;
use std::path::Path;

fn write(root: &Path, path: &Path, value: &Value) -> Result<bool> {
    if path.is_file() {
        if serde_json::from_slice::<Value>(&fs::read(path)?)
            .ok()
            .as_ref()
            == Some(value)
        {
            return Ok(false);
        }
        backup(root, path)?;
    }
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(true)
}

fn model_ref(model: &str) -> Option<String> {
    let (namespace, name) = model.split_once(':').unwrap_or(("minecraft", model));
    let name = name.strip_prefix("block/").unwrap_or(name);
    if name.is_empty()
        || !namespace
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-/".contains(c))
        || name
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return None;
    }
    Some(if namespace == "minecraft" {
        name.into()
    } else {
        format!("{namespace}:{name}")
    })
}

fn normalize_model(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                normalize_model(value);
            }
        }
        Value::Object(values) => {
            if let Some(model) = values.get_mut("model") {
                if let Some(name) = model.as_str().and_then(model_ref) {
                    *model = json!(name);
                }
            }
        }
        _ => {}
    }
}

fn condition(value: &Value, facing: &str) -> bool {
    let Some(values) = value.as_object() else {
        return false;
    };
    if let Some(or) = values.get("OR").and_then(Value::as_array) {
        return or.iter().any(|value| condition(value, facing));
    }
    if let Some(and) = values.get("AND").and_then(Value::as_array) {
        return and.iter().all(|value| condition(value, facing));
    }
    values.iter().all(|(key, value)| match key.as_str() {
        "facing" => value
            .as_str()
            .is_some_and(|value| value.split('|').any(|value| value == facing)),
        "triggered" => value.as_str() == Some("false") || value.as_bool() == Some(false),
        _ => false,
    })
}

fn directional(root: &Path, name: &str, report: &mut AssetReport) -> Result<()> {
    let directory = root.join("assets/minecraft");
    let path = directory.join(format!("blockstates/{name}.json"));
    let provided = path.is_file()
        || [
            format!("models/block/{name}.json"),
            format!("models/block/{name}_vertical.json"),
            format!("textures/blocks/{name}_front_horizontal.png"),
            format!("textures/blocks/{name}_front_vertical.png"),
        ]
        .iter()
        .any(|relative| directory.join(relative).is_file());
    if !provided {
        return Ok(());
    }
    let original = fs::read(&path)
        .ok()
        .and_then(|data| serde_json::from_slice::<Value>(&data).ok());
    let mut variants = Map::new();
    for (facing, rotation) in [
        ("down", Some(("x", 180))),
        ("up", None),
        ("north", None),
        ("south", Some(("y", 180))),
        ("west", Some(("y", 270))),
        ("east", Some(("y", 90))),
    ] {
        let key = format!("facing={facing}");
        let vertical = matches!(facing, "up" | "down");
        let model = if vertical {
            format!("{name}_vertical")
        } else {
            name.into()
        };
        let mut fallback = json!({"model":model});
        if let Some((axis, angle)) = rotation {
            fallback[axis] = json!(angle);
        }
        let selected = original
            .as_ref()
            .and_then(|value| value.get("variants"))
            .and_then(Value::as_object)
            .and_then(|values| {
                values.get(&key).or_else(|| {
                    values
                        .iter()
                        .find(|(key, _)| {
                            let parts = key.split(',').map(|part| part.trim()).collect::<Vec<_>>();
                            parts.contains(&format!("facing={facing}").as_str())
                                && parts.iter().all(|part| {
                                    part.starts_with("facing=") || *part == "triggered=false"
                                })
                        })
                        .map(|(_, value)| value)
                })
            });
        let selected = selected.cloned().or_else(|| {
            original
                .as_ref()
                .and_then(|value| value.get("multipart"))
                .and_then(Value::as_array)
                .and_then(|parts| {
                    let applicable = parts
                        .iter()
                        .filter(|part| {
                            part.get("when")
                                .is_none_or(|value| condition(value, facing))
                        })
                        .filter_map(|part| part.get("apply"))
                        .collect::<Vec<_>>();
                    if applicable.len() == 1 {
                        Some(applicable[0].clone())
                    } else {
                        None
                    }
                })
        });
        let mut value = selected.unwrap_or(fallback);
        normalize_model(&mut value);
        variants.insert(key, value);
    }
    let mut changed = write(root, &path, &json!({"variants":variants}))?;
    for vertical in [false, true] {
        let model = if vertical {
            format!("{name}_vertical")
        } else {
            name.into()
        };
        let path = directory.join(format!("models/block/{model}.json"));
        if path.is_file() {
            continue;
        }
        let value = if vertical {
            json!({"parent":"block/orientable_vertical", "textures":{"front":format!("blocks/{name}_front_vertical"), "side":"blocks/furnace_top"}})
        } else {
            json!({"parent":"block/orientable", "textures":{"front":format!("blocks/{name}_front_horizontal"), "top":"blocks/furnace_top", "side":"blocks/furnace_side"}})
        };
        changed |= write(root, &path, &value)?;
    }
    let horizontal = directory.join(format!("textures/blocks/{name}_front_horizontal.png"));
    let vertical = directory.join(format!("textures/blocks/{name}_front_vertical.png"));
    if !vertical.is_file() && horizontal.is_file() {
        fs::copy(&horizontal, &vertical)?;
        if horizontal.with_extension("png.mcmeta").is_file() {
            fs::copy(
                horizontal.with_extension("png.mcmeta"),
                vertical.with_extension("png.mcmeta"),
            )?;
        }
        changed = true;
    }
    if changed {
        report.converted.push(format!(
            "{name}：还原上、下、东、西、南、北六个旧版朝向，补齐水平与垂直模型及纹理引用"
        ));
    }
    Ok(())
}

pub(super) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    if source < 4 || target >= 4 {
        return Ok(());
    }
    directional(root, "dispenser", report)?;
    directional(root, "dropper", report)?;
    Ok(())
}
