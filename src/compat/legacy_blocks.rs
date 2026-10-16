use super::assets::{AssetReport, backup};
use crate::packfs as fs;
use crate::packfs::PathExt;
use anyhow::Result;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn write(root: &Path, path: &Path, value: &Value) -> Result<bool> {
    if path.pack_is_file() {
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

fn model_id(name: &str) -> Option<String> {
    let (namespace, name) = name.split_once(':').unwrap_or(("minecraft", name));
    if namespace.is_empty()
        || name.is_empty()
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
    Some(format!("{namespace}:{name}"))
}

fn inherited_fields(
    id: &str,
    models: &BTreeMap<String, Value>,
    visited: &mut BTreeSet<String>,
) -> Value {
    if visited.len() >= 64 || !visited.insert(id.into()) {
        return json!({});
    }
    let Some(model) = models.get(id) else {
        return json!({});
    };
    let mut inherited = model
        .get("parent")
        .and_then(Value::as_str)
        .and_then(model_id)
        .map(|parent| inherited_fields(&parent, models, visited))
        .unwrap_or_else(|| json!({}));
    for field in ["textures", "display"] {
        if let Some(values) = model.get(field).and_then(Value::as_object) {
            if !inherited.get(field).is_some_and(Value::is_object) {
                inherited[field] = json!({});
            }
            inherited[field]
                .as_object_mut()
                .unwrap()
                .extend(values.clone());
        }
    }
    if let Some(ambient) = model.get("ambientocclusion") {
        inherited["ambientocclusion"] = ambient.clone();
    }
    visited.remove(id);
    inherited
}

fn standalone_models(root: &Path, report: &mut AssetReport) -> Result<()> {
    let assets = root.join("assets");
    if !assets.pack_is_dir() {
        return Ok(());
    }
    let mut models = BTreeMap::new();
    let mut paths = BTreeMap::new();
    for entry in crate::packfs::WalkDir::new(&assets).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file() || entry.path().extension().is_none_or(|ext| ext != "json")
        {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&assets)?
            .to_string_lossy()
            .replace('\\', "/");
        let Some((namespace, name)) = relative.split_once("/models/") else {
            continue;
        };
        if namespace.contains('/') {
            continue;
        }
        let Some(id) = model_id(&format!("{namespace}:{}", name.trim_end_matches(".json"))) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&fs::read(entry.path())?) else {
            continue;
        };
        models.insert(id.clone(), value);
        paths.insert(id, entry.into_path());
    }
    let mut count = 0;
    for (id, value) in &models {
        if !value.get("elements").is_some_and(Value::is_array)
            || !value.get("parent").is_some_and(Value::is_string)
        {
            continue;
        }
        let mut converted = value.clone();
        let inherited = inherited_fields(id, &models, &mut BTreeSet::new());
        for field in ["textures", "display", "ambientocclusion"] {
            if let Some(value) = inherited.get(field) {
                converted[field] = value.clone();
            }
        }
        converted.as_object_mut().unwrap().remove("parent");
        if write(root, &paths[id], &converted)? {
            count += 1;
        }
    }
    if count > 0 {
        report.converted.push(format!("旧版立体模型：展开 {count} 个同时包含 parent 和 elements 的模型，保留几何、贴图与包内父模型设置"));
    }
    Ok(())
}

fn box_transform(value: &Value, view: &str) -> Option<Value> {
    let (rotation, translation, scale, legacy_rotation, legacy_translation, legacy_scale): (
        [f64; 3],
        [f64; 3],
        f64,
        [f64; 3],
        [f64; 3],
        f64,
    ) = match view {
        "gui" => ([30.0, 225.0, 0.0], [0.0; 3], 0.625, [0.0; 3], [0.0; 3], 1.0),
        "thirdperson" => (
            [75.0, 45.0, 0.0],
            [0.0, 2.5, 0.0],
            0.375,
            [10.0, -45.0, 170.0],
            [0.0, 1.5, -2.75],
            0.375,
        ),
        "firstperson" => ([0.0, 45.0, 0.0], [0.0; 3], 0.4, [0.0; 3], [0.0; 3], 1.0),
        _ => return None,
    };
    let read = |key: &str, default: [f64; 3]| -> Option<[f64; 3]> {
        value.get(key).map_or(Some(default), |value| {
            serde_json::from_value(value.clone()).ok()
        })
    };
    let input_rotation = read("rotation", [0.0; 3])?;
    let input_translation = read("translation", [0.0; 3])?;
    let input_scale = read("scale", [1.0; 3])?;
    let round = |value: f64| (value * 1_000_000.0).round() / 1_000_000.0;
    Some(json!({
        "rotation":std::array::from_fn::<_, 3, _>(|axis| round(legacy_rotation[axis] + input_rotation[axis] - rotation[axis])),
        "translation":std::array::from_fn::<_, 3, _>(|axis| round(legacy_translation[axis] + input_translation[axis] - translation[axis])),
        "scale":input_scale.map(|value| round(value * legacy_scale / scale))
    }))
}

fn modern_box_default(value: &Value, view: &str) -> bool {
    let (rotation, scale): ([f64; 3], [f64; 3]) = match view {
        "gui" => ([30.0, 225.0, 0.0], [0.625; 3]),
        "firstperson" => ([0.0, 45.0, 0.0], [0.4; 3]),
        "thirdperson" => ([75.0, 45.0, 0.0], [0.375; 3]),
        _ => return false,
    };
    let read = |key: &str| {
        value
            .get(key)
            .and_then(|value| serde_json::from_value::<[f64; 3]>(value.clone()).ok())
    };
    read("rotation") == Some(rotation) && read("scale") == Some(scale)
}

fn inventory_model(root: &Path, name: &str, source: u32, report: &mut AssetReport) -> Result<()> {
    let directory = root.join("assets/minecraft/models");
    let path = directory.join(format!("item/{name}.json"));
    let provided = path.pack_is_file();
    let mut model = if provided {
        serde_json::from_slice::<Value>(&fs::read(&path)?)?
    } else {
        json!({"parent":format!("block/{name}")})
    };
    let block_path = directory.join(format!("block/{name}.json"));
    let block = serde_json::from_slice::<Value>(&fs::read(&block_path)?)?;
    let migrated = [&model, &block].iter().any(|value| {
        value["parent"]
            .as_str()
            .is_some_and(|parent| parent.starts_with("fastpack:legacy_models/"))
    });
    let inherited = block.get("display").and_then(Value::as_object);
    if !model.get("display").is_some_and(Value::is_object) {
        model["display"] = json!({});
    }
    for (view, fallback) in [
        (
            "gui",
            json!({"rotation":[0,0,0],"translation":[0,0,0],"scale":[1,1,1]}),
        ),
        (
            "firstperson",
            json!({"rotation":[0,0,0],"translation":[0,0,0],"scale":[1,1,1]}),
        ),
        (
            "thirdperson",
            json!({"rotation":[10,-45,170],"translation":[0,1.5,-2.75],"scale":[0.375,0.375,0.375]}),
        ),
    ] {
        let right = format!("{view}_righthand");
        let own = model["display"]
            .get(view)
            .or_else(|| model["display"].get(&right))
            .cloned();
        let transform = own.clone().or_else(|| {
            inherited
                .and_then(|values| values.get(view).or_else(|| values.get(&right)))
                .cloned()
        });
        let modern_default = transform
            .as_ref()
            .is_some_and(|value| modern_box_default(value, view));
        let converted = if !migrated && (source >= 2 || modern_default) {
            transform
                .as_ref()
                .and_then(|value| box_transform(value, view))
                .unwrap_or(fallback)
        } else {
            own.unwrap_or(fallback)
        };
        model["display"][view] = converted;
        model["display"].as_object_mut().unwrap().remove(&right);
        model["display"]
            .as_object_mut()
            .unwrap()
            .remove(&format!("{view}_lefthand"));
    }
    let mut converted_block = block.clone();
    for view in ["gui", "firstperson", "thirdperson"] {
        if migrated {
            continue;
        }
        let right = format!("{view}_righthand");
        let Some(transform) =
            inherited.and_then(|display| display.get(view).or_else(|| display.get(&right)))
        else {
            continue;
        };
        if source < 2 && !modern_box_default(transform, view) {
            continue;
        }
        if let Some(converted) = box_transform(transform, view) {
            converted_block["display"][view] = converted;
            converted_block["display"]
                .as_object_mut()
                .unwrap()
                .remove(&right);
            converted_block["display"]
                .as_object_mut()
                .unwrap()
                .remove(&format!("{view}_lefthand"));
        }
    }
    if write(root, &block_path, &converted_block)? {
        report.converted.push(format!(
            "{name}：同步换算方块父模型的 GUI 与手持显示参数，阻止旧版默认值回退到新版缩放"
        ));
    }
    if write(root, &path, &model)? {
        report.converted.push(format!(
            "{name} 物品栏：生成旧版方块物品显示参数，修复角度重复叠加导致的图标变形"
        ));
    }
    Ok(())
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

fn directional(
    root: &Path,
    name: &str,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    let directory = root.join("assets/minecraft");
    let path = directory.join(format!("blockstates/{name}.json"));
    let provided = path.pack_is_file()
        || [
            format!("models/block/{name}.json"),
            format!("models/block/{name}_vertical.json"),
            format!("textures/blocks/{name}_front_horizontal.png"),
            format!("textures/blocks/{name}_front_vertical.png"),
        ]
        .iter()
        .any(|relative| directory.join(relative).pack_is_file());
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
        if path.pack_is_file() {
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
    if !vertical.pack_is_file() && horizontal.pack_is_file() {
        fs::copy(&horizontal, &vertical)?;
        if horizontal.with_extension("png.mcmeta").pack_is_file() {
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
    if target < 2 {
        inventory_model(root, name, source, report)?;
    }
    Ok(())
}

pub(super) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    if target >= 4 {
        return Ok(());
    }
    if target < 2 {
        standalone_models(root, report)?;
    }
    directional(root, "dispenser", source, target, report)?;
    directional(root, "dropper", source, target, report)?;
    Ok(())
}
